//! Pure building blocks of the chain: rate converter, fixed-length model
//! stages, voice and knock detectors, leveler, peak guard, suppression cap.

pub(super) struct Slew {
    pos: f64,
    pub(super) step: f64,
    tail: [f32; 4],
    buf: Vec<f32>,
}

impl Slew {
    pub(super) fn new() -> Self {
        Self {
            pos: 2.0,
            step: 1.0,
            tail: [0.0; 4],
            buf: Vec::new(),
        }
    }

    pub(super) fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
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
    pub(super) fn queued(&self) -> usize {
        self.pending.len()
    }
}

/// Where the *microphone* is in the conversation, independent of VoxLock.
///
/// The output-side VAD follows the model, so while the model is cold it reports
/// silence exactly when the speaker is talking. The cold-start reset, the
/// suppression cap and the level control need a verdict on the input instead.
/// A phrase starts on a level jump over the room's average, and it *continues*
/// while the mic stays well above the room it started from: after a couple of
/// seconds the average catches up with the voice, so a pure jump-over-average
/// would end the phrase in the middle of a sentence and the gain would fall
/// back to zero there (heard as the level galloping inside one phrase).
pub struct InputVoice {
    /// this block is speech (with a short hold through word gaps)
    speech: bool,
    /// this block itself jumped above the room (one block, no hold)
    voiced: bool,
    run: u32,
    avg_db: Option<f32>,
    /// room level captured when the current phrase was confirmed
    base_db: f32,
    last_db: f32,
    hold: u32,
}

impl InputVoice {
    /// speech sits at least this far above the room's average
    const JUMP_DB: f32 = 8.0;
    /// ...and this far above the room the phrase started from, to continue
    const SUSTAIN_DB: f32 = 6.0;
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
            base_db: -120.0,
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

        let jumped = (db - avg > Self::JUMP_DB && db > -45.0) || (db - avg > 15.0 && db > -55.0);
        self.voiced = jumped;
        if jumped {
            if self.run == 0 {
                // the room the phrase started from, not the running average
                self.base_db = avg;
            }
            self.run = self.run.saturating_add(1);
        } else {
            self.run = 0;
        }

        if self.run >= Self::CONFIRM_BLOCKS {
            self.hold = Self::HOLD_BLOCKS;
        } else if self.hold > 0 && db > self.base_db + Self::SUSTAIN_DB {
            // still well above the room: the phrase continues even though the
            // average now sits at the voice level
            self.hold = Self::HOLD_BLOCKS;
        } else {
            self.hold = self.hold.saturating_sub(1);
        }
        self.speech = self.hold > 0;
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

        // A real hole in a loud block is obvious after two blocks; waiting for
        // the third costs another 64 ms of the word, and the word starts there.
        let confirm = if in_db > -40.0 { 2 } else { 3 };
        if self.cut >= confirm && self.cool == 0 && (self.armed || cold) && self.retries < 4 {
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

    /// The model was just restarted from outside (a leak reset): forget the
    /// runs, keep the counters.
    pub fn restart(&mut self) {
        self.cut = 0;
        self.open = 0;
        self.closed = 0;
        self.retries = 0;
        self.armed = false;
    }
}

impl Default for DropoutGuard {
    fn default() -> Self {
        Self::new()
    }
}

/// The opposite failure of [`DropoutGuard`]: VoxLock stops isolating and starts
/// passing whatever is in the room — a television, a phone playing an audiobook.
/// The over-suppression guard cannot see this, and A/B takes showed the leak in
/// 100–400 ms bursts right at phrase boundaries.
///
/// It only acts while the caller says the user is silent *and* the model had
/// been holding the room down a moment before, so a quiet phrase of the user
/// (which has no level jump) is not mistaken for a leak, and a model that never
/// suppressed anything is left alone.
pub struct LeakGuard {
    /// consecutive blocks the model has been suppressing hard
    closed: u32,
    /// blocks left in which a "was closed" run still arms the guard
    armed: u32,
    /// consecutive blocks the model passes while the user is silent
    passing: u32,
    cool: u32,
    pub resets: u32,
}

impl LeakGuard {
    /// attenuation that counts as "the model is holding the room down"
    const CLOSED_DB: f32 = 20.0;
    /// attenuation below this means the model is passing the room through
    const PASSING_DB: f32 = 10.0;
    /// the input has to be audible at all, dBFS
    const AUDIBLE_DB: f32 = -52.0;
    /// ~0.3 s of leaking at 64 ms blocks before the model is restarted
    const CONFIRM_BLOCKS: u32 = 5;
    /// blocks of memory for "it was closed just now" (~2 s at 64 ms)
    const ARM_BLOCKS: u32 = 32;
    /// ~2 s between resets
    const COOL_BLOCKS: u32 = 30;

    pub fn new() -> Self {
        Self {
            closed: 0,
            armed: 0,
            passing: 0,
            cool: 0,
            resets: 0,
        }
    }

    /// Called at the top of a block with what the model did to the *previous*
    /// one. `speaker` is the mic-side verdict from [`InputVoice`].
    pub fn observe(&mut self, atten_db: f32, in_db: f32, speaker: bool) -> bool {
        self.cool = self.cool.saturating_sub(1);
        self.armed = self.armed.saturating_sub(1);

        if atten_db > Self::CLOSED_DB {
            self.closed = self.closed.saturating_add(1);
            if self.closed >= 8 {
                self.armed = Self::ARM_BLOCKS;
            }
        } else {
            self.closed = 0;
        }

        let passing = !speaker && in_db > Self::AUDIBLE_DB && atten_db < Self::PASSING_DB;
        if passing {
            self.passing = self.passing.saturating_add(1);
        } else {
            self.passing = 0;
        }

        if self.passing >= Self::CONFIRM_BLOCKS && self.cool == 0 && self.armed > 0 {
            self.passing = 0;
            self.armed = 0;
            self.cool = Self::COOL_BLOCKS;
            self.resets += 1;
            return true;
        }
        false
    }
}

impl Default for LeakGuard {
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
    pub(super) gain_db: f32,
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

    pub fn process(&mut self, block: &mut [f32], enabled: bool, allow_up: bool, speaker: bool) {
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

        // a steady bed (music, a television) is not the speaker even when the
        // microphones sees a lot of it: only blocks that jumped over the room
        // may move the gain, otherwise the level follows the background and the
        // voice "catches up" for a second after it stops
        if enabled && speaker && !transient && above_floor > 8.0 && rms_db > -58.0 {
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
                    // normal move is slow; only the start of a session may
                    // catch up quickly, when the remembered gain has not been
                    // confirmed by this room yet. A limit per block of 1.2 dB
                    // is ~19 dB/s: fast enough to follow a mic, slow enough
                    // not to be heard as the level "galloping" between words.
                    let catch_up = self.voice_blocks < 12;
                    let (coef, limit) = if catch_up {
                        (0.6, 4.0)
                    } else if wanted < self.gain_db {
                        (0.3, 1.2)
                    } else {
                        (0.12, 1.0)
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

        let target = if enabled {
            10f32.powf(self.gain_db / 20.0)
        } else {
            1.0
        };
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

/// Replaces NaN/Inf with silence. One bad sample would poison the biquad
/// states for the rest of the session, and the user would hear nothing at all.
pub(super) fn sanitize(block: &mut [f32]) -> bool {
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

pub fn rms_of(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0f64;
    for sample in samples {
        sum += (*sample as f64) * (*sample as f64);
    }
    (sum / samples.len() as f64).sqrt() as f32
}
