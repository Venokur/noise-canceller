//! WASAPI duplex engine: capture → 16 kHz → VoxLock → DSP → render.
//! Split in four: this file owns the device side and the public `Engine`,
//! `helpers` are the pure building blocks, `ab` the A/B take on disk and
//! `streams` the two audio callbacks.

mod ab;
mod helpers;
mod processor;
mod streams;

pub use ab::AbStatus;
pub use helpers::{
    cap_dry_mix, rms_of, DropoutGuard, FixedStage, InputVoice, LeakGuard, Leveler, PeakGuard,
};

use ab::AbBuffer;
use processor::Processor;
use streams::{install_panic_hook, open_input, open_output};

use crate::pipeline::Pipeline;

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::devices;
use crate::lang::Lang;
use crate::paths;
use crate::recordings;
use crate::settings::{self, Settings};
use crate::state::LearnedState;
use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use hound::{SampleFormat, WavSpec, WavWriter};

pub const MODEL_RATE: u32 = 16_000;
pub const BANDS: usize = 32;

/// The model works on 128-sample hops at 16 kHz, so a requested block is
/// rounded to the nearest multiple: the 60 ms slider runs as 64 ms.
pub fn block_samples(chunk_ms: f32) -> usize {
    let samples = ((MODEL_RATE as f32 * chunk_ms / 1000.0) as usize).max(128);
    ((samples + 64) / 128).max(1) * 128
}

pub fn block_ms(samples: usize) -> f32 {
    samples as f32 * 1000.0 / MODEL_RATE as f32
}

/// Blocks of near-digital silence after which the model is skipped.
///
/// Shared with `nc replay`: a different value there (it used to be 100) meant
/// the offline regression ran a different audio path than the live app.
pub const SILENCE_SKIP_BLOCKS: u32 = 5;

/// Long clean-only take, separate from the 60 s A/B pair.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanRecordStatus {
    pub running: bool,
    /// 0..1
    pub progress: f32,
    pub path: Option<String>,
}

pub struct Stats {
    pub input_rms: f32,
    pub output_rms: f32,
    pub rtf: f32,
    pub latency_ms: f32,
    /// effective output ring buffer (auto or manual), ms
    pub buffer_ms: f32,
    pub underruns: u32,
    /// capture sample rate and channel count, for the "48 kHz · mono" caption
    pub in_rate: u32,
    pub in_channels: u16,
    /// output sample rate, so the UI can turn the sample counter into ms
    pub out_rate: u32,
    pub backend: String,
    pub spectrum: [f32; BANDS],
    /// how much the model is cutting right now, positive dB
    pub suppression_db: f32,
    /// true while the input looks like speech (VAD proxy for the UI)
    pub speech: bool,
    /// peak of the last input callback, dBFS (clip indicator)
    pub input_peak_db: f32,
    /// peak of the rendered output, dBFS
    pub output_peak_db: f32,
    /// output is muted (global hotkey / tray)
    pub muted: bool,
    /// how many times the dropout guard reset VoxLock
    pub guard_resets: u32,
    /// blocks the output ring could not take (they are lost, not repeated)
    pub overflows: u32,
    /// capture samples lost because the input ring was full
    pub capture_drops: u32,
    /// current leveler gain, dB
    pub leveler_gain_db: f32,
    /// clock-drift correction of the async converter, ppm (0 = exactly 1.0)
    pub slew_ppm: f32,
    /// threads the model actually started with (auto or manual)
    pub threads: u16,
    /// 0..1 similarity of the last window to the enrolled profile; negative
    /// means "unknown" (no profile, silence, or the model is off)
    pub speaker_confidence: f32,
    /// momentary loudness of the output, LUFS
    pub lufs: f32,
    /// true-peak estimate of the output, dBTP
    pub true_peak_db: f32,
    /// false when the processor thread died (panic) while the streams live on
    pub healthy: bool,
}

#[derive(Clone)]
struct Shared {
    running: Arc<AtomicBool>,
    input_rms: Arc<AtomicU32>,
    output_rms: Arc<AtomicU32>,
    rtf: Arc<AtomicU32>,
    underruns: Arc<AtomicU32>,
    /// callbacks that could not be filled completely (the auto-buffer signal)
    underrun_events: Arc<AtomicU32>,
    overflows: Arc<AtomicU32>,
    latency: Arc<AtomicU32>,
    settings: Arc<Mutex<Settings>>,
    ab: Arc<Mutex<Option<AbBuffer>>>,
    ab_done: Arc<AtomicBool>,
    /// true from `start_ab` until the wav files are written: the ui polls this
    /// and used to stop as soon as the buffer was taken, before the paths
    /// existed, so the play buttons never appeared
    ab_active: Arc<AtomicBool>,
    ab_folder: Arc<Mutex<Option<String>>>,
    ab_paths: Arc<Mutex<(Option<String>, Option<String>)>>,
    spectrum: Arc<Mutex<[f32; BANDS]>>,
    settings_seq: Arc<AtomicU32>,
    wet: Arc<AtomicU32>,
    /// 0..1 amount of synthesized air above 8 kHz
    air: Arc<AtomicU32>,
    buffer_ms: Arc<AtomicU32>,
    leveling: Arc<AtomicBool>,
    out_fill: Arc<AtomicU32>,
    out_raw: Arc<AtomicU32>,
    suppression: Arc<AtomicU32>,
    speech: Arc<AtomicBool>,
    input_peak: Arc<AtomicU32>,
    output_peak: Arc<AtomicU32>,
    muted: Arc<AtomicBool>,
    guard_resets: Arc<AtomicU32>,
    leveler_gain: Arc<AtomicU32>,
    slew_ppm: Arc<AtomicU32>,
    in_frames: Arc<AtomicU64>,
    pushed: Arc<AtomicU64>,
    pulled: Arc<AtomicU64>,
    /// capture samples dropped because the input ring was full
    capture_drops: Arc<AtomicU32>,
    /// capture channel: -1 mixes all channels, 0.. picks one
    input_channel: Arc<AtomicI32>,
    /// manual input gain before the leveler, dB
    input_gain: Arc<AtomicU32>,
    /// similarity of the live voice to the profile, f32 bits; negative = unknown
    speaker_confidence: Arc<AtomicU32>,
    /// momentary loudness of the output, LUFS
    lufs: Arc<AtomicU32>,
    /// true-peak estimate of the rendered output, dBTP
    true_peak: Arc<AtomicU32>,
    /// duck the output when the speaker confidence is low
    speaker_guard: Arc<AtomicBool>,
    speaker_threshold: Arc<AtomicU32>,
    /// long clean-only recording: producer fed by the processor, writer owns
    /// the consumer and the wav file
    clean_rec: Arc<Mutex<Option<rtrb::Producer<f32>>>>,
    clean_rec_active: Arc<AtomicBool>,
    clean_rec_progress: Arc<AtomicU32>,
    clean_rec_path: Arc<Mutex<Option<String>>>,
    clean_rec_target: Arc<AtomicU32>,
    clean_rec_rate: Arc<AtomicU32>,
    /// frames the output device asked for in the last callback
    callback_frames: Arc<AtomicU32>,
    /// blocks where the model returned NaN/Inf and were zeroed
    bad_blocks: Arc<AtomicU32>,
}

impl Shared {
    fn new(settings: Settings) -> Self {
        Self {
            running: Arc::new(AtomicBool::new(true)),
            input_rms: Arc::new(AtomicU32::new(0)),
            output_rms: Arc::new(AtomicU32::new(0)),
            rtf: Arc::new(AtomicU32::new(0)),
            underruns: Arc::new(AtomicU32::new(0)),
            underrun_events: Arc::new(AtomicU32::new(0)),
            overflows: Arc::new(AtomicU32::new(0)),
            latency: Arc::new(AtomicU32::new(0)),
            settings: Arc::new(Mutex::new(settings.clone())),
            ab: Arc::new(Mutex::new(None)),
            ab_done: Arc::new(AtomicBool::new(false)),
            ab_active: Arc::new(AtomicBool::new(false)),
            ab_folder: Arc::new(Mutex::new(None)),
            ab_paths: Arc::new(Mutex::new((None, None))),
            spectrum: Arc::new(Mutex::new([0.0; BANDS])),
            settings_seq: Arc::new(AtomicU32::new(1)),
            wet: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            air: Arc::new(AtomicU32::new(settings.air_amount.to_bits())),
            buffer_ms: Arc::new(AtomicU32::new(50.0f32.to_bits())),
            leveling: Arc::new(AtomicBool::new(settings.input_leveling)),
            out_fill: Arc::new(AtomicU32::new(0)),
            out_raw: Arc::new(AtomicU32::new(0)),
            suppression: Arc::new(AtomicU32::new(0)),
            speech: Arc::new(AtomicBool::new(false)),
            input_peak: Arc::new(AtomicU32::new(0)),
            output_peak: Arc::new(AtomicU32::new(0)),
            muted: Arc::new(AtomicBool::new(false)),
            guard_resets: Arc::new(AtomicU32::new(0)),
            leveler_gain: Arc::new(AtomicU32::new(0)),
            slew_ppm: Arc::new(AtomicU32::new(0)),
            in_frames: Arc::new(AtomicU64::new(0)),
            pushed: Arc::new(AtomicU64::new(0)),
            pulled: Arc::new(AtomicU64::new(0)),
            capture_drops: Arc::new(AtomicU32::new(0)),
            input_channel: Arc::new(AtomicI32::new(settings.input_channel)),
            input_gain: Arc::new(AtomicU32::new(settings.input_gain_db.to_bits())),
            speaker_confidence: Arc::new(AtomicU32::new((-1.0f32).to_bits())),
            lufs: Arc::new(AtomicU32::new((-120.0f32).to_bits())),
            true_peak: Arc::new(AtomicU32::new((-120.0f32).to_bits())),
            speaker_guard: Arc::new(AtomicBool::new(settings.speaker_guard)),
            speaker_threshold: Arc::new(AtomicU32::new(
                settings.speaker_threshold.clamp(0.05, 0.95).to_bits(),
            )),
            clean_rec: Arc::new(Mutex::new(None)),
            clean_rec_active: Arc::new(AtomicBool::new(false)),
            clean_rec_progress: Arc::new(AtomicU32::new(0)),
            clean_rec_path: Arc::new(Mutex::new(None)),
            clean_rec_target: Arc::new(AtomicU32::new(0)),
            clean_rec_rate: Arc::new(AtomicU32::new(48_000)),
            callback_frames: Arc::new(AtomicU32::new(0)),
            bad_blocks: Arc::new(AtomicU32::new(0)),
        }
    }
}

/// Rate converter with a deliberately slow controller. The microphone clock and
/// the output clock differ by a fraction of a percent; correcting that by moving
/// the read position is fine, correcting it quickly is not — a fast controller
/// bends pitch and timbre, which is audible as "the voice speeds up and slows
/// down". The step therefore moves by at most a fraction of a percent every
/// couple of seconds.
/// Writes the learned state (`data/state.json`) off the processing thread.
///
/// `data/` sits next to the exe, and on a user's machine that is often a
/// OneDrive folder: a `fs::write` there can block for tens of milliseconds
/// while the output ring holds about 120 ms. The processing thread used to do
/// these writes itself, so a sync hiccup was audible as a hole.
///
/// The thread owns the file: messages only carry one field, the thread merges it
/// into the state it already has and serializes the whole JSON itself, so a
/// write can never leave the file with one value updated and the other stale.
struct Persist {
    tx: std::sync::mpsc::Sender<PersistMsg>,
}

enum PersistMsg {
    AutoBuffer(f32),
    LevelerGain(f32),
}

impl Persist {
    fn spawn(dir: std::path::PathBuf, mut state: LearnedState) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<PersistMsg>();
        let _ = thread::Builder::new()
            .name("nc-persist".into())
            .spawn(move || {
                while let Ok(msg) = rx.recv() {
                    match msg {
                        PersistMsg::AutoBuffer(ms) => state.auto_buffer_ms = ms,
                        PersistMsg::LevelerGain(db) => state.leveler_gain_db = db,
                    }
                    state.save(&dir);
                }
            });
        Self { tx }
    }

    fn set_auto_buffer(&self, ms: f32) {
        // the receiver only goes away when the engine stops; losing a state
        // update is not worth blocking the audio path
        let _ = self.tx.send(PersistMsg::AutoBuffer(ms));
    }

    fn set_leveler_gain(&self, db: f32) {
        let _ = self.tx.send(PersistMsg::LevelerGain(db));
    }
}

/// Fades the "Подавление" readout while nothing is being suppressed.
///
/// The number is the model's attenuation on speech blocks, smoothed per block.
/// It used to be written only on speech, so it froze at the last phrase and the
/// app looked like it kept cutting the room in silence; here it decays in about
/// half a second instead.
fn fade_suppression(shared: &Shared) {
    let current = f32::from_bits(shared.suppression.load(Ordering::Relaxed));
    let next = if current * 0.9 < 0.05 {
        0.0
    } else {
        current * 0.9
    };
    shared.suppression.store(next.to_bits(), Ordering::Relaxed);
}

pub struct Engine {
    shared: Shared,
    input: Option<cpal::Stream>,
    output: Option<cpal::Stream>,
    processor: Option<JoinHandle<()>>,
    backend: String,
    in_rate: u32,
    in_channels: u16,
    out_rate: u32,
    threads: u16,
    speaker: Option<crate::speaker::GuardHandle>,
}

impl Engine {
    pub fn start(
        settings: Settings,
        input_name: Option<String>,
        output_name: Option<String>,
        voice: Option<std::path::PathBuf>,
        test_mode: bool,
    ) -> Result<Self> {
        install_panic_hook();

        let lang = Lang::from_settings(&settings);
        let host = cpal::default_host();
        let input = devices::pick_input(&host, input_name.as_deref(), lang)?;
        // the cable's capture side is our own output: processing it is a
        // feedback loop (and every app playing into the cable is "the speaker")
        if let Some(name) = devices::name_of(&input) {
            if devices::is_cable_name(&name) {
                return Err(anyhow!(lang.fmt(
                    "«{name}» — это виртуальный кабель, а не микрофон: приложение услышит собственный выход. Выберите настоящий микрофон.",
                    "“{name}” is a virtual cable, not a microphone: the app would hear its own output. Choose a real microphone.",
                    &[("name", &name)],
                )));
            }
        }

        // a normal run must never speak into the speakers: that is what the
        // explicit "test in speakers" mode is for
        let output = if test_mode {
            devices::pick_output(&host, output_name.as_deref(), lang)?
        } else {
            match devices::find_cable(&host) {
                Some(device) => device,
                None => {
                    return Err(anyhow!(lang.text(
                        "виртуальный микрофон не найден — установите VB-CABLE или включите проверку в колонках",
                        "the virtual microphone was not found — install VB-CABLE or turn on the speaker check",
                    )))
                }
            }
        };

        let in_cfg = input.default_input_config()?;
        let out_cfg = output.default_output_config()?;

        let in_rate = in_cfg.sample_rate();
        let in_channels = in_cfg.channels();
        let out_rate = out_cfg.sample_rate();

        let chunk_samples = block_samples(settings.chunk_ms);
        let chunk_ms = block_ms(chunk_samples);

        // the automatic pick uses the *rounded* block, not the slider value
        let threads = settings.effective_threads(chunk_ms);

        // the model chain lives in one place, shared with `nc replay`
        let pipeline = Pipeline::new(
            &settings,
            voice.as_deref(),
            threads,
            chunk_samples,
            &paths::enroll_model(),
            &paths::chunk_model(),
        )?;
        let backend = if settings.voice_isolation {
            lang.fmt(
                "VoxLock · CPU · {ms} мс блок",
                "VoxLock · CPU · {ms} ms block",
                &[("ms", &(chunk_ms.round() as u32).to_string())],
            )
        } else {
            lang.fmt(
                "Шумодав без изоляции · {ms} мс блок",
                "Denoiser without isolation · {ms} ms block",
                &[("ms", &(chunk_ms.round() as u32).to_string())],
            )
        };

        let shared = Shared::new(settings.clone());
        shared.wet.store(
            settings.isolation_strength.clamp(0.0, 1.0).to_bits(),
            Ordering::Relaxed,
        );

        // Speaker confidence: a second opinion about who is talking. The model
        // is optional; without it (or without a profile) the meter stays "—".
        let mut speaker_in = None;
        let mut speaker = None;
        if settings.speaker_check && settings.voice_isolation {
            if let Some(profile) = voice.as_ref().filter(|path| path.exists()) {
                if paths::speaker_model().exists() {
                    let (producer, consumer) = rtrb::RingBuffer::<f32>::new(16_000 * 4);
                    speaker = Some(crate::speaker::spawn(
                        paths::speaker_model(),
                        profile.clone(),
                        consumer,
                        shared.speaker_confidence.clone(),
                        shared.running.clone(),
                    ));
                    speaker_in = Some(producer);
                } else {
                    crate::log::info("speaker: campplus_en.onnx is missing, confidence is off");
                }
            }
        }

        // the auto buffer is learned between sessions: start from the stored
        // value (`data/state.json`), otherwise from the manual one; the ring is
        // prefilled to it
        let learned = LearnedState::load(&paths::data_dir());
        // read once, here, before the streams run: the processing thread must
        // not read or write this folder
        let learned_buffer_ms = if learned.auto_buffer_ms > 0.0 {
            learned.auto_buffer_ms
        } else {
            settings.buffer_ms
        }
        .clamp(settings::BUFFER_MS_MIN, settings::BUFFER_MS_MAX);
        let buffer_ms = if settings.buffer_auto {
            learned_buffer_ms
        } else {
            settings
                .buffer_ms
                .clamp(settings::BUFFER_MS_MIN, settings::BUFFER_MS_MAX)
        };
        shared
            .buffer_ms
            .store(buffer_ms.to_bits(), Ordering::Relaxed);

        // learned state is written by a helper thread: `data/` may be a synced
        // folder and a blocking write on the audio path is audible
        let persist = Persist::spawn(paths::data_dir(), learned);

        // rings ------------------------------------------------------------
        let (in_prod, in_cons) = rtrb::RingBuffer::<f32>::new(MODEL_RATE as usize * 2);
        let (mut out_prod, out_cons) = rtrb::RingBuffer::<f32>::new(out_rate as usize * 2);

        // prefill the output ring so playback starts cleanly: one block plus the
        // buffer, otherwise the first block arrives too late and starts with a
        // hole right away
        let prefill = (out_rate as f32 * (buffer_ms + chunk_ms + 24.0) / 1000.0) as usize;
        for _ in 0..prefill {
            let _ = out_prod.push(0.0);
        }

        // processing thread ------------------------------------------------
        let processor = Processor::new(
            shared.clone(),
            &settings,
            pipeline,
            persist,
            out_rate,
            buffer_ms,
            learned_buffer_ms,
            learned.leveler_gain_db,
            speaker_in,
        );
        let processor = thread::Builder::new()
            .name("voxlock-processor".into())
            .spawn(move || processor.run(in_cons, out_prod))
            .map_err(|e| anyhow!("processor thread: {e}"))?;

        let input_stream = open_input(&input, &in_cfg, shared.clone(), in_prod)?;
        let output_stream = open_output(&output, &out_cfg, shared.clone(), out_cons)?;

        input_stream.play()?;
        output_stream.play()?;

        crate::log::info(&format!(
            "engine start: in='{}' {} Hz, out='{}' {} Hz, block {} ms, {} threads, buffer {:.0} ms{}",
            input_name.as_deref().unwrap_or("default"),
            in_rate,
            if test_mode {
                output_name.as_deref().unwrap_or("default")
            } else {
                "VB-CABLE"
            },
            out_rate,
            chunk_ms,
            threads,
            buffer_ms,
            if settings.buffer_auto { ", auto" } else { "" }
        ));

        Ok(Self {
            shared,
            input: Some(input_stream),
            output: Some(output_stream),
            processor: Some(processor),
            backend,
            in_rate,
            in_channels,
            out_rate,
            threads,
            speaker,
        })
    }

    /// Starts a long clean-only recording. The processor pushes the final
    /// (post-DSP) block into a ring; a separate thread owns the wav writer, so
    /// the audio path never touches the disk.
    pub fn start_clean_recording(&self, seconds: f32) -> Result<CleanRecordStatus> {
        if self.shared.clean_rec_active.load(Ordering::Relaxed) {
            return Err(anyhow!("a clean recording is already running"));
        }
        let seconds = seconds.clamp(5.0, 1800.0);
        let target = (self.out_rate as f32 * seconds) as u32;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = recordings::clean_take(stamp);
        let (producer, mut consumer) = rtrb::RingBuffer::<f32>::new(self.out_rate as usize * 2);
        if let Ok(mut slot) = self.shared.clean_rec.lock() {
            *slot = Some(producer);
        }
        self.shared
            .clean_rec_target
            .store(target, Ordering::Relaxed);
        self.shared
            .clean_rec_rate
            .store(self.out_rate, Ordering::Relaxed);
        self.shared
            .clean_rec_progress
            .store(0.0f32.to_bits(), Ordering::Relaxed);
        if let Ok(mut slot) = self.shared.clean_rec_path.lock() {
            *slot = Some(path.to_string_lossy().to_string());
        }
        let active = self.shared.clean_rec_active.clone();
        let progress = self.shared.clean_rec_progress.clone();
        let path_slot = self.shared.clean_rec_path.clone();
        let rate = self.out_rate;
        active.store(true, Ordering::Relaxed);

        std::thread::Builder::new()
            .name("nc-clean-rec".into())
            .spawn(move || {
                let spec = WavSpec {
                    channels: 1,
                    sample_rate: rate,
                    bits_per_sample: 16,
                    sample_format: SampleFormat::Int,
                };
                let mut writer = match WavWriter::create(&path, spec) {
                    Ok(writer) => writer,
                    Err(err) => {
                        crate::log::info(&format!("clean recording: {err}"));
                        active.store(false, Ordering::Relaxed);
                        return;
                    }
                };
                let mut written = 0u32;
                let mut pending: Vec<f32> = Vec::with_capacity(8192);
                while active.load(Ordering::Relaxed) && written < target {
                    while let Ok(sample) = consumer.pop() {
                        pending.push(sample);
                        if pending.len() >= 4096 {
                            for value in pending.drain(..) {
                                let _ =
                                    writer.write_sample((value.clamp(-1.0, 1.0) * 32767.0) as i16);
                            }
                            written += 4096;
                            progress.store(
                                (written as f32 / target.max(1) as f32)
                                    .clamp(0.0, 1.0)
                                    .to_bits(),
                                Ordering::Relaxed,
                            );
                        }
                    }
                    for value in pending.drain(..) {
                        let _ = writer.write_sample((value.clamp(-1.0, 1.0) * 32767.0) as i16);
                    }
                    written = written.min(target);
                    progress.store(
                        (written as f32 / target.max(1) as f32)
                            .clamp(0.0, 1.0)
                            .to_bits(),
                        Ordering::Relaxed,
                    );
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                // drain what is still in the ring so the last syllable is kept
                while let Ok(value) = consumer.pop() {
                    let _ = writer.write_sample((value.clamp(-1.0, 1.0) * 32767.0) as i16);
                }
                let _ = writer.finalize();
                active.store(false, Ordering::Relaxed);
                progress.store(1.0f32.to_bits(), Ordering::Relaxed);
                if let Ok(mut slot) = path_slot.lock() {
                    *slot = Some(path.to_string_lossy().to_string());
                }
                crate::log::info(&format!("clean recording saved: {}", path.display()));
            })
            .map_err(|e| anyhow!("clean recording thread: {e}"))?;

        Ok(self.clean_record_status())
    }

    pub fn stop_clean_recording(&self) {
        self.shared.clean_rec_active.store(false, Ordering::Relaxed);
        if let Ok(mut slot) = self.shared.clean_rec.lock() {
            slot.take();
        }
    }

    pub fn clean_record_status(&self) -> CleanRecordStatus {
        let path = self
            .shared
            .clean_rec_path
            .lock()
            .ok()
            .and_then(|slot| slot.clone());
        CleanRecordStatus {
            running: self.shared.clean_rec_active.load(Ordering::Relaxed),
            progress: f32::from_bits(self.shared.clean_rec_progress.load(Ordering::Relaxed)),
            path,
        }
    }

    pub fn update_settings(&self, settings: &Settings) {
        if let Ok(mut guard) = self.shared.settings.lock() {
            *guard = settings.clone();
        }
        self.shared.wet.store(
            settings.isolation_strength.clamp(0.0, 1.0).to_bits(),
            Ordering::Relaxed,
        );
        self.shared.air.store(
            settings.air_amount.clamp(0.0, 1.0).to_bits(),
            Ordering::Relaxed,
        );
        // in auto mode the processor owns the buffer; do not overwrite the
        // learned value with the manual slider
        if !settings.buffer_auto {
            self.shared
                .buffer_ms
                .store(settings.buffer_ms.to_bits(), Ordering::Relaxed);
        }
        self.shared
            .leveling
            .store(settings.input_leveling, Ordering::Relaxed);
        self.shared
            .input_channel
            .store(settings.input_channel, Ordering::Relaxed);
        self.shared
            .input_gain
            .store(settings.input_gain_db.to_bits(), Ordering::Relaxed);
        self.shared
            .speaker_guard
            .store(settings.speaker_guard, Ordering::Relaxed);
        self.shared.speaker_threshold.store(
            settings.speaker_threshold.clamp(0.05, 0.95).to_bits(),
            Ordering::Relaxed,
        );
        self.shared.settings_seq.fetch_add(1, Ordering::Relaxed);
    }

    /// (capture frames, samples pushed to the output ring, samples played)
    pub fn clock_stats(&self) -> (u64, u64, u64) {
        (
            self.shared.in_frames.load(Ordering::Relaxed),
            self.shared.pushed.load(Ordering::Relaxed),
            self.shared.pulled.load(Ordering::Relaxed),
        )
    }

    pub fn stats(&self) -> Stats {
        Stats {
            input_rms: f32::from_bits(self.shared.input_rms.load(Ordering::Relaxed)),
            output_rms: f32::from_bits(self.shared.output_rms.load(Ordering::Relaxed)),
            rtf: f32::from_bits(self.shared.rtf.load(Ordering::Relaxed)),
            latency_ms: f32::from_bits(self.shared.latency.load(Ordering::Relaxed)),
            buffer_ms: f32::from_bits(self.shared.buffer_ms.load(Ordering::Relaxed)),
            underruns: self.shared.underruns.load(Ordering::Relaxed),
            in_rate: self.in_rate,
            in_channels: self.in_channels,
            out_rate: self.out_rate,
            overflows: self.shared.overflows.load(Ordering::Relaxed),
            capture_drops: self.shared.capture_drops.load(Ordering::Relaxed),
            backend: self.backend.clone(),
            spectrum: self
                .shared
                .spectrum
                .lock()
                .map(|s| *s)
                .unwrap_or([0.0; BANDS]),
            suppression_db: f32::from_bits(self.shared.suppression.load(Ordering::Relaxed)),
            speech: self.shared.speech.load(Ordering::Relaxed),
            input_peak_db: 20.0
                * f32::from_bits(self.shared.input_peak.load(Ordering::Relaxed))
                    .max(1e-6)
                    .log10(),
            output_peak_db: 20.0
                * f32::from_bits(self.shared.output_peak.load(Ordering::Relaxed))
                    .max(1e-6)
                    .log10(),
            muted: self.shared.muted.load(Ordering::Relaxed),
            guard_resets: self.shared.guard_resets.load(Ordering::Relaxed),
            leveler_gain_db: f32::from_bits(self.shared.leveler_gain.load(Ordering::Relaxed)),
            slew_ppm: f32::from_bits(self.shared.slew_ppm.load(Ordering::Relaxed)),
            threads: self.threads,
            speaker_confidence: f32::from_bits(
                self.shared.speaker_confidence.load(Ordering::Relaxed),
            ),
            lufs: f32::from_bits(self.shared.lufs.load(Ordering::Relaxed)),
            true_peak_db: f32::from_bits(self.shared.true_peak.load(Ordering::Relaxed)),
            healthy: self.healthy(),
        }
    }

    /// The processing thread is still running. A panic there would leave the
    /// audio streams alive but silent, so the UI watches this and restarts.
    pub fn healthy(&self) -> bool {
        self.processor
            .as_ref()
            .map(|handle| !handle.is_finished())
            .unwrap_or(false)
    }

    /// Mutes everything that goes to the cable, for the global hotkey.
    pub fn set_muted(&self, muted: bool) {
        self.shared.muted.store(muted, Ordering::Relaxed);
    }

    pub fn is_muted(&self) -> bool {
        self.shared.muted.load(Ordering::Relaxed)
    }

    pub fn start_ab(&self, seconds: f32) -> Result<()> {
        let seconds = seconds.clamp(1.0, 60.0);
        let target_out = (self.out_rate as f32 * seconds) as usize;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if let Ok(mut slot) = self.shared.ab.lock() {
            *slot = Some(AbBuffer {
                // reserve the whole take up front: growing the vector inside
                // the capture callback would allocate on the audio thread
                raw: Vec::with_capacity((self.in_rate as f32 * seconds) as usize + 8192),
                clean: Vec::with_capacity(target_out + 8192),
                raw_rate: self.in_rate,
                out_rate: self.out_rate,
                target_out,
                stamp,
            });
        }
        // keep the settings that produced this take next to it: without them a
        // recording cannot be replayed, and "why does this one sound worse" has
        // no answer an hour later
        if let Ok(current) = self.shared.settings.lock() {
            if let Ok(text) = serde_json::to_string_pretty(&*current) {
                let _ = std::fs::write(recordings::snapshot(stamp), text);
            }
        }
        if let Ok(mut folder) = self.shared.ab_folder.lock() {
            *folder = None;
        }
        if let Ok(mut slot) = self.shared.ab_paths.lock() {
            *slot = (None, None);
        }
        self.shared.ab_done.store(false, Ordering::Relaxed);
        self.shared.ab_active.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub fn ab_status(&self) -> AbStatus {
        let active = self.shared.ab_active.load(Ordering::Relaxed);
        let folder = self.shared.ab_folder.lock().ok().and_then(|f| f.clone());
        let progress = self
            .shared
            .ab
            .lock()
            .ok()
            .and_then(|slot| {
                slot.as_ref().map(|buf| {
                    (buf.clean.len() as f32 / buf.target_out.max(1) as f32).clamp(0.0, 1.0)
                })
            })
            // the buffer is gone while the files are being written: keep the
            // bar full instead of dropping it to zero for a frame
            .unwrap_or(if active || folder.is_some() { 1.0 } else { 0.0 });

        let (raw, clean) = self
            .shared
            .ab_paths
            .lock()
            .map(|p| (p.0.clone(), p.1.clone()))
            .unwrap_or((None, None));

        AbStatus {
            running: self.shared.ab_active.load(Ordering::Relaxed),
            progress,
            folder,
            raw,
            clean,
        }
    }

    pub fn stop(&mut self) {
        // `Drop` calls this as well: log and join only once
        if !self.shared.running.swap(false, Ordering::Relaxed) {
            return;
        }
        crate::log::info(&format!(
            "engine stop: slew {:.0} ppm, model resets {}, underruns {}, overflows {}",
            f32::from_bits(self.shared.slew_ppm.load(Ordering::Relaxed)),
            self.shared.guard_resets.load(Ordering::Relaxed),
            self.shared.underruns.load(Ordering::Relaxed),
            self.shared.overflows.load(Ordering::Relaxed),
        ));
        self.input.take();
        self.output.take();
        self.stop_clean_recording();
        if let Some(mut speaker) = self.speaker.take() {
            speaker.stop();
        }
        if let Some(handle) = self.processor.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::helpers::{sanitize, Slew};
    use super::streams::push_block;
    use super::*;

    #[test]
    fn pushes_count_what_landed_and_flag_an_overflow() {
        let shared = Shared::new(Settings::default());
        let (mut producer, mut consumer) = rtrb::RingBuffer::<f32>::new(4);

        push_block(&mut producer, &[1.0, 2.0, 3.0, 4.0], &shared);
        assert_eq!(shared.overflows.load(Ordering::Relaxed), 0);
        assert_eq!(shared.pushed.load(Ordering::Relaxed), 4);

        // a full ring takes nothing: it must be an overflow, and `pushed` must
        // not claim the samples that were dropped
        push_block(&mut producer, &[5.0, 6.0, 7.0], &shared);
        assert_eq!(shared.overflows.load(Ordering::Relaxed), 1);
        assert_eq!(shared.pushed.load(Ordering::Relaxed), 4);
        assert_eq!(consumer.pop(), Ok(1.0));
    }

    #[test]
    fn block_is_rounded_to_the_models_hop() {
        // 60 ms is what the slider says; the model works in 128-sample hops at
        // 16 kHz, so it runs as 64 ms and the status line has to say 64
        assert_eq!(block_samples(60.0), 1024);
        assert_eq!(block_ms(block_samples(60.0)), 64.0);
        assert_eq!(block_samples(20.0), 384);
        assert_eq!(block_ms(block_samples(20.0)), 24.0);
        // never below one hop, whatever the settings file says
        assert_eq!(block_samples(1.0), 128);
    }

    fn tone(rms: f32, len: usize, phase: &mut f32) -> Vec<f32> {
        let amplitude = rms * std::f32::consts::SQRT_2;
        (0..len)
            .map(|_| {
                *phase += 0.08;
                amplitude * phase.sin()
            })
            .collect()
    }

    #[test]
    fn leveler_brings_a_quiet_microphone_to_the_target() {
        let mut leveler = Leveler::new(0.0);
        let mut phase = 0.0f32;
        let mut last = Vec::new();

        // -40 dBFS input, 60 ms blocks, one second of signal
        for _ in 0..60 {
            let mut block = tone(0.01, 960, &mut phase);
            leveler.process(&mut block, true, true, true);
            last = block;
        }

        let out_db = 20.0 * rms_of(&last).log10();
        assert!(
            out_db > -27.0 && out_db < -21.0,
            "leveler should reach about -26 dBFS, got {out_db:.1}"
        );
    }

    #[test]
    fn leveler_can_be_switched_off() {
        let mut leveler = Leveler::new(0.0);
        let mut phase = 0.0f32;
        let mut block = tone(0.01, 960, &mut phase);
        leveler.process(&mut block, false, true, true);
        let out_db = 20.0 * rms_of(&block).log10();
        assert!(
            (out_db - -40.0).abs() < 1.0,
            "expected untouched level, got {out_db:.1}"
        );
    }

    #[test]
    fn silence_block_detection_matches_quiet_room() {
        assert!(rms_of(&vec![0.0; 960]) < 1e-4);
        let mut phase = 0.0f32;
        assert!(rms_of(&tone(0.02, 960, &mut phase)) > 1e-4);
    }

    #[test]
    fn leveler_ignores_a_television_below_the_speaker() {
        let mut leveler = Leveler::new(10.0);
        let mut phase = 0.0f32;

        // the speaker, about -36 dBFS rms: this sets the peak
        for _ in 0..40 {
            let mut block = tone(0.022, 960, &mut phase);
            leveler.process(&mut block, true, true, true);
        }
        let settled = leveler.gain_db;

        // the television, 17 dB quieter, for five seconds
        for _ in 0..80 {
            let mut block = tone(0.0031, 960, &mut phase);
            leveler.process(&mut block, true, true, true);
        }
        let moved = leveler.gain_db - settled;
        assert!(
            moved < 1.0,
            "the television moved the leveler by {moved:.1} dB"
        );
    }

    #[test]
    fn leveler_ignores_knocks() {
        let mut leveler = Leveler::new(0.0);
        let mut phase = 0.0f32;

        // settle on quiet speech first
        for _ in 0..30 {
            let mut block = tone(0.01, 960, &mut phase);
            leveler.process(&mut block, true, true, true);
        }
        let before = leveler.gain_db;

        // a knock: one loud sample in an otherwise silent block
        let mut knock = vec![0.0f32; 960];
        knock[0] = 0.9;
        leveler.process(&mut knock, true, true, true);

        let moved = (leveler.gain_db - before).abs();
        assert!(moved < 0.1, "a knock moved the level by {moved:.2} dB");
    }

    #[test]
    fn fixed_stage_keeps_the_block_length() {
        let mut stage = FixedStage::new();
        let mut out = vec![0.0f32; 4];

        // a model that returns nothing yet: silence, not raw audio
        stage.take(&[], &mut out, None);
        assert_eq!(out, vec![0.0; 4]);
        assert_eq!(stage.queued(), 0);

        // a model that returns more than one block: the surplus waits
        stage.take(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &mut out, None);
        assert_eq!(out, vec![1.0, 2.0, 3.0, 4.0]);
        assert_eq!(stage.queued(), 2);

        // the next block starts with the leftover, then the fresh samples
        stage.take(&[7.0, 8.0], &mut out, None);
        assert_eq!(out, vec![5.0, 6.0, 7.0, 8.0]);
        assert_eq!(stage.queued(), 0);

        // a short block is covered by the fallback stage, not by silence
        let fallback = [9.0f32, 9.0, 9.0, 9.0];
        stage.take(&[4.0, 4.0], &mut out, Some(&fallback));
        assert_eq!(out, vec![4.0, 4.0, 9.0, 9.0]);
        assert_eq!(stage.queued(), 0);

        // a reset drops whatever was waiting
        stage.take(&[1.0, 2.0, 3.0, 4.0, 5.0], &mut out, None);
        stage.reset();
        assert_eq!(stage.queued(), 0);
    }

    #[test]
    fn sanitize_replaces_non_finite_samples() {
        let mut block = [0.1f32, f32::NAN, f32::INFINITY, -0.2];
        assert!(sanitize(&mut block));
        assert_eq!(block, [0.1, 0.0, 0.0, -0.2]);
        // a clean block is left alone
        assert!(!sanitize(&mut block));
        assert_eq!(block, [0.1, 0.0, 0.0, -0.2]);
    }

    #[test]
    fn slew_step_changes_the_sample_rate() {
        let mut slow = Slew::new();
        let mut fast = Slew::new();
        let input = vec![0.1f32; 2048];
        let mut a = Vec::new();
        let mut b = Vec::new();
        slow.step = 1.0;
        fast.step = 1.005;
        for _ in 0..20 {
            slow.process(&input, &mut a);
            fast.process(&input, &mut b);
        }
        // a larger step reads further per sample, so it must produce fewer
        assert!(b.len() < a.len(), "{} vs {}", b.len(), a.len());
        let ratio = a.len() as f64 / b.len() as f64;
        assert!((ratio - 1.005).abs() < 0.002, "ratio {ratio:.4}");
    }

    #[test]
    fn peak_guard_limits_knocks_but_not_speech() {
        let mut guard = PeakGuard::new(16_000.0);

        let mut speech = vec![0.1f32; 960];
        guard.process(&mut speech);
        assert!(
            (speech[959] - 0.1).abs() < 1e-3,
            "speech was changed to {}",
            speech[959]
        );

        let mut knock = vec![0.02f32; 960];
        knock[10] = 0.95;
        guard.process(&mut knock);
        assert!(knock[10] <= 0.52, "knock left at {}", knock[10]);
    }

    #[test]
    fn cap_puts_a_floor_under_the_output() {
        let input = 0.03f32; // -30 dBFS
        let model = 1e-5f32; // -100 dBFS, the model zeroed the voice
        let mix = cap_dry_mix(input, model, 24.0);
        let out = model + (input - model) * mix;
        let floor = input * 10f32.powf(-24.0 / 20.0);
        assert!((out - floor).abs() < 1e-6, "out {out:.5} floor {floor:.5}");
        // no limit, or a cut shallower than the limit: no mixing
        assert_eq!(cap_dry_mix(input, model, 0.0), 0.0);
        assert_eq!(cap_dry_mix(input, input * 0.5, 24.0), 0.0);
    }

    #[test]
    fn dropout_guard_cold_onset_fires_when_the_model_was_closed() {
        let mut guard = DropoutGuard::new();
        // the model has been suppressing the room for half a second
        for _ in 0..8 {
            assert!(!guard.observe(-55.0, -90.0, false, 26.0));
        }
        assert!(
            guard.cold_onset(true),
            "the first word must restart the model"
        );
        assert!(guard.cold());
        // a cold onset while the model was open is not a cold start
        let mut open = DropoutGuard::new();
        for _ in 0..8 {
            open.observe(-30.0, -28.0, true, 26.0);
        }
        assert!(!open.cold_onset(true));
    }

    #[test]
    fn a_background_voice_does_not_restart_the_model() {
        // A distant voice is loud enough to pass -54 dB and the model suppresses
        // it by 40 dB, but the owner is silent: that is not a dropout.
        let mut guard = DropoutGuard::new();
        let mut resets = 0;
        for _ in 0..200 {
            if guard.observe(-45.0, -85.0, false, 26.0) {
                resets += 1;
            }
        }
        assert_eq!(resets, 0, "the guard restarted the model {resets} times");
    }

    #[test]
    fn dropout_guard_retries_but_is_bounded() {
        let mut guard = DropoutGuard::new();
        for _ in 0..8 {
            guard.observe(-55.0, -90.0, false, 26.0);
        }
        guard.cold_onset(true);
        let mut resets = 0;
        for _ in 0..200 {
            if guard.observe(-30.0, -80.0, true, 26.0) {
                resets += 1;
            }
        }
        assert!(resets >= 2, "the guard gave up after {resets} reset(s)");
        assert!(
            resets <= 4,
            "the guard reset {resets} times: it should be bounded"
        );
    }

    #[test]
    fn input_voice_holds_through_word_gaps() {
        let mut voice = InputVoice::new();
        let quiet = vec![0.0f32; 1024];
        for _ in 0..80 {
            assert!(!voice.update(&quiet));
        }
        let speech = vec![0.02f32; 1024];
        // the first block is a jump, the second confirms it
        assert!(!voice.update(&speech));
        assert!(voice.update(&speech));
        assert!(voice.voiced());
        assert!(voice.speech());
        // one quiet block between words keeps the flag up
        assert!(voice.update(&quiet));
        assert!(!voice.voiced());
        assert!(voice.speech());
    }

    #[test]
    fn input_voice_keeps_a_long_phrase_alive() {
        // after a few seconds the average catches up with the voice. A phrase
        // must not end there: the cap and the level control would switch off in
        // the middle of a sentence, which is heard as the level galloping.
        let mut voice = InputVoice::new();
        let quiet = vec![0.004f32; 960]; // about -48 dBFS room
        for _ in 0..40 {
            voice.update(&quiet);
        }
        let speech = vec![0.02f32; 960]; // about -34 dBFS voice
        voice.update(&speech);
        assert!(voice.update(&speech));
        for _ in 0..300 {
            assert!(voice.update(&speech), "the phrase ended in the middle");
        }
        for _ in 0..20 {
            voice.update(&quiet);
        }
        assert!(!voice.speech(), "the phrase never ended after the pause");
    }

    /// The live processing thread is the one piece selftest and replay do not
    /// cover: both drive Pipeline/Chain directly. This pushes a block through
    /// the real loop (pacing, silence detection, leveler, publish) with no model
    /// loaded, so a broken ring hand-off or a controller that never lets a block
    /// through fails here instead of in the user's ears.
    #[test]
    fn processor_moves_a_block_to_the_output_ring() {
        use std::path::Path;

        let settings = Settings {
            voice_isolation: false,
            residual_denoise: "off".into(),
            vad_gate: false,
            air_amount: 0.0,
            input_leveling: false,
            agc_enabled: false,
            gate_enabled: false,
            limiter_enabled: false,
            comfort_noise_enabled: false,
            tone_enabled: false,
            ..Settings::default()
        };

        let chunk = block_samples(settings.chunk_ms);
        let shared = Shared::new(settings.clone());
        let dir = std::env::temp_dir().join(format!("nc-processor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let persist = Persist::spawn(dir.clone(), LearnedState::default());
        let pipeline = Pipeline::new(
            &settings,
            None,
            2,
            chunk,
            Path::new("unused"),
            Path::new("unused"),
        )
        .expect("pipeline without models");
        let processor = Processor::new(
            shared.clone(),
            &settings,
            pipeline,
            persist,
            48_000,
            50.0,
            50.0,
            0.0,
            None,
        );

        let (mut input, input_cons) = rtrb::RingBuffer::<f32>::new(chunk * 16);
        let (output_prod, mut output_cons) = rtrb::RingBuffer::<f32>::new(48_000);
        let handle = thread::spawn(move || processor.run(input_cons, output_prod));

        let mut phase = 0.0f32;
        for _ in 0..2 {
            for _ in 0..chunk {
                let _ = input.push(phase.sin() * 0.1);
                phase += 2.0 * std::f32::consts::PI * 220.0 / MODEL_RATE as f32;
            }
            thread::sleep(Duration::from_millis(60));
        }
        thread::sleep(Duration::from_millis(100));
        shared
            .running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        handle.join().expect("processor thread");

        let mut pulled = 0usize;
        let mut peak = 0.0f32;
        while let Ok(sample) = output_cons.pop() {
            pulled += 1;
            peak = peak.max(sample.abs());
        }
        let _ = std::fs::remove_dir_all(&dir);

        assert!(pulled > chunk, "processor pushed only {pulled} samples");
        assert!(peak > 1e-3, "output stayed silent (peak {peak})");
    }

    #[test]
    fn persist_merges_both_values_into_one_file() {
        let dir = std::env::temp_dir().join(format!("nc-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let persist = Persist::spawn(dir.clone(), LearnedState::default());
        persist.set_auto_buffer(170.0);
        persist.set_leveler_gain(4.5);

        // messages are handled in order on the writer thread; a state file that
        // lost one of them would be a merge bug, not a timing race
        let path = LearnedState::path(&dir);
        for _ in 0..300 {
            let state = std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| serde_json::from_str::<LearnedState>(&text).ok());
            if let Some(state) = state {
                if state.auto_buffer_ms == 170.0 && state.leveler_gain_db == 4.5 {
                    let _ = std::fs::remove_dir_all(&dir);
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = std::fs::remove_dir_all(&dir);
        panic!("nc-persist did not write both values into state.json");
    }
}
