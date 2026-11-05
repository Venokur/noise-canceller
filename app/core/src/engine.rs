//! WASAPI duplex engine: capture → 16 kHz → VoxLock → DSP → render.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

use crate::dsp::Chain;
use crate::model::{Denoiser, Vad};
use crate::paths;
use crate::resample::Resampler;
use crate::settings::Settings;

pub const MODEL_RATE: u32 = 16_000;
pub const BANDS: usize = 32;
/// Blocks of near-digital silence after which the model is skipped.
///
/// Shared with `nc replay`: a different value there (it used to be 100) meant
/// the offline regression ran a different audio path than the live app.
pub const SILENCE_SKIP_BLOCKS: u32 = 5;

#[derive(Default)]
pub struct Stats {
    pub input_rms: f32,
    pub output_rms: f32,
    pub rtf: f32,
    pub latency_ms: f32,
    pub underruns: u32,
    pub backend: String,
    pub spectrum: [f32; BANDS],
    /// how much the model is cutting right now, positive dB
    pub suppression_db: f32,
    /// true while the input looks like speech (VAD proxy for the UI)
    pub speech: bool,
    /// peak of the last input callback, dBFS (clip indicator)
    pub input_peak_db: f32,
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
    /// false when the processor thread died (panic) while the streams live on
    pub healthy: bool,
}

pub struct AbBuffer {
    raw: Vec<f32>,
    clean: Vec<f32>,
    raw_rate: u32,
    out_rate: u32,
    target_out: usize,
    stamp: u64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AbStatus {
    pub running: bool,
    pub progress: f32,
    pub folder: Option<String>,
    pub raw: Option<String>,
    pub clean: Option<String>,
}

#[derive(Clone)]
struct Shared {
    running: Arc<AtomicBool>,
    input_rms: Arc<AtomicU32>,
    output_rms: Arc<AtomicU32>,
    rtf: Arc<AtomicU32>,
    underruns: Arc<AtomicU32>,
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
    buffer_ms: Arc<AtomicU32>,
    leveling: Arc<AtomicBool>,
    out_fill: Arc<AtomicU32>,
    out_raw: Arc<AtomicU32>,
    suppression: Arc<AtomicU32>,
    speech: Arc<AtomicBool>,
    input_peak: Arc<AtomicU32>,
    muted: Arc<AtomicBool>,
    guard_resets: Arc<AtomicU32>,
    leveler_gain: Arc<AtomicU32>,
    slew_ppm: Arc<AtomicU32>,
    in_frames: Arc<AtomicU64>,
    pushed: Arc<AtomicU64>,
    pulled: Arc<AtomicU64>,
    /// capture samples dropped because the input ring was full
    capture_drops: Arc<AtomicU32>,
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
            buffer_ms: Arc::new(AtomicU32::new(50.0f32.to_bits())),
            leveling: Arc::new(AtomicBool::new(settings.input_leveling)),
            out_fill: Arc::new(AtomicU32::new(0)),
            out_raw: Arc::new(AtomicU32::new(0)),
            suppression: Arc::new(AtomicU32::new(0)),
            speech: Arc::new(AtomicBool::new(false)),
            input_peak: Arc::new(AtomicU32::new(0)),
            muted: Arc::new(AtomicBool::new(false)),
            guard_resets: Arc::new(AtomicU32::new(0)),
            leveler_gain: Arc::new(AtomicU32::new(0)),
            slew_ppm: Arc::new(AtomicU32::new(0)),
            in_frames: Arc::new(AtomicU64::new(0)),
            pushed: Arc::new(AtomicU64::new(0)),
            pulled: Arc::new(AtomicU64::new(0)),
            capture_drops: Arc::new(AtomicU32::new(0)),
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
struct Slew {
    pos: f64,
    step: f64,
    tail: [f32; 4],
    buf: Vec<f32>,
}

impl Slew {
    fn new() -> Self {
        Self {
            pos: 2.0,
            step: 1.0,
            tail: [0.0; 4],
            buf: Vec::new(),
        }
    }

    fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.buf.clear();
        self.buf.extend_from_slice(&self.tail);
        self.buf.extend_from_slice(input);

        let len = self.buf.len();
        let mut pos = self.pos;

        while (pos as usize) + 2 < len {
            let i = pos as usize;
            let t = (pos - i as f64) as f32;
            let y0 = self.buf[i - 1];
            let y1 = self.buf[i];
            let y2 = self.buf[i + 1];
            let y3 = self.buf[i + 2];

            let c0 = y1;
            let c1 = 0.5 * (y2 - y0);
            let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
            let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
            out.push(((c3 * t + c2) * t + c1) * t + c0);

            pos += self.step;
        }

        let keep = len.saturating_sub(4);
        self.tail.copy_from_slice(&self.buf[keep..keep + 4]);
        self.pos = pos - keep as f64;
    }
}

/// Keeps a denoiser stage time-invariant.
///
/// Public because `nc replay` mirrors the live engine and must not drop or
/// pad samples differently.
#[derive(Default)]
pub struct FixedStage {
    pending: Vec<f32>,
}

impl FixedStage {
    pub fn new() -> Self {
        Self {
            pending: Vec::with_capacity(4096),
        }
    }

    pub fn reset(&mut self) {
        self.pending.clear();
    }

    /// Accepts whatever the model returned for one block and writes exactly
    /// `out.len()` samples. Extra samples wait for the next block, a shortfall
    /// is covered by `fallback` (a safe earlier stage) or silence. Dropping the
    /// surplus instead — the old behaviour — quietly speeds speech up, and
    /// padding with the raw microphone mixes noise back in.
    pub fn take(&mut self, produced: &[f32], out: &mut [f32], fallback: Option<&[f32]>) {
        self.pending.extend_from_slice(produced);
        let n = out.len();
        let have = self.pending.len().min(n);
        out[..have].copy_from_slice(&self.pending[..have]);
        if have < n {
            match fallback {
                Some(src) => {
                    for (i, sample) in out.iter_mut().enumerate().skip(have) {
                        *sample = src.get(i).copied().unwrap_or(0.0);
                    }
                }
                None => out[have..].fill(0.0),
            }
        }
        self.pending.drain(..have);
        // a model that keeps returning extra must not grow without bound
        let cap = n * 8;
        if self.pending.len() > cap {
            let drop = self.pending.len() - cap;
            self.pending.drain(..drop);
        }
    }

    #[cfg(test)]
    fn queued(&self) -> usize {
        self.pending.len()
    }
}

/// Where the *microphone* is in the conversation, independent of VoxLock.
///
/// The output-side VAD follows the model, so while the model is cold it reports
/// silence exactly when the speaker is talking. The cold-start reset and the
/// suppression cap need a verdict on the input instead. This is a level jump
/// over the room's long-term average: cheap, and it does not mistake a steady
/// fan for speech the way Silero can in a very quiet room.
pub struct InputVoice {
    /// this block is speech (with a short hold through word gaps)
    speech: bool,
    /// this block itself jumped above the room (one block, no hold)
    voiced: bool,
    run: u32,
    avg_db: Option<f32>,
    last_db: f32,
    hold: u32,
}

impl InputVoice {
    /// speech sits at least this far above the room's average
    const JUMP_DB: f32 = 8.0;
    /// two blocks is enough: a click or a knock is not a phrase
    const CONFIRM_BLOCKS: u32 = 2;
    /// ~0.5 s of hold, so a word gap does not switch the cap off
    const HOLD_BLOCKS: u32 = 8;

    pub fn new() -> Self {
        Self {
            speech: false,
            voiced: false,
            run: 0,
            avg_db: None,
            last_db: -120.0,
            hold: 0,
        }
    }

    /// Feeds one block of the raw microphone and returns true while it looks
    /// like the speaker.
    pub fn update(&mut self, block: &[f32]) -> bool {
        let db = 20.0 * (rms_of(block).max(1e-9)).log10();
        self.last_db = db;
        // a ~3 s average follows the room. A minimum tracker was tried first
        // and failed: fan noise dips and peaks by 10-15 dB, so every peak
        // looked like a word.
        let avg = match self.avg_db {
            None => db,
            Some(a) => a + (db - a) * 0.02,
        };
        self.avg_db = Some(avg);

        self.voiced = (db - avg > Self::JUMP_DB && db > -45.0)
            || (db - avg > 15.0 && db > -55.0);
        if self.voiced {
            self.run = self.run.saturating_add(1);
        } else {
            self.run = 0;
        }
        let confirmed = self.run >= Self::CONFIRM_BLOCKS;
        if confirmed {
            self.hold = Self::HOLD_BLOCKS;
        } else {
            self.hold = self.hold.saturating_sub(1);
        }
        self.speech = confirmed || self.hold > 0;
        self.speech
    }

    pub fn speech(&self) -> bool {
        self.speech
    }

    /// True only on the block that jumped above the room: the first word after
    /// a pause. The cold-start reset uses this, not the held `speech`.
    pub fn voiced(&self) -> bool {
        self.voiced
    }
    /// Current block level and tracked room level, dBFS, for diagnostics.
    pub fn levels_db(&self) -> (f32, f32) {
        (self.last_db, self.avg_db.unwrap_or(-120.0))
    }
}

impl Default for InputVoice {
    fn default() -> Self {
        Self::new()
    }
}

/// Tracks what VoxLock has been doing and decides when it must be restarted.
///
/// The old inline guard only armed after eight "open" blocks. After a pause the
/// model has been suppressing the room, so there are zero open blocks and the
/// guard never fired exactly when it was needed; after one reset it also
/// disarmed itself for good. Both left the first words muted for seconds.
pub struct DropoutGuard {
    cut: u32,
    cool: u32,
    open: u32,
    armed: bool,
    /// consecutive blocks where the model suppressed its input
    closed: u32,
    cold_hold: u32,
    retries: u32,
    pub resets: u32,
}

impl DropoutGuard {
    pub fn new() -> Self {
        Self {
            cut: 0,
            cool: 0,
            open: 0,
            armed: false,
            closed: 0,
            cold_hold: 0,
            retries: 0,
            resets: 0,
        }
    }

    /// Called *before* the model runs. Returns true on the first speech block
    /// after the model has been suppressing everything for a while: its
    /// speaker state is stale, so it must be started over before it sees the
    /// word — otherwise the first 0.5-3 s are cut while it finds the speaker.
    pub fn cold_onset(&mut self, voiced: bool) -> bool {
        if self.cold_hold > 0 {
            // one proactive reset per cold window; the guard handles retries
            return false;
        }
        let onset = voiced && self.closed >= 8;
        if onset {
            self.closed = 0;
            self.cold_hold = 80; // ~5 s of blocks
            self.retries = 0;
            self.cut = 0;
            self.armed = true;
        }
        onset
    }

    /// Called *after* the model runs. `out_db` is -120 when the model returned
    /// nothing. `cut_db` is the attenuation that counts as a dropout (raised
    /// when a suppression cap is set, otherwise the cap would hide every cut).
    pub fn observe(&mut self, in_db: f32, out_db: f32, in_speech: bool, cut_db: f32) -> bool {
        // update the "model has been closed" run for the next block
        if !out_db.is_finite() || out_db < in_db - 20.0 {
            self.closed = self.closed.saturating_add(1);
        } else {
            self.closed = 0;
        }
        self.cold_hold = self.cold_hold.saturating_sub(1);
        let cold = self.cold_hold > 0;

        // `!is_finite` also catches NaN: the old comparison was false for NaN
        // and the guard stayed blind for the whole episode
        let dead = !out_db.is_finite() || out_db < in_db - cut_db;
        let cut = in_db > -54.0 && dead;
        if cut {
            if self.cut == 0 {
                self.armed = self.open >= 8 || cold;
            }
            self.cut += 1;
        } else {
            self.cut = 0;
        }

        if out_db > in_db - 20.0 {
            self.open += 1;
        } else {
            self.open = 0;
        }
        self.cool = self.cool.saturating_sub(1);

        if !in_speech && self.cut == 0 {
            self.retries = 0;
        }

        if self.cut >= 3 && self.cool == 0 && (self.armed || cold) && self.retries < 4 {
            self.resets += 1;
            self.retries += 1;
            self.cut = 0;
            self.cool = 15;
            self.open = 0;
            // retry only while the input still looks like the speaker
            self.armed = cold;
            return true;
        }
        false
    }

    pub fn cold(&self) -> bool {
        self.cold_hold > 0
    }

    /// The engine skipped the model because the room was digitally silent.
    /// Its state is stale just like after a long stretch of background.
    pub fn skipped(&mut self) {
        self.closed = self.closed.saturating_add(1);
    }
}

impl Default for DropoutGuard {
    fn default() -> Self {
        Self::new()
    }
}

/// Fraction of the model input to mix back in so the model cannot cut more
/// than `max_db` below the input. 0 = no mixing needed; callers apply it as
/// `wet = wet.min(1 - mix)` (the mix mixes the pre-model signal, not the raw
/// microphone: it has already been leveled and peak-limited).
pub fn cap_dry_mix(in_rms: f32, out_rms: f32, max_db: f32) -> f32 {
    if max_db <= 0.0 || in_rms <= 1e-6 {
        return 0.0;
    }
    let floor = in_rms * 10f32.powf(-max_db / 20.0);
    if out_rms >= floor {
        return 0.0;
    }
    ((floor - out_rms) / (in_rms - out_rms).max(1e-6)).clamp(0.0, 1.0)
}

/// Brings the microphone up to a level the model and the gate expect.
/// A quiet headset mic can sit 30 dB below a normal one, and then every
/// absolute threshold in the chain is wrong. The gain follows speech, not
/// knocks: transients have a big crest factor and are ignored.
pub struct Leveler {
    gain_db: f32,
    applied: f32,
    /// tracked level of the room, so silence is not mistaken for quiet speech
    floor_db: f32,
    /// loudest speech of the last seconds, slowly decaying
    loud: f32,
    voice_blocks: u32,
}

impl Leveler {
    pub fn new(start_db: f32) -> Self {
        let db = start_db.clamp(0.0, 18.0);
        // the remembered gain says what speech is expected to look like: a
        // television in the room is far below that peak, so it cannot pull the
        // gain up before the user says a word
        let loud = 0.0;
        Self {
            gain_db: db,
            applied: 10f32.powf(db / 20.0),
            floor_db: -60.0,
            loud,
            voice_blocks: 0,
        }
    }

    pub fn gain_db(&self) -> f32 {
        self.gain_db
    }

    pub fn process(&mut self, block: &mut [f32], enabled: bool, allow_up: bool) {
        if block.is_empty() {
            return;
        }

        let mut sum = 0.0f64;
        let mut peak = 0.0f32;
        for sample in block.iter() {
            let value = *sample as f64;
            sum += value * value;
            peak = peak.max(sample.abs());
        }

        let rms = (sum / block.len() as f64).sqrt() as f32;
        let rms_db = 20.0 * (rms + 1e-9).log10();
        let crest_db = 20.0 * ((peak + 1e-9) / (rms + 1e-9)).log10();

        // a knock on the desk is loud for a few milliseconds and then gone; it
        // must not pull the level down and make the following words quiet
        let transient = crest_db > 12.0;

        // track the room: the floor drops on quiet blocks and creeps up on
        // loud ones, so a steady quiet block is noise, not a quiet word. It
        // used to be that pauses counted as speech and pushed the gain to
        // maximum, after which every word came out far louder than the one
        // before — the jump the ear hears as the level "galloping"
        if rms_db < self.floor_db {
            self.floor_db = rms_db;
        } else {
            self.floor_db += 0.003;
        }
        let above_floor = rms_db - self.floor_db;

        if enabled && !transient && above_floor > 8.0 && rms_db > -58.0 {
            self.voice_blocks = self.voice_blocks.saturating_add(1);

            // the level is the loudest speech of the last seconds, not the
            // average: in a room with a television the average sits closer to
            // the TV than to the speaker, the gain creeps up to +18 dB, and the
            // first word after a pause comes out far louder than the rest
            self.loud = if self.loud <= 0.0 {
                rms
            } else {
                (self.loud * 0.995).max(rms)
            };
            let loud_db = 20.0 * (self.loud + 1e-9).log10();

            // a block 8 dB below the recent peak is background, not the
            // speaker: it must not move the gain
            if rms_db > loud_db - 8.0 {
                let speech_db = loud_db - 4.0; // word rms vs its loudest block
                let wanted = (-26.0 - speech_db).clamp(0.0, 18.0);

                // a remembered gain learned on a television can be 8-10 dB
                // too high: do not carry it into the first words of a session
                // at all, just drop to what the speaker needs right away
                if self.voice_blocks < 4 && wanted < self.gain_db - 3.0 {
                    self.gain_db = wanted;
                } else {
                    // normal move is slow; start-up and a stale remembered gain
                    // that is far too high are corrected in a couple of blocks
                    let catch_up = self.voice_blocks < 12 || wanted < self.gain_db - 6.0;
                    let (coef, limit) = if catch_up {
                        (0.6, 6.0)
                    } else if wanted < self.gain_db {
                        (0.3, 2.5)
                    } else {
                        (0.12, 1.5)
                    };
                    // for the first 1.5 s of a session only lower the gain: a
                    // television in the room must not push it up before the
                    // user says a word
                    let rising = wanted > self.gain_db;
                    if !rising || allow_up {
                        let delta = ((wanted - self.gain_db) * coef).clamp(-limit, limit);
                        self.gain_db += delta;
                    }
                }
            }
        }

        let target = if enabled { 10f32.powf(self.gain_db / 20.0) } else { 1.0 };
        let step = (target - self.applied) / block.len() as f32;
        let mut gain = self.applied;
        for sample in block.iter_mut() {
            gain += step;
            *sample *= gain;
        }
        self.applied = target;
    }
}

/// Peak guard in front of the model. A knock is far louder than speech; without
/// this the model sees a clipped burst, and everything it produces for the next
/// moment is wrong. Instant attack, 80 ms release, ceiling at −6 dBFS.
pub struct PeakGuard {
    gain: f32,
    release: f32,
}

impl PeakGuard {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            gain: 1.0,
            release: 1.0 - (-1000.0 / 80.0 / sample_rate.max(1.0)).exp(),
        }
    }

    pub fn process(&mut self, block: &mut [f32]) {
        let ceiling = 0.5f32;
        let mut gain = self.gain;

        for sample in block.iter_mut() {
            let abs = sample.abs();
            if abs * gain > ceiling {
                gain = ceiling / (abs + 1e-9);
            } else {
                gain += (1.0 - gain) * self.release;
            }
            *sample *= gain;
        }

        self.gain = gain;
    }
}

/// Names of the Windows default input/output, for "follow the system device".
pub fn default_device_names() -> (Option<String>, Option<String>) {
    let host = cpal::default_host();
    let input = host
        .default_input_device()
        .and_then(|d| d.description().ok().map(|desc| desc.name().to_string()))
        // the cable's capture side is not a microphone: following it means
        // processing our own output (feedback) and every app playing into it
        .filter(|name| !is_cable_name(name));
    let output = host
        .default_output_device()
        .and_then(|d| d.description().ok().map(|desc| desc.name().to_string()));
    (input, output)
}

/// True for the virtual cable in either direction.
pub fn is_cable_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("cable") || lower.contains("vb-audio") || lower.contains("vb-cable")
}

pub fn input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|it| it.filter_map(|d| d.description().ok().map(|desc| desc.name().to_string())).collect())
        .unwrap_or_default()
}

pub fn output_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.output_devices()
        .map(|it| it.filter_map(|d| d.description().ok().map(|desc| desc.name().to_string())).collect())
        .unwrap_or_default()
}

pub struct Engine {
    shared: Shared,
    input: Option<cpal::Stream>,
    output: Option<cpal::Stream>,
    processor: Option<JoinHandle<()>>,
    backend: String,
    in_rate: u32,
    out_rate: u32,
    threads: u16,
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

        let host = cpal::default_host();
        let input = pick_device(&host, input_name.as_deref(), true)?;
        // the cable's capture side is our own output: processing it is a
        // feedback loop (and every app playing into the cable is "the speaker")
        if let Ok(desc) = input.description() {
            let name = desc.name().to_string();
            if is_cable_name(&name) {
                return Err(anyhow!(
                    "«{name}» — это виртуальный кабель, а не микрофон: приложение услышит собственный выход. Выберите настоящий микрофон."
                ));
            }
        }

        // a normal run must never speak into the speakers: that is what the
        // explicit "test in speakers" mode is for
        let output = if test_mode {
            pick_device(&host, output_name.as_deref(), false)?
        } else {
            match find_cable(&host) {
                Some(device) => device,
                None => {
                    return Err(anyhow!(
                        "виртуальный микрофон не найден — установите VB-CABLE или включите проверку в колонках"
                    ))
                }
            }
        };

        let in_cfg = input.default_input_config()?;
        let out_cfg = output.default_output_config()?;

        let in_rate = in_cfg.sample_rate();
        let out_rate = out_cfg.sample_rate();

        let chunk_samples = ((MODEL_RATE as f32 * settings.chunk_ms / 1000.0) as usize).max(128);
        // round to the nearest multiple of 128: 60 ms must stay 60 ms, not 56
        let chunk_samples = ((chunk_samples + 64) / 128).max(1) * 128;
        let chunk_ms = chunk_samples as f32 * 1000.0 / MODEL_RATE as f32;

        // the automatic pick uses the *rounded* block, not the slider value
        let threads = if settings.auto_threads {
            crate::settings::recommended_threads(chunk_ms)
        } else {
            settings.denoiser_threads.clamp(1, 16)
        };

        let denoiser = if settings.voice_isolation {
            Some(Denoiser::new(
                &paths::enroll_model(),
                &paths::chunk_model(),
                voice.as_deref(),
                threads,
            )?)
        } else {
            None
        };
        let backend = if settings.voice_isolation {
            format!("VoxLock · CPU · {} мс блок", settings.chunk_ms as u32)
        } else {
            format!("Шумодав без изоляции · {} мс блок", settings.chunk_ms as u32)
        };

        let shared = Shared::new(settings.clone());
        shared
            .wet
            .store(settings.isolation_strength.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        shared.buffer_ms.store(settings.buffer_ms.to_bits(), Ordering::Relaxed);

        // rings ------------------------------------------------------------
        let (mut in_prod, mut in_cons) = rtrb::RingBuffer::<f32>::new(MODEL_RATE as usize * 2);
        let (mut out_prod, mut out_cons) = rtrb::RingBuffer::<f32>::new(out_rate as usize * 2);

        // prefill the output ring so playback starts cleanly: one block plus the
        // buffer, otherwise the first block arrives too late and starts with a
        // hole right away
        let prefill = (out_rate as f32 * (settings.buffer_ms + chunk_ms + 24.0) / 1000.0) as usize;
        for _ in 0..prefill {
            let _ = out_prod.push(0.0);
        }

        let in_ab = shared.ab.clone();
        let proc_ab = shared.ab.clone();
        let proc_ab_done = shared.ab_done.clone();
        let proc_ab_active = shared.ab_active.clone();
        let proc_ab_folder = shared.ab_folder.clone();
        let proc_ab_paths = shared.ab_paths.clone();

        // processing thread ------------------------------------------------
        let proc_shared = shared.clone();
        let mut chain = Chain::new();
        chain.configure(&settings, out_rate);
        let mut up = Resampler::new(MODEL_RATE, out_rate);
        let mut up_buf = vec![0.0f32; up.max_output(chunk_samples) + 64];
        let mut denoiser = denoiser;
        let mut block = vec![0.0f32; chunk_samples];
        let mut out_scratch: Vec<f32> = Vec::with_capacity(chunk_samples * 4);

        // optional second denoiser: it cleans what the target-speaker model
        // lets through. It can run before VoxLock (cleaner input for the
        // extraction) or after it (cleaner residual), chosen in settings.
        let residual_kind = settings.residual_denoise.to_lowercase();
        let order_before = settings.residual_order.eq_ignore_ascii_case("before");
        let mut gtcrn_before = None;
        let mut gtcrn_after = None;
        if residual_kind == "gtcrn" && paths::gtcrn_model().exists() {
            if let Ok(model) = Denoiser::new_gtcrn(&paths::gtcrn_model(), 2) {
                let stream = crate::model::StreamDenoiser::new(model);
                if order_before {
                    gtcrn_before = Some(stream);
                } else {
                    gtcrn_after = Some(stream);
                }
            }
        }
        let mut dpdfnet = if residual_kind == "dpdfnet" && paths::dpdfnet_model().exists() {
            Denoiser::new_dpdfnet(
                &paths::dpdfnet_model(),
                settings.residual_attenuation_db,
                threads,
            )
            .ok()
            .map(crate::model::StreamDenoiser::new)
        } else {
            None
        };
        let mut res16: Vec<f32> = Vec::with_capacity(chunk_samples * 2);
        let mut res48: Vec<f32> = Vec::with_capacity(chunk_samples * 8);
        let mut vad = if settings.vad_gate && paths::vad_model().exists() {
            Vad::new(&paths::vad_model(), 0.5).ok()
        } else {
            None
        };
        // a second detector on the model input: the output-side one follows
        // VoxLock, so while the model is cold it reports silence exactly when
        // the speaker is talking. The cap and the cold reset need the input.
        let mut input_vad = Vad::new(&paths::vad_model(), 0.5).ok();
        let mut input_voice = InputVoice::new();
        let mut dropout = DropoutGuard::new();
        let mut max_suppression = settings.max_suppression_db.clamp(0.0, 48.0);
        let mut cap_mix = 0.0f32;
        let mut vad_gain = 1.0f32;
        let vad_attack = 1.0 - (-1000.0f32 / 5.0 / 16_000.0).exp();
        let vad_release = 1.0 - (-1000.0f32 / 450.0 / 16_000.0).exp();

        let mut analyzer = crate::spectrum::Spectrum::new(1024, out_rate as f32);
        let mut chain_seq = 0u32;
        let mut rtf_window = (0usize, Duration::ZERO);
        let mut cpu_window_start = Instant::now();
        // remember the level between runs: without it the first words of a
        // session start quiet and climb for a second or two
        let level_path = paths::data_dir().join("leveler.txt");
        let saved_gain = std::fs::read_to_string(&level_path)
            .ok()
            .and_then(|s| s.trim().parse::<f32>().ok())
            .unwrap_or(0.0)
            .clamp(0.0, 18.0);
        let mut leveler = Leveler::new(saved_gain);

        let session_start = Instant::now();
        let mut saved_level = saved_gain;
        let mut persist_ticks = 0u32;
        let mut guard = PeakGuard::new(MODEL_RATE as f32);
        let zeros = vec![0.0f32; chunk_samples];
        let mut silent_run = 0u32;
        let mut was_speaking = false;
        let mut onset_blocks = 0u32;
        let mut speech_hold = 0u32;
        let mut slew = Slew::new();
        let mut input_ring: Vec<f32> = Vec::with_capacity(4096);
        let mut slew_out: Vec<f32> = Vec::with_capacity(chunk_samples * 4);
        let mut slew_step = 1.0f64;
        // rate control: the ring level is an integral of the clock error, so a
        // deadband around it moves the converter only when it actually drifts
        let mut drift_at = Instant::now();
        let mut drift_logs = 0u32;
        let trace = std::env::var("NC_TRACE").is_ok();
        // every model stage hands back exactly `chunk_samples`, whatever the
        // model returned: a varying length shifts speech in time
        let mut isolate_out = vec![0.0f32; chunk_samples];
        let mut model_out: Vec<f32> = Vec::with_capacity(chunk_samples * 2);
        let mut isolate_stage = FixedStage::new();
        let mut gtcrn_out = vec![0.0f32; chunk_samples];
        let mut gtcrn_stage = FixedStage::new();
        let mut gtcrn_before_out = vec![0.0f32; chunk_samples];
        let mut gtcrn_before_stage = FixedStage::new();
        let mut dpdf_out = vec![0.0f32; up_buf.len()];
        let mut dpdf_stage = FixedStage::new();

        let processor = thread::Builder::new()
            .name("voxlock-processor".into())
            .spawn(move || {
                while proc_shared.running.load(Ordering::Relaxed) {
                    let seq = proc_shared.settings_seq.load(Ordering::Relaxed);
                    if seq != chain_seq {
                        if let Ok(current) = proc_shared.settings.lock() {
                            chain.configure(&current, out_rate);
                            max_suppression = current.max_suppression_db.clamp(0.0, 48.0);
                        }
                        chain_seq = seq;
                    }

                    if in_cons.slots() < chunk_samples {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }

                    // pace production: never run ahead of what the output ring
                    // can hold, otherwise the excess is simply thrown away
                    let chunk_out = (out_rate as f32 * chunk_ms / 1000.0) as usize;
                    let buffer_ms = f32::from_bits(proc_shared.buffer_ms.load(Ordering::Relaxed));
                    // a virtual cable can ask for 200 ms in one callback; the ring
                    // has to hold at least one callback plus a block, otherwise
                    // every callback drains it and the render starsves
                    let callback_ms = proc_shared.callback_frames.load(Ordering::Relaxed) as f32
                        / out_rate.max(1) as f32
                        * 1000.0;
                    let block_ms = chunk_samples as f32 / MODEL_RATE as f32 * 1000.0;
                    let ref_ms = buffer_ms + callback_ms.max(block_ms * 0.5);
                    let max_fill = (out_rate as f32 * ref_ms / 1000.0) as usize + chunk_out + 64;
                    // leave room for a whole block plus the resampler's tail:
                    // pushing into a nearly full ring silently dropped the rest
                    // of the block, which is a hole in the middle of a word
                    let need = chunk_out + 64;

                    // Rate control on the real ring level. Comparing pushed with
                    // pulled sample counts jumped by a whole block at every window
                    // boundary, and that noise could walk the converter to ±3 % —
                    // audible as the voice speeding up and down and the timbre
                    // changing. The fill is the integral of the clock error, so a
                    // deadband around it is enough; inside it the converter runs
                    // at exactly 1.0 and nothing bends the pitch.
                    if drift_at.elapsed() >= Duration::from_secs(if drift_logs == 0 { 8 } else { 4 }) {
                        drift_at = Instant::now();
                        let fill_ms = f32::from_bits(proc_shared.out_fill.load(Ordering::Relaxed))
                            / out_rate.max(1) as f32
                            * 1000.0;
                        // the ring target adapts to the device: a cable that asks
                        // for 200 ms per callback needs a 200 ms ring
                        // `ref_ms` is computed above for this iteration
                        // where the rate should go, never more than 0.2 % away
                        // from 1.0: that is ~3.5 cents, below what anyone hears.
                        // Returning to 1.0 is allowed to be faster than the
                        // correction itself, so a one-off emergency cannot leave
                        // the pitch bent for a minute.
                        let err = fill_ms - ref_ms;
                        // proportional: the bigger the backlog, the further the
                        // read head advances per output sample (step > 1 = fewer
                        // samples), so a steady mismatch between the mic and the
                        // cable is cancelled at a small constant offset instead
                        // of filling the ring for an hour. Inside ±8 ms the
                        // converter runs at exactly 1.0, so the pitch is not
                        // being bent all the time.
                        let target: f64 = if fill_ms < 8.0 {
                            0.995
                        } else if fill_ms > buffer_ms + 120.0 {
                            1.005
                        } else if err.abs() < 8.0 {
                            1.0
                        } else {
                            (1.0 + 0.0002 * err as f64).clamp(0.995, 1.005)
                        };
                        let max_step = if (target - 1.0).abs() < 1e-9 {
                            0.002
                        } else {
                            0.001
                        };
                        slew_step += (target - slew_step).clamp(-max_step, max_step);
                        drift_logs += 1;
                        if drift_logs.is_multiple_of(8) {
                            crate::log::info(&format!(
                                "clock: fill {fill_ms:.0} ms, ref {ref_ms:.0} ms, cb {callback_ms:.0} ms, step {:.0} ppm, underruns {}, overflows {}, input drops {}",
                                (slew_step - 1.0) * 1e6,
                                proc_shared.underruns.load(Ordering::Relaxed),
                                proc_shared.overflows.load(Ordering::Relaxed),
                                proc_shared.capture_drops.load(Ordering::Relaxed),
                            ));
                        }
                        if trace {
                            eprintln!(
                                "drift: fill {fill_ms:.0} ms, ref {ref_ms:.0} ms, target {target:.4}, step {slew_step:.4}"
                            );
                        }
                        proc_shared
                            .slew_ppm
                            .store((((slew_step - 1.0) as f32) * 1e6).to_bits(), Ordering::Relaxed);
                    }
                    if proc_shared.out_raw.load(Ordering::Relaxed) as usize + need > max_fill {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }

                    for slot in block.iter_mut() {
                        *slot = in_cons.pop().unwrap_or(0.0);
                    }
                    let in_voice = input_voice.update(&block);

                    // silence costs nothing: skip the model and emit a clean
                    // zero block once the room has been quiet for a moment
                    let raw_rms = rms_of(&block);
                    if raw_rms < 6e-4 {
                        silent_run += 1;
                    } else {
                        silent_run = 0;
                    }

                    if silent_run > SILENCE_SKIP_BLOCKS {
                        // keep the pre-roll current: after a long gap it used
                        // to hold audio from before the silence, so a guard
                        // reset replayed the wrong thing
                        input_ring.extend_from_slice(&block);
                        let keep = 3200; // 200 ms at 16 kHz
                        if input_ring.len() > keep {
                            let drop = input_ring.len() - keep;
                            input_ring.drain(..drop);
                        }
                        // the detector has not seen audio for a while: leave the
                        // gate open, otherwise it needs ~300 ms to notice the
                        // next word and swallows the first syllable
                        dropout.skipped();
                        vad_gain = 1.0;
                        let produced = up.process(&zeros, &mut up_buf);
                        for sample in up_buf[..produced].iter_mut() {
                            *sample = 0.0;
                        }
                        ab_append_clean(
                        &proc_ab,
                        &proc_ab_done,
                        &proc_ab_active,
                        &proc_ab_folder,
                        &proc_ab_paths,
                        &up_buf[..produced],
                    );
                        analyzer.push(&up_buf[..produced]);
                        if let Ok(mut slot) = proc_shared.spectrum.lock() {
                            *slot = analyzer.bands();
                        }
                        // push at the model's own rate, no rate conversion
                        slew_out.clear();
                        slew.step = slew_step;
                        slew.process(&up_buf[..produced], &mut slew_out);
                        let mut written = 0usize;
                        for sample in &slew_out {
                            if out_prod.push(*sample).is_err() {
                                break;
                            }
                            written += 1;
                        }
                        if written < slew_out.len() {
                            proc_shared.overflows.fetch_add(1, Ordering::Relaxed);
                        }
                        // count what we produced, not what fitted: a full ring
                        // must not look like a slow producer
                        proc_shared.pushed.fetch_add(slew_out.len() as u64, Ordering::Relaxed);
                        continue;
                    }

                    let leveling = proc_shared.leveling.load(Ordering::Relaxed);
                    let allow_up = session_start.elapsed() > Duration::from_millis(1500) || speech_hold > 2;
                    leveler.process(&mut block, leveling, allow_up);
                    proc_shared
                        .leveler_gain
                        .store(leveler.gain_db().to_bits(), Ordering::Relaxed);
                    guard.process(&mut block);

                    // "before" mode: clean the input so VoxLock extracts the
                    // voice from a quieter signal
                    if let Some(model) = gtcrn_before.as_mut() {
                        res16.clear();
                        model.process(&block, &mut res16);
                        gtcrn_before_stage.take(&res16, &mut gtcrn_before_out, Some(&block));
                        block.copy_from_slice(&gtcrn_before_out);
                    }

                    persist_ticks += 1;
                    if persist_ticks >= 40 {
                        persist_ticks = 0;
                        let gain = leveler.gain_db();
                        if (gain - saved_level).abs() > 2.0 {
                            saved_level = gain;
                            let _ = std::fs::write(&level_path, format!("{gain:.1}"));
                        }
                    }

                    // the cap needs to know whether the *model input* is
                    // speech: the output-side VAD cannot answer while the model
                    // is cutting the speaker itself
                    let vad_in = input_vad.as_mut().map(|v| v.push(&block));
                    let cap_voice = vad_in.unwrap_or(in_voice) || in_voice;

                    // after a long stretch of the model suppressing everything,
                    // its speaker state is stale. Start it over before it sees
                    // the first word, otherwise it cuts 40+ dB until it finds
                    // the speaker again (the "no voice at the start" report).
                    let cold_onset = dropout.cold_onset(input_voice.voiced());
                    if cold_onset {
                        if let Some(model) = denoiser.as_mut() {
                            model.reset();
                            isolate_stage.reset();
                            // the residual stages were sitting on the room too
                            if let Some(model) = gtcrn_after.as_mut() {
                                model.reset();
                                gtcrn_stage.reset();
                            }
                            if let Some(model) = gtcrn_before.as_mut() {
                                model.reset();
                                gtcrn_before_stage.reset();
                            }
                            if let Some(model) = dpdfnet.as_mut() {
                                model.reset();
                                dpdf_stage.reset();
                            }
                            crate::log::info(&format!(
                                "cold start: VoxLock reset while the model was closed (in {:.0} dB)",
                                20.0 * raw_rms.max(1e-9).log10()
                            ));
                        }
                    }

                    let started = Instant::now();
                    // every stage below hands back exactly `chunk_samples`
                    let mut model_out_rms: Option<f32> = None;
                    match denoiser.as_mut() {
                        Some(model) => {
                            model.run_into(&block, &mut model_out);
                            // measure what the model really returned: the padded
                            // tail is silence and would look like a 40 dB cut
                            model_out_rms = if model_out.is_empty() {
                                None
                            } else {
                                Some(rms_of(&model_out))
                            };
                            isolate_stage.take(&model_out, &mut isolate_out, None);
                        }
                        // isolation off: the leveler output goes straight on to
                        // the noise stage, no target-speaker model at all
                        None => {
                            isolate_stage.reset();
                            isolate_out.copy_from_slice(&block);
                        }
                    }
                    let mut isolated: &[f32] = &isolate_out;

                    // VoxLock loses the speaker now and then and cuts 40+ dB
                    // for seconds. Reset it and replay the last 200 ms of the
                    // input so it locks again immediately; the output of the
                    // replay is thrown away, only the internal state is rebuilt.
                    if let Some(model) = denoiser.as_mut() {
                        let in_now = rms_of(&block);
                        let in_db = 20.0 * in_now.max(1e-9).log10();
                        // NaN or no output is as dead as a deep cut: the old
                        // comparison was false for NaN and the guard stayed
                        // blind for the whole episode
                        let out_db = match model_out_rms {
                            Some(v) if v.is_finite() && v > 0.0 => 20.0 * v.log10(),
                            _ => -120.0,
                        };
                        // with a suppression cap a 50 dB cut of the model is
                        // not visible in the output, so the dropout threshold
                        // has to stay above the cap
                        let cut_db = 26.0f32.max(max_suppression + 6.0);
                        // VoxLock needs a moment to lock on after a start
                        let warm = session_start.elapsed() > Duration::from_secs(1);
                        let reset_now = warm && dropout.observe(in_db, out_db, cap_voice, cut_db);
                        if reset_now {
                            proc_shared.guard_resets.fetch_add(1, Ordering::Relaxed);
                            crate::log::info(&format!(
                                "dropout guard: VoxLock reset + 200 ms pre-roll (in {in_db:.0} dB, out {out_db:.0} dB, reset #{})",
                                dropout.resets
                            ));
                            model.reset();
                            isolate_stage.reset();
                            for part in input_ring.chunks(chunk_samples) {
                                model.run_into(part, &mut model_out);
                            }
                            model.run_into(&block, &mut model_out);
                            model_out_rms = if model_out.is_empty() {
                                None
                            } else {
                                Some(rms_of(&model_out))
                            };
                            isolate_stage.take(&model_out, &mut isolate_out, None);
                            isolated = &isolate_out;
                        }
                        input_ring.extend_from_slice(&block);
                        let keep = 3200; // 200 ms at 16 kHz
                        if input_ring.len() > keep {
                            let drop = input_ring.len() - keep;
                            input_ring.drain(..drop);
                        }
                    }
                    out_scratch.clear();

                    let count = isolated.len().min(block.len());

                    // how much the model cut, measured on the block it just saw
                    let in_rms = rms_of(&block);
                    let out_rms = model_out_rms.unwrap_or_else(|| rms_of(&isolated[..count]));
                    let atten = if in_rms > 1e-6 && out_rms > 1e-8 {
                        -20.0 * (out_rms / in_rms).log10()
                    } else {
                        0.0
                    };
                    let speaking = in_rms > 1.8e-3;
                    proc_shared.speech.store(speaking, Ordering::Relaxed);

                    // Maximum suppression: if the model cut a speech block by
                    // more than the limit, mix the (leveled, peak-limited) model
                    // input back until the output is exactly `limit` down. This
                    // is the only guarantee that the speaker stays audible when
                    // VoxLock mistakes them for noise.
                    let need = if max_suppression > 0.0 && cap_voice && in_rms > 2e-3 {
                        cap_dry_mix(in_rms, out_rms, max_suppression)
                    } else {
                        0.0
                    };
                    // attack fast (the hole starts now), release slow
                    let coef = if need > cap_mix { 0.5 } else { 0.15 };
                    cap_mix += (need - cap_mix) * coef;
                    let cap_active = cap_mix > 0.01;

                    // the card shows the model's attenuation of *speech*, and
                    // never more than the cap (a 50 dB number only scares)
                    if (cap_voice || cap_active) && out_rms > 1e-8 {
                        let shown = if max_suppression > 0.0 {
                            atten.min(max_suppression)
                        } else {
                            atten
                        };
                        let current = f32::from_bits(proc_shared.suppression.load(Ordering::Relaxed));
                        let smoothed = current + (shown - current) * 0.1;
                        proc_shared.suppression.store(smoothed.to_bits(), Ordering::Relaxed);
                    }
                    // a block the model cut hard is not the target speaker: do
                    // not lift its level, unless the cap keeps the speaker
                    // audible anyway
                    chain.set_target_present(atten < 8.0 || cap_active);
                    let wet = f32::from_bits(proc_shared.wet.load(Ordering::Relaxed)).clamp(0.0, 1.0);
                    // `cap_mix` is the fraction of the model input to mix back,
                    // so it puts a floor under how far `wet` may go
                    let wet_eff = if cap_mix > 0.0 {
                        wet.min(1.0 - cap_mix)
                    } else {
                        wet
                    };

                    // After a knock, or when speech starts, the model needs a
                    // moment to lock onto the enrolled voice and outputs almost
                    // nothing. Keep a little of the dry signal for that second,
                    // so the first words are not swallowed.
                    let mut peak = 0.0f32;
                    for sample in block.iter() {
                        peak = peak.max(sample.abs());
                    }
                    let crest_db = 20.0 * ((peak + 1e-9) / (rms_of(&block) + 1e-9)).log10();
                    let block_rms = rms_of(&block);
                    let rms_db = 20.0 * (block_rms + 1e-9).log10();
                    // hold the speech state for ~0.4 s so syllables do not
                    // retrigger the onset boost on every word
                    if block_rms > 1.8e-3 {
                        speech_hold = 7;
                    } else {
                        speech_hold = speech_hold.saturating_sub(1);
                    }
                    let speaking = speech_hold > 0;
                    // a knock is a click in an otherwise quiet block
                    let knock = crest_db > 19.0 && rms_db < -50.0;
                    let onset_now = (speaking && !was_speaking) || knock;
                    if onset_now {
                        onset_blocks = 5; // ~0.3 s at 60 ms blocks
                    }
                    was_speaking = speaking;

                    // The model needs 40-400 ms to pass the first syllable after a
                    // pause and outputs exact zeros for that time. Replacing that
                    // gap with the dry input was tried and **reverted**: the dry
                    // signal brings back the room and other voices, which breaks
                    // the whole point of the app. The gap stays; only the 3 dB
                    // onset boost below softens it.

                    onset_blocks = onset_blocks.saturating_sub(1);

                    for i in 0..count {
                        let mixed = block[i] * (1.0 - wet_eff) + isolated[i] * wet_eff;
                        out_scratch.push(mixed);
                    }
                    #[allow(clippy::needless_range_loop)]
                    for i in count..block.len() {
                        out_scratch.push(block[i]);
                    }

                    // Silero VAD gate: close the output in pauses, keep every
                    // syllable. A level threshold cannot tell a quiet word from
                    // noise, the detector can.
                    let mut voiced: Option<bool> = None;
                    if let Some(detector) = vad.as_mut() {
                        let speech = detector.push(&out_scratch);
                        voiced = Some(speech);
                        let target = if speech { 1.0f32 } else { 0.0f32 };
                        let coef = if target > vad_gain { vad_attack } else { vad_release };
                        for sample in out_scratch.iter_mut() {
                            vad_gain += (target - vad_gain) * coef;
                            *sample *= vad_gain;
                        }
                    }
                    // the model lets the level fall 10-20 dB between syllables; a
                    // gain lift was tried here and **reverted**: it raised the
                    // residual noise and other voices along with the quiet
                    // syllables. The level stays as the model produced it.
                    // the detector knows better than the attenuation whether
                    // this block is the target speaker: with a profile the model
                    // cuts even the enrolled voice by 10-14 dB, and refusing to
                    // lift those blocks made phrase levels differ by 5 dB or more
                    if let Some(speech) = voiced {
                        // during a capped cut the model may be cutting the
                        // speaker itself; keep the level control working so the
                        // first words are audible as soon as it passes them
                        chain.set_target_present(speech || cap_active);
                    }

                    // GTCRN works at 16 kHz, right after VoxLock
                    let mut source: &[f32] = &out_scratch;
                    if let Some(model) = gtcrn_after.as_mut() {
                        res16.clear();
                        model.process(&out_scratch, &mut res16);
                        gtcrn_stage.take(&res16, &mut gtcrn_out, Some(&out_scratch));
                        source = &gtcrn_out;
                    }

                    let produced = up.process(source, &mut up_buf);

                    // DPDFNet works at 48 kHz, after the resampler
                    if let Some(model) = dpdfnet.as_mut() {
                        res48.clear();
                        model.process(&up_buf[..produced], &mut res48);
                        dpdf_stage.take(&res48, &mut dpdf_out[..produced], Some(&up_buf[..produced]));
                        up_buf[..produced].copy_from_slice(&dpdf_out[..produced]);
                    }

                    if sanitize(&mut up_buf[..produced]) {
                        let seen = proc_shared.bad_blocks.fetch_add(1, Ordering::Relaxed) + 1;
                        if seen == 1 {
                            crate::log::info("model returned NaN/Inf — block replaced with silence");
                        }
                    }
                    chain.process(&mut up_buf[..produced]);

                    // the model needs about a second to lock onto the speaker;
                    // lift its output for that second so the first words are
                    // not swallowed (the ramp makes the return inaudible)
                    if onset_blocks > 0 {
                        let boost_db = 3.0 * (onset_blocks as f32 / 5.0);
                        let lin = 10f32.powf(boost_db / 20.0);
                        for sample in up_buf[..produced].iter_mut() {
                            *sample = (*sample * lin).clamp(-1.0, 1.0);
                        }
                    }
                    ab_append_clean(
                        &proc_ab,
                        &proc_ab_done,
                        &proc_ab_active,
                        &proc_ab_folder,
                        &proc_ab_paths,
                        &up_buf[..produced],
                    );

                    analyzer.push(&up_buf[..produced]);
                    if let Ok(mut slot) = proc_shared.spectrum.lock() {
                        *slot = analyzer.bands();
                    }

                    // push through the slow rate converter
                    slew_out.clear();
                    slew.step = slew_step;
                    slew.process(&up_buf[..produced], &mut slew_out);

                    let mut written = 0usize;
                    for sample in &slew_out {
                        if out_prod.push(*sample).is_err() {
                            break;
                        }
                        written += 1;
                    }
                    if written < slew_out.len() {
                        // the ring was full: these samples never reached the device
                        proc_shared.overflows.fetch_add(1, Ordering::Relaxed);
                    }
                    // count what actually landed in the ring: `pushed` used to add
                    // the whole block even when it was dropped, which made
                    // pushed/pulled look 2 % off and dragged the old rate
                    // controller with it
                    proc_shared.pushed.fetch_add(written as u64, Ordering::Relaxed);

                    let _available = out_prod.slots();

                    let elapsed = started.elapsed();
                    rtf_window.0 += count;
                    rtf_window.1 += elapsed;
                    if rtf_window.0 >= MODEL_RATE as usize {
                        let rtf = rtf_window.1.as_secs_f32() / (rtf_window.0 as f32 / MODEL_RATE as f32);
                        proc_shared.rtf.store(rtf.to_bits(), Ordering::Relaxed);
                        rtf_window = (0, Duration::ZERO);
                    }

                    if cpu_window_start.elapsed() > Duration::from_secs(1) {
                        cpu_window_start = Instant::now();
                    }
                }
            })
            .map_err(|e| anyhow!("processor thread: {e}"))?;

        // input stream -----------------------------------------------------
        let in_shared = shared.clone();
        let mut down = Resampler::new(in_rate, MODEL_RATE);
        let mut mono = vec![0.0f32; 8192];
        let mut down_buf = vec![0.0f32; 8192];

        let in_config: StreamConfig = in_cfg.config();
        let channels = in_config.channels as usize;
        let in_format = in_cfg.sample_format();

        fn err_fn(err: cpal::Error) {
        eprintln!("input stream error: {err}");
        crate::log::info(&format!("input stream error: {err}"));
    }

        let input_stream = match in_format {
            SampleFormat::F32 => input.build_input_stream(
                in_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    capture_f32(data, channels, &mut mono, &mut down, &mut down_buf, &mut in_prod, &in_shared, &in_ab);
                },
                err_fn,
                None,
            )?,
            SampleFormat::I16 => input.build_input_stream(
                in_config,
                {
                    // one buffer for the whole stream: converting in the audio
                    // callback must not allocate
                    let mut converted: Vec<f32> = Vec::new();
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        converted.clear();
                        converted.extend(data.iter().map(|v| *v as f32 / 32768.0));
                        capture_f32(
                            &converted,
                            channels,
                            &mut mono,
                            &mut down,
                            &mut down_buf,
                            &mut in_prod,
                            &in_shared,
                            &in_ab,
                        );
                    }
                },
                err_fn,
                None,
            )?,
            other => return Err(anyhow!("unsupported input sample format: {other:?}")),
        };

        // output stream ----------------------------------------------------
        let out_config: StreamConfig = out_cfg.config();
        let out_channels = out_config.channels as usize;
        let out_format = out_cfg.sample_format();
        fn out_err(err: cpal::Error) {
        eprintln!("output stream error: {err}");
        crate::log::info(&format!("output stream error: {err}"));
    }

        let shared_for_out = shared.clone();
        let mut smoothed_fill = 0.0f32;
        let output_stream = match out_format {
            SampleFormat::F32 => output.build_output_stream(
                out_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    render_f32(
                        data,
                        out_channels,
                        &mut out_cons,
                        &shared_for_out,
                        out_rate,
                        &mut smoothed_fill,
                    );
                },
                out_err,
                None,
            )?,
            SampleFormat::I16 => output.build_output_stream(
                out_config,
                {
                    // one conversion buffer for the stream: no allocation in
                    // the audio callback
                    let mut tmp: Vec<f32> = Vec::new();
                    move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                        tmp.resize(data.len(), 0.0);
                        render_f32(
                            &mut tmp,
                            out_channels,
                            &mut out_cons,
                            &shared_for_out,
                            out_rate,
                            &mut smoothed_fill,
                        );
                        for (dst, src) in data.iter_mut().zip(tmp.iter()) {
                            *dst = (*src * 32767.0).clamp(-32768.0, 32767.0) as i16;
                        }
                    }
                },
                out_err,
                None,
            )?,
            other => return Err(anyhow!("unsupported output sample format: {other:?}")),
        };

        input_stream.play()?;
        output_stream.play()?;

        crate::log::info(&format!(
            "engine start: in='{}' {} Hz, out='{}' {} Hz, block {} ms, {} threads, buffer {} ms",
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
            settings.buffer_ms
        ));

        Ok(Self {
            shared,
            input: Some(input_stream),
            output: Some(output_stream),
            processor: Some(processor),
            backend,
            in_rate,
            out_rate,
            threads,
        })
    }

    pub fn update_settings(&self, settings: &Settings) {
        if let Ok(mut guard) = self.shared.settings.lock() {
            *guard = settings.clone();
        }
        self.shared
            .wet
            .store(settings.isolation_strength.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        self.shared
            .buffer_ms
            .store(settings.buffer_ms.to_bits(), Ordering::Relaxed);
        self.shared
            .leveling
            .store(settings.input_leveling, Ordering::Relaxed);
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

    pub fn stats(&self) -> Stats {        Stats {
            input_rms: f32::from_bits(self.shared.input_rms.load(Ordering::Relaxed)),
            output_rms: f32::from_bits(self.shared.output_rms.load(Ordering::Relaxed)),
            rtf: f32::from_bits(self.shared.rtf.load(Ordering::Relaxed)),
            latency_ms: f32::from_bits(self.shared.latency.load(Ordering::Relaxed)),
            underruns: self.shared.underruns.load(Ordering::Relaxed),
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
                * f32::from_bits(self.shared.input_peak.load(Ordering::Relaxed)).max(1e-6).log10(),
            muted: self.shared.muted.load(Ordering::Relaxed),
            guard_resets: self.shared.guard_resets.load(Ordering::Relaxed),
            leveler_gain_db: f32::from_bits(self.shared.leveler_gain.load(Ordering::Relaxed)),
            slew_ppm: f32::from_bits(self.shared.slew_ppm.load(Ordering::Relaxed)),
            threads: self.threads,
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
                let path = paths::recordings_dir().join(format!("ab-{stamp}.json"));
                let _ = std::fs::write(path, text);
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

pub fn cable_present() -> bool {
    find_cable(&cpal::default_host()).is_some()
}

fn find_cable(host: &cpal::Host) -> Option<cpal::Device> {
    let list = host.output_devices().ok()?;
    for device in list {
        let name = device
            .description()
            .ok()
            .map(|d| d.name().to_string())
            .unwrap_or_default();
        if name.contains("CABLE Input") || name.contains("VB-Audio") || name.contains("VB-CABLE") {
            return Some(device);
        }
    }
    None
}

fn pick_device(host: &cpal::Host, name: Option<&str>, input: bool) -> Result<cpal::Device> {
    if let Some(target) = name {
        if !target.is_empty() {
            let list = if input { host.input_devices()? } else { host.output_devices()? };
            for device in list {
                if device.description().ok().map(|d| d.name().to_string()).as_deref() == Some(target)
                {
                    return Ok(device);
                }
            }
            return Err(anyhow!(
                "устройство «{target}» не найдено — обновите список устройств"
            ));
        }
    }

    if input {
        host.default_input_device().ok_or_else(|| anyhow!("no input device"))
    } else {
        host.default_output_device().ok_or_else(|| anyhow!("no output device"))
    }
}

#[allow(clippy::too_many_arguments)]
fn capture_f32(
    data: &[f32],
    channels: usize,
    mono: &mut Vec<f32>,
    down: &mut Resampler,
    down_buf: &mut Vec<f32>,
    ring: &mut rtrb::Producer<f32>,
    shared: &Shared,
    ab: &std::sync::Arc<std::sync::Mutex<Option<AbBuffer>>>,
) {
    let frames = data.len() / channels.max(1);
    if frames == 0 {
        return;
    }
    if mono.len() < frames {
        mono.resize(frames, 0.0);
    }

    let mut sum_sq = 0.0f64;
    let mut peak = 0.0f32;
    for f in 0..frames {
        let mut acc = 0.0f32;
        for c in 0..channels {
            acc += data[f * channels + c];
        }
        let value = acc / channels as f32;
        mono[f] = value;
        sum_sq += (value as f64) * (value as f64);
        peak = peak.max(value.abs());
    }

    ab_append_raw(ab, &mono[..frames]);
    shared.in_frames.fetch_add(frames as u64, Ordering::Relaxed);

    let rms = (sum_sq / frames as f64).sqrt() as f32;
    shared.input_rms.store(rms.to_bits(), Ordering::Relaxed);
    // decaying peak: a 5 ms clip must survive long enough for the 150 ms UI poll
    let held = (f32::from_bits(shared.input_peak.load(Ordering::Relaxed)) * 0.995).max(peak);
    shared.input_peak.store(held.to_bits(), Ordering::Relaxed);

    let needed = down.max_output(frames);
    if down_buf.len() < needed {
        down_buf.resize(needed, 0.0);
    }
    let produced = down.process(&mono[..frames], down_buf);
    let mut dropped = 0usize;
    for (i, sample) in down_buf[..produced].iter().enumerate() {
        if ring.push(*sample).is_err() {
            // the processor is behind: the rest of this callback will not fit
            dropped = produced - i;
            break;
        }
    }
    if dropped > 0 {
        shared
            .capture_drops
            .fetch_add(dropped as u32, Ordering::Relaxed);
    }
}

fn render_f32(
    data: &mut [f32],
    channels: usize,
    ring: &mut rtrb::Consumer<f32>,
    shared: &Shared,
    out_rate: u32,
    smoothed: &mut f32,
) {
    let mut sum_sq = 0.0f64;
    let frames = data.len() / channels.max(1);
    let mut last = 0.0f32;

    // smooth the fill over ~0.5 s: the producer pushes a whole block at once, so
    // the raw value is a sawtooth and a controller fed with it hunts
    let fill = ring.slots() as f32;
    shared.out_raw.store(ring.slots() as u32, Ordering::Relaxed);
    shared.callback_frames.store(frames as u32, Ordering::Relaxed);
    if *smoothed <= 0.0 {
        *smoothed = fill;
    }
    *smoothed += (fill - *smoothed) * 0.02;
    // stored as f32 bits: the controller reads it with `from_bits`, and storing
    // a plain number here made it read back as ~0, so the ring always looked
    // starved
    shared.out_fill.store(smoothed.to_bits(), Ordering::Relaxed);
    shared.pulled.fetch_add(frames as u64, Ordering::Relaxed);

    let muted = shared.muted.load(Ordering::Relaxed);

    for f in 0..frames {
        // a starved ring repeats the last sample instead of inserting silence: a
        // hole in the middle of a word is what "words disappear" sounds like.
        // The repeat decays, so a longer gap fades out instead of holding DC and
        // clicking when the audio comes back.
        let value = match ring.pop() {
            Ok(sample) => sample,
            Err(_) => {
                shared.underruns.fetch_add(1, Ordering::Relaxed);
                last *= 0.985;
                last
            }
        };
        last = value;
        sum_sq += (value as f64) * (value as f64);
        // muted: keep draining so the timing does not drift, but send silence
        let value = if muted { 0.0 } else { value };
        for c in 0..channels {
            data[f * channels + c] = value;
        }
    }

    let rms = if frames > 0 {
        (sum_sq / frames as f64).sqrt() as f32
    } else {
        0.0
    };
    shared.output_rms.store(rms.to_bits(), Ordering::Relaxed);

    // real buffer fill seen by the consumer, plus VoxLock's own 24 ms of latency
    let fill_ms = ring.slots() as f32 / out_rate.max(1) as f32 * 1000.0;
    shared.latency.store((fill_ms + 24.0).to_bits(), Ordering::Relaxed);
}

static PANIC_HOOK: std::sync::Once = std::sync::Once::new();

/// A panic in the processing thread used to leave silence with no trace; log it
/// so `data/app.log` explains what happened.
fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            crate::log::info(&format!("panic: {info}"));
            default_hook(info);
        }));
    });
}

/// Replaces NaN/Inf with silence. One bad sample would poison the biquad
/// states for the rest of the session, and the user would hear nothing at all.
fn sanitize(block: &mut [f32]) -> bool {
    if block.iter().all(|s| s.is_finite()) {
        return false;
    }
    for sample in block.iter_mut() {
        if !sample.is_finite() {
            *sample = 0.0;
        }
    }
    true
}

fn rms_of(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0f64;
    for sample in samples {
        sum += (*sample as f64) * (*sample as f64);
    }
    (sum / samples.len() as f64).sqrt() as f32
}

fn ab_append_raw(ab: &Arc<Mutex<Option<AbBuffer>>>, samples: &[f32]) {
    // `try_lock`: this runs inside the capture callback, and blocking there is
    // exactly how a take gets a hole. The processor holds the lock only for the
    // length of one `extend_from_slice`, so a skipped callback is rare.
    if let Ok(mut slot) = ab.try_lock() {
        if let Some(buf) = slot.as_mut() {
            let limit = buf.raw_rate as usize * 90;
            if buf.raw.len() < limit {
                buf.raw.extend_from_slice(samples);
            }
        }
    }
}

fn ab_append_clean(
    ab: &Arc<Mutex<Option<AbBuffer>>>,
    done: &Arc<AtomicBool>,
    active: &Arc<AtomicBool>,
    folder: &Arc<Mutex<Option<String>>>,
    paths: &Arc<Mutex<(Option<String>, Option<String>)>>,
    samples: &[f32],
) {
    let mut guard = match ab.lock() {
        Ok(g) => g,
        Err(_) => return,
    };

    let Some(buf) = guard.as_mut() else {
        return;
    };

    buf.clean.extend_from_slice(samples);
    if buf.clean.len() < buf.target_out {
        return;
    }

    let finished = match guard.take() {
        Some(buf) => buf,
        None => return,
    };
    drop(guard);

    // writing two wav files takes tens of milliseconds; doing it here would
    // stall the processing thread at the exact moment the take ends
    let done = done.clone();
    let active = active.clone();
    let folder = folder.clone();
    let paths = paths.clone();
    std::thread::spawn(move || {
        match write_ab(&finished) {
            Ok((dir, raw, clean)) => {
                if let Ok(mut slot) = folder.lock() {
                    *slot = Some(dir);
                }
                if let Ok(mut slot) = paths.lock() {
                    *slot = (Some(raw), Some(clean));
                }
            }
            Err(err) => {
                eprintln!("a/b write failed: {err}");
                crate::log::info(&format!("a/b write failed: {err}"));
            }
        }

        done.store(true, Ordering::Relaxed);
        active.store(false, Ordering::Relaxed);
    });
}

fn write_ab(buf: &AbBuffer) -> anyhow::Result<(String, String, String)> {
    let dir = paths::recordings_dir();
    let raw = dir.join(format!("ab-{}-raw.wav", buf.stamp));
    let clean = dir.join(format!("ab-{}-clean.wav", buf.stamp));
    write_mono(&raw, &buf.raw, buf.raw_rate)?;
    write_mono(&clean, &buf.clean, buf.out_rate)?;
    Ok((
        dir.to_string_lossy().to_string(),
        raw.to_string_lossy().to_string(),
        clean.to_string_lossy().to_string(),
    ))
}

fn write_mono(path: &std::path::Path, samples: &[f32], rate: u32) -> anyhow::Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for sample in samples {
        writer.write_sample((sample.clamp(-1.0, 1.0) * 32767.0) as i16)?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            leveler.process(&mut block, true, true);
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
        leveler.process(&mut block, false, true);
        let out_db = 20.0 * rms_of(&block).log10();
        assert!((out_db - -40.0).abs() < 1.0, "expected untouched level, got {out_db:.1}");
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
            leveler.process(&mut block, true, true);
        }
        let settled = leveler.gain_db;

        // the television, 17 dB quieter, for five seconds
        for _ in 0..80 {
            let mut block = tone(0.0031, 960, &mut phase);
            leveler.process(&mut block, true, true);
        }
        let moved = leveler.gain_db - settled;
        assert!(moved < 1.0, "the television moved the leveler by {moved:.1} dB");
    }

    #[test]
    fn leveler_ignores_knocks() {
        let mut leveler = Leveler::new(0.0);
        let mut phase = 0.0f32;

        // settle on quiet speech first
        for _ in 0..30 {
            let mut block = tone(0.01, 960, &mut phase);
            leveler.process(&mut block, true, true);
        }
        let before = leveler.gain_db;

        // a knock: one loud sample in an otherwise silent block
        let mut knock = vec![0.0f32; 960];
        knock[0] = 0.9;
        leveler.process(&mut knock, true, true);

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
        assert!((speech[959] - 0.1).abs() < 1e-3, "speech was changed to {}", speech[959]);

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
        assert!(guard.cold_onset(true), "the first word must restart the model");
        assert!(guard.cold());
        // a cold onset while the model was open is not a cold start
        let mut open = DropoutGuard::new();
        for _ in 0..8 {
            open.observe(-30.0, -28.0, true, 26.0);
        }
        assert!(!open.cold_onset(true));
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
        assert!(resets <= 4, "the guard reset {resets} times: it should be bounded");
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
}
