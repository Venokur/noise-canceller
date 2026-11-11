use std::time::Instant;

use anyhow::{anyhow, Result};
use noise_canceller_core::dsp::Chain;
use noise_canceller_core::paths;
use noise_canceller_core::resample::Resampler;
use noise_canceller_core::settings::Settings;
use noise_canceller_core::{devices, wav, Denoiser};
use noise_canceller_core::{InputVoice, Leveler, PeakGuard, Pipeline, SILENCE_SKIP_BLOCKS};

fn main() -> Result<()> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "selftest".into());

    match mode.as_str() {
        "devices" => {
            println!("input:");
            for (i, name) in devices::input_devices().iter().enumerate() {
                println!("  [{i}] {name}");
            }
            println!("output:");
            for (i, name) in devices::output_devices().iter().enumerate() {
                println!("  [{i}] {name}");
            }
            Ok(())
        }
        "run" => run_engine(),
        "profile" => record_profile(),
        "ab" => record_ab(),
        "selftest" => selftest(),
        "settings" => show_settings(),
        "denoise" => denoise_file(),
        "replay" => replay_file(),
        "metrics" => metrics_file(),
        other => Err(anyhow!("unknown mode: {other} (use selftest|devices)")),
    }
}

/// Metrics for one A/B pair, and the regression verdict.
///
/// `--check` turns the same numbers into a pass/fail with the thresholds from
/// `playback`: this replaced a second implementation in Python, and the two had
/// drifted (26.7 vs 27.4 dB on a real take).
fn metrics_file() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let json = args.iter().any(|a| a == "--json");
    let check = args.iter().any(|a| a == "--check");
    let files: Vec<&str> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(String::as_str)
        .collect();
    if files.len() != 2 {
        return Err(anyhow!(
            "usage: nc metrics <raw.wav> <clean.wav> [--json] [--check]"
        ));
    }

    let m = noise_canceller_core::playback::ab_metrics(
        std::path::Path::new(files[0]),
        std::path::Path::new(files[1]),
    )?;

    if json {
        println!("{}", serde_json::to_string(&m)?);
    } else {
        println!(
            "speech {:.1} dB, level {:+.1} dB, contrast {:.1} dB, dropout {:.2} s",
            m.speech_level_db, m.level_delta_db, m.pause_contrast_db, m.longest_dropout_s
        );
    }

    if check {
        let problems = m.problems();
        for problem in &problems {
            eprintln!("FAIL: {problem}");
        }
        if !problems.is_empty() {
            std::process::exit(1);
        }
    }
    Ok(())
}

fn run_engine() -> Result<()> {
    use std::time::Duration;

    let seconds: u64 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let in_index: i32 = std::env::args()
        .nth(3)
        .and_then(|v| v.parse().ok())
        .unwrap_or(-1);
    let out_index: i32 = std::env::args()
        .nth(4)
        .and_then(|v| v.parse().ok())
        .unwrap_or(-1);

    let pick = |list: Vec<String>, index: i32| -> Option<String> {
        if index < 0 {
            None
        } else {
            list.get(index as usize).cloned()
        }
    };

    let input = pick(devices::input_devices(), in_index);
    let output = pick(devices::output_devices(), out_index);

    let threads: u16 = std::env::args()
        .nth(5)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut settings = Settings::default();
    if threads > 0 {
        settings.denoiser_threads = threads;
    }

    let mut engine =
        noise_canceller_core::engine::Engine::start(settings, input, output, None, true)?;

    println!("engine started, {seconds}s");

    let mut previous = engine.clock_stats();

    for tick in 0..seconds * 2 {
        std::thread::sleep(Duration::from_millis(500));
        let s = engine.stats();
        let now = engine.clock_stats();
        let pushed = (now.1 - previous.1) as f32 * 2.0;
        let pulled = (now.2 - previous.2) as f32 * 2.0;
        let captured = (now.0 - previous.0) as f32 * 2.0;
        previous = now;
        println!(
            "  {:>4.1}s  in={:.4} out={:.4} rtf={:.3} latency={:.0}ms underruns={} cap={:.0} push={:.0} pull={:.0}",
            (tick + 1) as f32 * 0.5,
            s.input_rms,
            s.output_rms,
            s.rtf,
            s.latency_ms,
            s.underruns,
            captured,
            pushed,
            pulled
        );
    }

    engine.stop();
    Ok(())
}

fn record_profile() -> Result<()> {
    let seconds: f32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(5.0);

    let profile = noise_canceller_core::profile::record(None, seconds)?;
    println!("saved {}", profile.path);
    println!(
        "duration={:.1}s speech={:.0}% snr={:.1}dB peak={:.1}dB clip={:.2}% score={}",
        profile.quality.duration,
        profile.quality.speech_percent * 100.0,
        profile.quality.snr_db,
        profile.quality.peak_db,
        profile.quality.clip_percent,
        profile.quality.score
    );
    Ok(())
}

fn record_ab() -> Result<()> {
    use std::time::Duration;

    let seconds: f32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(3.0);

    let mut engine =
        noise_canceller_core::engine::Engine::start(Settings::default(), None, None, None, true)?;

    std::thread::sleep(Duration::from_millis(800));
    engine.start_ab(seconds)?;

    for _ in 0..(seconds as u64 * 10 + 30) {
        std::thread::sleep(Duration::from_millis(200));
        let status = engine.ab_status();
        if !status.running {
            println!("done: {:?}", status.folder);
            engine.stop();
            return Ok(());
        }
    }

    engine.stop();
    Err(anyhow!("a/b did not finish in time"))
}

/// Runs the whole chain offline: leveler → peak guard → model → DSP.
/// Used to compare tuning on a real A/B recording without speaking live.
/// TEMP: lets the offline comparison point at other VoxLock builds without
/// touching the app's model folder.
fn enroll_path() -> std::path::PathBuf {
    std::env::var("NC_ENROLL")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| paths::enroll_model())
}

fn chunk_path() -> std::path::PathBuf {
    std::env::var("NC_CHUNK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| paths::chunk_model())
}

fn replay_file() -> Result<()> {
    let input = std::env::args()
        .nth(2)
        .ok_or_else(|| anyhow!("usage: nc replay <in.wav> <out.wav> [profile.wav]"))?;
    let output = std::env::args()
        .nth(3)
        .ok_or_else(|| anyhow!("usage: nc replay <in.wav> <out.wav> [profile.wav]"))?;
    let profile = std::env::args().nth(4).map(std::path::PathBuf::from);
    let settings_path = std::env::args().nth(5).map(std::path::PathBuf::from);

    let (samples, rate) = wav::read_mono(std::path::Path::new(&input))?;

    let mut down = Resampler::new(rate, 16_000);
    let mut at16 = vec![0.0f32; down.max_output(samples.len()) + 64];
    let len = down.process(&samples, &mut at16);
    at16.truncate(len);

    let settings = match &settings_path {
        Some(path) => Settings::load(path),
        None => Settings::default(),
    };
    let chunk = noise_canceller_core::engine::block_samples(settings.chunk_ms);

    // the live engine always feeds full blocks; pad the tail so replay does too
    // and the model stages see the same lengths as in the app
    let rem = at16.len() % chunk;
    if rem != 0 {
        at16.resize(at16.len() + (chunk - rem), 0.0);
    }

    // the live engine starts the leveler from the gain it remembered in
    // data/state.json, so replay has to do the same or the first phrase comes
    // out quiet and slowly rises — a difference that is not in the real output
    let mut start_db = 0.0f32;
    if let Some(dir) = settings_path.as_ref().and_then(|p| p.parent()) {
        start_db = noise_canceller_core::state::LearnedState::load(dir).leveler_gain_db;
    }
    let mut leveler = Leveler::new(start_db);
    let mut guard = PeakGuard::new(16_000.0);
    // the same chain as the live engine, built by the same code
    let mut pipeline = Pipeline::new(
        &settings,
        profile.as_deref().filter(|p| p.exists()),
        settings.effective_threads(noise_canceller_core::engine::block_ms(chunk)),
        chunk,
        &enroll_path(),
        &chunk_path(),
    )?;

    let mut up = Resampler::new(16_000, rate);
    let mut chain = Chain::new();
    chain.configure(&settings, rate);

    let mut out: Vec<f32> = Vec::with_capacity(at16.len() * 3 + 64);
    let mut up_buf = vec![0.0f32; up.max_output(chunk) + 64];
    let mut input_voice = InputVoice::new();
    let mut block_index = 0usize;
    let mut was_speaking = false;
    let mut onset_blocks = 0u32;
    let mut agc_lo = f64::INFINITY;
    let mut agc_hi = f64::NEG_INFINITY;
    let mut vad_true = 0u32;
    let mut vad_all = 0u32;
    let mut written_total = 0usize;
    let mut last_trace = -1i32;
    let trace = std::env::var("NC_TRACE").is_ok();
    let mut stats_vox = (0usize, 0usize, 0usize);
    let zeros = vec![0.0f32; chunk];
    let mut silent_run = 0u32;
    for block in at16.chunks_mut(chunk) {
        // same silence skip as the live engine, so replay reproduces what the
        // app does after a quiet room — including the gate state
        let raw_rms = (block.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>()
            / block.len().max(1) as f64)
            .sqrt() as f32;
        block_index += 1;
        let in_voice = input_voice.update(block);
        if raw_rms < 6e-4 {
            silent_run += 1;
        } else {
            silent_run = 0;
        }
        if silent_run > SILENCE_SKIP_BLOCKS {
            // same as the live engine: keep the pre-roll current and leave the
            // gate open, the detector has not seen audio for a while
            pipeline.skip_silence(block);
            let produced = up.process(&zeros, &mut up_buf);
            for sample in up_buf[..produced].iter_mut() {
                *sample = 0.0;
            }
            out.extend_from_slice(&up_buf[..produced]);
            continue;
        }

        let allow_up = out.len() as f32 / rate as f32 > 1.5;
        leveler.process(block, settings.input_leveling, allow_up, in_voice);
        guard.process(block);

        let wet = settings.isolation_strength.clamp(0.0, 1.0);
        let warm = block_index as f32 * chunk as f32 / 16_000.0 > 1.0;
        let out_block = pipeline.process(block, wet, warm, in_voice);

        if out_block.leak_reset && trace {
            eprintln!(
                "leak guard: reset at {:.2} s (in {:.0} dB, atten {:.0} dB)",
                block_index as f32 * chunk as f32 / 16_000.0,
                20.0 * out_block.in_rms.max(1e-9).log10(),
                out_block.atten_db
            );
        }

        if out_block.cold_reset && trace {
            eprintln!(
                "cold start: reset while the model was closed at {:.2} s (in {:.0} dB)",
                block_index as f32 * chunk as f32 / 16_000.0,
                20.0 * raw_rms.max(1e-9).log10()
            );
        }
        stats_vox.0 += out_block.model_in_len;
        stats_vox.1 += out_block.model_out_len;
        if out_block.model_out_len != out_block.model_in_len {
            stats_vox.2 += 1;
        }

        // same onset logic as the live engine
        let rms = (block.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>()
            / block.len().max(1) as f64)
            .sqrt() as f32;
        let peak = block.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let crest = 20.0 * ((peak + 1e-9) / (rms + 1e-9)).log10();
        let speaking = rms > 1.8e-3;
        let rms_db = 20.0 * (rms + 1e-9).log10();
        let knock = crest > 19.0 && rms_db < -50.0;
        let onset_now = (speaking && !was_speaking) || knock;
        if onset_now {
            onset_blocks = 5;
        }
        was_speaking = speaking;

        // same target as the live engine: the output detector must agree with
        // the mic that the user is talking, otherwise an audiobook is lifted
        let vad_ok = out_block.target_vad.unwrap_or(out_block.target_model);
        chain.set_target_present((in_voice && vad_ok) || out_block.cap_active);
        if let Some(speech) = out_block.target_vad {
            if speech {
                vad_true += 1;
            }
            vad_all += 1;
        }
        let produced = up.process(out_block.wet, &mut up_buf);
        if trace {
            let t = written_total as f32 / rate as f32;
            if (t * 4.0) as i32 > last_trace {
                last_trace = (t * 4.0) as i32;
                eprintln!(
                    "t {t:.2} in {:.0} out {:.0} atten {:.0} cap {} voice {} vad {} level {:.1} agc {:.1}",
                    20.0 * out_block.in_rms.max(1e-9).log10(),
                    20.0 * out_block.out_rms.max(1e-9).log10(),
                    out_block.atten_db,
                    u8::from(out_block.cap_active),
                    u8::from(in_voice),
                    u8::from(out_block.target_vad.unwrap_or(false)),
                    leveler.gain_db(),
                    chain.agc_gain_db(),
                );
            }
        }
        chain.process(&mut up_buf[..produced]);
        let g = chain.agc_gain_db();
        agc_lo = agc_lo.min(g);
        agc_hi = agc_hi.max(g);
        if onset_blocks > 0 {
            let boost_db = 3.0 * (onset_blocks as f32 / 5.0);
            let lin = 10f32.powf(boost_db / 20.0);
            for sample in up_buf[..produced].iter_mut() {
                *sample = (*sample * lin).clamp(-1.0, 1.0);
            }
        }
        onset_blocks = onset_blocks.saturating_sub(1);
        out.extend_from_slice(&up_buf[..produced]);
        written_total += produced;
    }

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&output, spec)?;
    for sample in &out {
        writer.write_sample((sample.clamp(-1.0, 1.0) * 32767.0) as i16)?;
    }
    writer.finalize()?;

    let rms = |v: &[f32]| {
        let s: f64 = v.iter().map(|x| (*x as f64) * (*x as f64)).sum();
        20.0 * ((s / v.len().max(1) as f64).sqrt() as f32 + 1e-12).log10()
    };
    if std::env::var("NC_TRACE").is_ok() {
        eprintln!(
            "voxlock fed {} got {} mismatch blocks {}",
            stats_vox.0, stats_vox.1, stats_vox.2
        );
    }
    println!(
        "in {:.1} dB, out {:.1} dB, level {:.1} dB, agc {:.1}..{:.1} dB, {} samples @ {rate} Hz, profile {}",
        rms(&at16),
        rms(&out),
        leveler.gain_db(),
        agc_lo,
        agc_hi,
        out.len(),
        if profile.is_some() { "on" } else { "off" }
    );
    println!("vad true {}/{} blocks", vad_true, vad_all.max(1));
    Ok(())
}

fn denoise_file() -> Result<()> {
    use noise_canceller_core::dsp::Biquad;

    let input = std::env::args()
        .nth(2)
        .ok_or_else(|| anyhow!("usage: nc denoise <in.wav> <out.wav> [hp_hz]"))?;
    let output = std::env::args()
        .nth(3)
        .ok_or_else(|| anyhow!("usage: nc denoise <in.wav> <out.wav> [hp_hz]"))?;
    let hp_hz: f32 = std::env::args()
        .nth(4)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let profile = std::env::args().nth(5).map(std::path::PathBuf::from);

    let (samples, rate) = wav::read_mono(std::path::Path::new(&input))?;

    let mut resampler = Resampler::new(rate, 16_000);
    let mut at16 = vec![0.0f32; resampler.max_output(samples.len()) + 64];
    let len = resampler.process(&samples, &mut at16);
    let mut at16 = at16[..len].to_vec();

    if hp_hz > 0.0 {
        let mut hp = Biquad::default();
        hp.high_pass(16_000.0, hp_hz, std::f64::consts::FRAC_1_SQRT_2);
        let mut a = Biquad::default();
        a.high_pass(16_000.0, hp_hz, std::f64::consts::FRAC_1_SQRT_2);
        for sample in at16.iter_mut() {
            let v = hp.process(*sample as f64);
            *sample = a.process(v) as f32;
        }
    }

    let mut denoiser = Denoiser::new(
        &enroll_path(),
        &chunk_path(),
        profile.as_deref().filter(|p| p.exists()),
        4,
    )?;
    let chunk = 960;
    let mut out = Vec::with_capacity(at16.len());
    for block in at16.chunks_exact(chunk) {
        out.extend_from_slice(&denoiser.run(block));
    }
    out.extend_from_slice(&denoiser.flush());

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&output, spec)?;
    for sample in &out {
        writer.write_sample((sample.clamp(-1.0, 1.0) * 32767.0) as i16)?;
    }
    writer.finalize()?;

    let rms = |v: &[f32]| {
        let s: f64 = v.iter().map(|x| (*x as f64) * (*x as f64)).sum();
        (s / v.len().max(1) as f64).sqrt() as f32
    };
    println!(
        "in {} samples, out {} samples, hp {hp_hz} Hz, rms in {:.4} out {:.4}",
        at16.len(),
        out.len(),
        rms(&at16),
        rms(&out)
    );
    Ok(())
}

fn show_settings() -> Result<()> {
    let path = std::env::args()
        .nth(2)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| paths::data_dir().join("settings.json"));

    let s = Settings::load(&path);
    let learned = noise_canceller_core::state::LearnedState::load(
        path.parent().unwrap_or_else(|| std::path::Path::new(".")),
    );
    let block_ms = noise_canceller_core::engine::block_ms(
        noise_canceller_core::engine::block_samples(s.chunk_ms),
    );
    println!("file      {}", path.display());
    println!("rev       {}", s.defaults_rev);
    println!(
        "residual  {} (gtcrn {} · dpdfnet {})",
        s.residual_denoise,
        if paths::gtcrn_model().exists() {
            "present"
        } else {
            "missing"
        },
        if paths::dpdfnet_model().exists() {
            "present"
        } else {
            "missing"
        }
    );
    println!(
        "roots     {} · denoise {}",
        paths::models_dir().display(),
        paths::denoise_dir().display()
    );
    println!(
        "chunk     {} ms (block {block_ms:.0} ms) · threads {} ({}) · buffer {} ms",
        s.chunk_ms,
        s.effective_threads(block_ms),
        if s.auto_threads {
            "auto".to_string()
        } else {
            format!("manual {}", s.denoiser_threads)
        },
        s.buffer_ms
    );
    println!(
        "gate      {} dB · ratio {} · attack {} · release {} · hold {}",
        s.gate_threshold_db, s.gate_ratio, s.gate_attack_ms, s.gate_release_ms, s.gate_hold_ms
    );
    println!(
        "leveling  {} · agc {} (target {} dB, max {} dB)",
        s.input_leveling, s.agc_enabled, s.agc_target_db, s.agc_max_gain_db
    );
    println!(
        "learned   leveler {:.1} dB · auto buffer {} ms",
        learned.leveler_gain_db,
        if learned.auto_buffer_ms > 0.0 {
            format!("{:.0}", learned.auto_buffer_ms)
        } else {
            "—".to_string()
        }
    );
    println!(
        "devices   in «{}» out «{}»",
        s.input_device, s.output_device
    );
    Ok(())
}

fn selftest() -> Result<()> {
    let dir = paths::models_dir();
    let combined = dir.join("test_wavs").join("en-combined.wav");
    let voice = dir.join("test_wavs").join("en-voice-sample-f.wav");

    println!("models: {}", dir.display());

    let (samples16, rate) = wav::read_mono(&combined)?;
    println!(
        "input: {} samples @ {rate} Hz ({:.2}s)",
        samples16.len(),
        samples16.len() as f32 / rate as f32
    );

    // simulate a 48 kHz microphone
    let mut up_mic = Resampler::new(rate, 48_000);
    let mut mic = vec![0.0f32; up_mic.max_output(samples16.len()) + 8];
    let mic_len = up_mic.process(&samples16, &mut mic);
    let mic = &mic[..mic_len];

    // optional input attenuation, to imitate a very quiet microphone
    let atten_db: f32 = std::env::args()
        .nth(4)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let atten = 10f32.powf(atten_db / 20.0);
    let mic: Vec<f32> = mic.iter().map(|v| v * atten).collect();
    let mic = &mic[..];

    // engine downsamples to the model rate
    let mut down = Resampler::new(48_000, 16_000);
    let mut at16 = vec![0.0f32; down.max_output(mic_len) + 8];
    let at16_len = down.process(mic, &mut at16);

    let threads: u16 = std::env::args()
        .nth(3)
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    let mut denoiser = Denoiser::new(&enroll_path(), &chunk_path(), Some(&voice), threads)?;
    println!(
        "denoiser: sr={} shift={}",
        denoiser.sample_rate(),
        denoiser.frame_shift()
    );

    let chunk_ms: usize = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);
    // same rounding as the engine: 60 ms is 1024 samples, not 960
    let chunk = ((16000 * chunk_ms / 1000).max(128) + 64) / 128 * 128;
    let mut isolated = Vec::with_capacity(at16_len);
    let started = Instant::now();
    let mut mismatched = 0usize;
    // the live engine lifts a quiet microphone *before* the model; without the
    // same stage here a -40 dB take would test the gate, not the leveler.
    // The 5th argument seeds the gain the way data/state.json does.
    let start_gain: f32 = std::env::args()
        .nth(5)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let mut leveler = Leveler::new(start_gain);
    let mut input_voice = InputVoice::new();

    for block in at16[..at16_len].chunks_mut(chunk) {
        let second = isolated.len() as f32 / 16_000.0;
        let speaker = input_voice.update(block);
        leveler.process(block, true, second > 1.5, speaker);
        let produced = denoiser.run(block);
        if produced.len() != block.len() {
            mismatched += 1;
        }
        isolated.extend_from_slice(&produced);
    }
    isolated.extend_from_slice(&denoiser.flush());

    let elapsed = started.elapsed().as_secs_f32();
    let audio_seconds = at16_len as f32 / 16_000.0;
    let rtf = elapsed / audio_seconds;

    // back to 48 kHz and through the DSP chain
    let mut up = Resampler::new(16_000, 48_000);
    let mut converted = vec![0.0f32; up.max_output(isolated.len()) + 64];
    let converted_len = up.process(&isolated, &mut converted);

    let mut dsp = converted[..converted_len].to_vec();
    let mut chain = Chain::new();
    chain.configure(&Settings::default(), 48_000);
    chain.process(&mut dsp);

    println!("mic   rms={:.4}", rms(mic));
    println!("level {:.1} dB", leveler.gain_db());
    println!(
        "model rms={:.4} peak={:.3}",
        rms(&isolated),
        peak(&isolated)
    );
    println!("dsp   rms={:.4} peak={:.3}", rms(&dsp), peak(&dsp));

    // the model must hand back what it was given: a short or long stream is what
    // made speech speed up and slow down
    let delta = isolated.len() as i64 - at16_len as i64;
    println!(
        "length in={at16_len} out={} delta={delta} samples",
        isolated.len()
    );
    println!("mismatched blocks: {mismatched}");

    let finite = isolated.iter().all(|s| s.is_finite()) && dsp.iter().all(|s| s.is_finite());
    println!("finite={finite}");

    // silence must stay silent: a burst here is the "explosion" in the earpiece
    let mut silent_model = Denoiser::new(&enroll_path(), &chunk_path(), Some(&voice), threads)?;
    let zeros = vec![0.0f32; chunk];
    let mut silent_out: Vec<f32> = Vec::new();
    for _ in 0..(16_000 * 2 / chunk) {
        silent_out.extend_from_slice(&silent_model.run(&zeros));
    }
    println!(
        "silence rms={:.1} dB peak={:.4}",
        20.0 * rms(&silent_out).max(1e-9).log10(),
        peak(&silent_out)
    );

    println!("rtf={rtf:.3} ({})", if rtf < 1.0 { "OK" } else { "SLOW" });

    Ok(())
}

fn rms(data: &[f32]) -> f32 {
    if data.is_empty() {
        return 0.0;
    }
    let sum: f64 = data.iter().map(|v| (*v as f64) * (*v as f64)).sum();
    (sum / data.len() as f64).sqrt() as f32
}

fn peak(data: &[f32]) -> f32 {
    data.iter().fold(0.0f32, |acc, v| acc.max(v.abs()))
}
