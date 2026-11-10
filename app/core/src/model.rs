//! VoxLock through the sherpa-onnx C API (the reference streaming implementation).

use std::ffi::{c_char, c_void, CString};
use std::path::Path;
use std::ptr;
use std::sync::OnceLock;

use anyhow::{anyhow, Result};
use libloading::Library;

#[repr(C)]
struct GtcrnConfig {
    model: *const c_char,
}

/// Part of the C struct layout and therefore part of `ModelConfig`; the app no
/// longer offers DPDFNet (it clipped words and clicked), so it stays null.
#[repr(C)]
struct DpdfNetConfig {
    model: *const c_char,
    attenuation_limit_db: f32,
}

#[repr(C)]
struct VoxLockConfig {
    enroll_model: *const c_char,
    chunk_model: *const c_char,
    voice_sample: *const c_char,
}

#[repr(C)]
struct ModelConfig {
    gtcrn: GtcrnConfig,
    num_threads: i32,
    debug: i32,
    provider: *const c_char,
    dpdfnet: DpdfNetConfig,
    voxlock: VoxLockConfig,
}

#[repr(C)]
struct OnlineConfig {
    model: ModelConfig,
}

#[repr(C)]
struct DenoisedAudio {
    samples: *const f32,
    n: i32,
    sample_rate: i32,
}

type CreateFn = unsafe extern "C" fn(*const OnlineConfig) -> *const c_void;
type DestroyFn = unsafe extern "C" fn(*const c_void);
type SampleRateFn = unsafe extern "C" fn(*const c_void) -> i32;
type FrameShiftFn = unsafe extern "C" fn(*const c_void) -> i32;
type RunFn = unsafe extern "C" fn(*const c_void, *const f32, i32, i32) -> *const DenoisedAudio;
type FlushFn = unsafe extern "C" fn(*const c_void) -> *const DenoisedAudio;
type FreeFn = unsafe extern "C" fn(*const DenoisedAudio);

#[repr(C)]
struct SileroVadConfig {
    model: *const c_char,
    threshold: f32,
    min_silence_duration: f32,
    min_speech_duration: f32,
    window_size: i32,
    max_speech_duration: f32,
}

#[repr(C)]
struct TenVadConfig {
    model: *const c_char,
    threshold: f32,
    min_silence_duration: f32,
    min_speech_duration: f32,
    window_size: i32,
    max_speech_duration: f32,
}

#[repr(C)]
struct VadModelConfig {
    silero_vad: SileroVadConfig,
    sample_rate: i32,
    num_threads: i32,
    provider: *const c_char,
    debug: i32,
    ten_vad: TenVadConfig,
}

type VadCreateFn = unsafe extern "C" fn(*const VadModelConfig, f32) -> *const c_void;
type VadDestroyFn = unsafe extern "C" fn(*const c_void);
type VadAcceptFn = unsafe extern "C" fn(*const c_void, *const f32, i32);
type VadDetectedFn = unsafe extern "C" fn(*const c_void) -> i32;
type VadResetFn = unsafe extern "C" fn(*const c_void);
type DenoiserResetFn = unsafe extern "C" fn(*const c_void);

struct Api {
    _lib: Library,
    create: CreateFn,
    destroy: DestroyFn,
    sample_rate: SampleRateFn,
    frame_shift: FrameShiftFn,
    run: RunFn,
    flush: FlushFn,
    free: FreeFn,
    vad_create: VadCreateFn,
    vad_destroy: VadDestroyFn,
    vad_accept: VadAcceptFn,
    vad_detected: VadDetectedFn,
    vad_reset: VadResetFn,
    denoiser_reset: DenoiserResetFn,
}

static API: OnceLock<Api> = OnceLock::new();

fn api() -> Result<&'static Api> {
    if let Some(api) = API.get() {
        return Ok(api);
    }

    let _ = crate::paths::data_dir();

    // find the DLLs: native/, then models/, then next to the exe
    let candidates = [
        crate::paths::native_dir().join("sherpa-onnx-c-api.dll"),
        crate::paths::models_dir().join("sherpa-onnx-c-api.dll"),
        crate::paths::exe_dir().join("sherpa-onnx-c-api.dll"),
    ];

    let path = candidates
        .iter()
        .find(|p| p.exists())
        .ok_or_else(|| anyhow!("sherpa-onnx-c-api.dll not found"))?;

    crate::paths::set_native_search_dir(path.parent().unwrap_or(Path::new(".")));

    unsafe {
        let lib = Library::new(path)?;
        let create = *lib.get(b"SherpaOnnxCreateOnlineSpeechDenoiser\0")?;
        let destroy = *lib.get(b"SherpaOnnxDestroyOnlineSpeechDenoiser\0")?;
        let sample_rate = *lib.get(b"SherpaOnnxOnlineSpeechDenoiserGetSampleRate\0")?;
        let frame_shift = *lib.get(b"SherpaOnnxOnlineSpeechDenoiserGetFrameShiftInSamples\0")?;
        let run = *lib.get(b"SherpaOnnxOnlineSpeechDenoiserRun\0")?;
        let flush = *lib.get(b"SherpaOnnxOnlineSpeechDenoiserFlush\0")?;
        let free = *lib.get(b"SherpaOnnxDestroyDenoisedAudio\0")?;
        let vad_create = *lib.get(b"SherpaOnnxCreateVoiceActivityDetector\0")?;
        let vad_destroy = *lib.get(b"SherpaOnnxDestroyVoiceActivityDetector\0")?;
        let vad_accept = *lib.get(b"SherpaOnnxVoiceActivityDetectorAcceptWaveform\0")?;
        let vad_detected = *lib.get(b"SherpaOnnxVoiceActivityDetectorDetected\0")?;
        let vad_reset = *lib.get(b"SherpaOnnxVoiceActivityDetectorReset\0")?;
        let denoiser_reset = *lib.get(b"SherpaOnnxOnlineSpeechDenoiserReset\0")?;

        let api = Api {
            _lib: lib,
            create,
            destroy,
            sample_rate,
            frame_shift,
            run,
            flush,
            free,
            vad_create,
            vad_destroy,
            vad_accept,
            vad_detected,
            vad_reset,
            denoiser_reset,
        };

        let _ = API.set(api);
    }

    API.get().ok_or_else(|| anyhow!("sherpa-onnx API not initialised"))
}

pub struct Denoiser {
    handle: *const c_void,
    sample_rate: i32,
    frame_shift: i32,
}

/// A denoiser wrapped with sample-accurate framing: the model is always fed
/// exactly `frame_shift` samples, and its output is queued, so the caller sees
/// one continuous stream instead of blocks of varying length (that mismatch is
/// what made DPDFNet click).
pub struct StreamDenoiser {
    model: Denoiser,
    pending: Vec<f32>,
    /// one reusable frame: `run` used to build a fresh Vec for every shift
    chunk: Vec<f32>,
    /// model output for the current frame, reused as well
    scratch: Vec<f32>,
    shift: usize,
}

impl StreamDenoiser {
    pub fn new(model: Denoiser) -> Self {
        let shift = model.frame_shift.max(1) as usize;
        Self {
            model,
            pending: Vec::with_capacity(shift * 4),
            chunk: Vec::with_capacity(shift),
            scratch: Vec::with_capacity(shift),
            shift,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.model.sample_rate.max(1) as u32
    }

    pub fn frame_shift(&self) -> usize {
        self.shift
    }

    /// Feeds a block and appends everything the model is ready to give back.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.pending.extend_from_slice(input);

        while self.pending.len() >= self.shift {
            self.chunk.clear();
            self.chunk.extend_from_slice(&self.pending[..self.shift]);
            self.pending.drain(..self.shift);
            self.model.run_into(&self.chunk, &mut self.scratch);
            out.extend_from_slice(&self.scratch);
        }
    }

    /// Drops the model's streaming state (noise estimate, speaker lock) and
    /// whatever it was holding. Used when the chain starts over after a long
    /// silence: a stopped residual denoiser also has a stale noise estimate.
    pub fn reset(&mut self) {
        self.pending.clear();
        self.model.reset();
    }

    /// Emits the tail still held by the model (end of stream).
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        if !self.pending.is_empty() {
            let tail: Vec<f32> = std::mem::take(&mut self.pending);
            out.extend_from_slice(&self.model.run(&tail));
        }
        out.extend_from_slice(&self.model.flush());
    }
}

unsafe impl Send for Denoiser {}

impl Denoiser {
    pub fn new(enroll: &Path, chunk: &Path, voice_sample: Option<&Path>, threads: u16) -> Result<Self> {
        let api = api()?;

        let enroll_c = CString::new(crate::paths::native_safe(enroll).to_string_lossy().as_bytes())?;
        let chunk_c = CString::new(crate::paths::native_safe(chunk).to_string_lossy().as_bytes())?;
        let voice_c = voice_sample
            .filter(|p| p.exists())
            .map(|p| CString::new(crate::paths::native_safe(p).to_string_lossy().as_bytes()))
            .transpose()?;

        let config = OnlineConfig {
            model: ModelConfig {
                gtcrn: GtcrnConfig { model: ptr::null() },
                num_threads: threads.max(1) as i32,
                debug: 0,
                provider: ptr::null(),
                dpdfnet: DpdfNetConfig {
                    model: ptr::null(),
                    attenuation_limit_db: 0.0,
                },
                voxlock: VoxLockConfig {
                    enroll_model: enroll_c.as_ptr(),
                    chunk_model: chunk_c.as_ptr(),
                    voice_sample: voice_c.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
                },
            },
        };

        Self::create(api, config)
    }

    /// GTCRN: a very light speech enhancer (about 0.5 MB, 16 kHz).
    pub fn new_gtcrn(model: &Path, threads: u16) -> Result<Self> {
        let api = api()?;
        let model_c = CString::new(crate::paths::native_safe(model).to_string_lossy().as_bytes())?;

        let config = OnlineConfig {
            model: ModelConfig {
                gtcrn: GtcrnConfig { model: model_c.as_ptr() },
                num_threads: threads.max(1) as i32,
                debug: 0,
                provider: ptr::null(),
                dpdfnet: DpdfNetConfig {
                    model: ptr::null(),
                    attenuation_limit_db: 0.0,
                },
                voxlock: VoxLockConfig {
                    enroll_model: ptr::null(),
                    chunk_model: ptr::null(),
                    voice_sample: ptr::null(),
                },
            },
        };

        Self::create(api, config)
    }

    fn create(api: &Api, config: OnlineConfig) -> Result<Self> {
        let handle = unsafe { (api.create)(&config) };
        if handle.is_null() {
            return Err(anyhow!("OnlineSpeechDenoiser::create returned NULL"));
        }

        let sample_rate = unsafe { (api.sample_rate)(handle) };
        let frame_shift = unsafe { (api.frame_shift)(handle) };

        Ok(Self {
            handle,
            sample_rate,
            frame_shift,
        })
    }

    pub fn sample_rate(&self) -> i32 {
        self.sample_rate
    }

    pub fn frame_shift(&self) -> i32 {
        self.frame_shift
    }

    /// Feeds one chunk at the model sample rate; returns whatever is ready.
    /// Drops the model's streaming state (noise estimate, speaker lock).
    /// Used by the dropout guard: feed a short pre-roll afterwards so the
    /// denoiser re-locks to the speaker immediately instead of staying silent.
    pub fn reset(&mut self) {
        if let Ok(api) = api() {
            unsafe { (api.denoiser_reset)(self.handle) };
        }
    }

    pub fn run(&mut self, chunk: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        self.run_into(chunk, &mut out);
        out
    }

    /// Same as `run`, but writes into a caller buffer. The engine calls this
    /// once per block, and a fresh Vec each time was 60 KB/s of allocator churn.
    pub fn run_into(&mut self, chunk: &[f32], out: &mut Vec<f32>) {
        out.clear();
        let Ok(api) = api() else {
            return;
        };

        let ptr = unsafe {
            (api.run)(
                self.handle,
                chunk.as_ptr(),
                chunk.len() as i32,
                self.sample_rate,
            )
        };

        if ptr.is_null() {
            return;
        }

        unsafe {
            let audio = &*ptr;
            let slice = std::slice::from_raw_parts(audio.samples, audio.n.max(0) as usize);
            out.extend_from_slice(slice);
            (api.free)(ptr);
        }
    }

    pub fn flush(&mut self) -> Vec<f32> {
        let Ok(api) = api() else {
            return Vec::new();
        };

        let ptr = unsafe { (api.flush)(self.handle) };
        if ptr.is_null() {
            return Vec::new();
        }

        unsafe {
            let audio = &*ptr;
            let slice = std::slice::from_raw_parts(audio.samples, audio.n.max(0) as usize);
            let copy = slice.to_vec();
            (api.free)(ptr);
            copy
        }
    }
}

impl Drop for Denoiser {
    fn drop(&mut self) {
        if let Ok(api) = api() {
            unsafe { (api.destroy)(self.handle) };
        }
    }
}

/// Silero voice activity detector from the same sherpa-onnx library.
/// It tells whether the block contains speech, which is what the output gate
/// and the level control should follow instead of a plain level threshold.
pub struct Vad {
    handle: *const c_void,
    pending: Vec<f32>,
    /// reusable Silero window, one allocation for the lifetime of the session
    chunk: Vec<f32>,
    window: usize,
    last: bool,
}

unsafe impl Send for Vad {}

impl Vad {
    pub fn new(model: &Path, threshold: f32) -> Result<Self> {
        let api = api()?;
        let model_c = CString::new(crate::paths::native_safe(model).to_string_lossy().as_bytes())?;

        let config = VadModelConfig {
            silero_vad: SileroVadConfig {
                model: model_c.as_ptr(),
                threshold: threshold.clamp(0.05, 0.95),
                min_silence_duration: 0.5,
                min_speech_duration: 0.08,
                window_size: 512,
                max_speech_duration: 30.0,
            },
            sample_rate: 16_000,
            num_threads: 1,
            provider: ptr::null(),
            debug: 0,
            ten_vad: TenVadConfig {
                model: ptr::null(),
                threshold: 0.0,
                min_silence_duration: 0.0,
                min_speech_duration: 0.0,
                window_size: 0,
                max_speech_duration: 0.0,
            },
        };

        let handle = unsafe { (api.vad_create)(&config, 30.0) };
        if handle.is_null() {
            return Err(anyhow!("VoiceActivityDetector::create returned NULL"));
        }

        Ok(Self {
            handle,
            pending: Vec::with_capacity(1024),
            chunk: Vec::with_capacity(512),
            window: 512,
            last: false,
        })
    }

    /// Feeds a block at 16 kHz and returns the latest decision.
    pub fn push(&mut self, samples: &[f32]) -> bool {
        let Ok(api) = api() else {
            return false;
        };

        self.pending.extend_from_slice(samples);

        while self.pending.len() >= self.window {
            self.chunk.clear();
            self.chunk.extend_from_slice(&self.pending[..self.window]);
            self.pending.drain(..self.window);
            unsafe {
                (api.vad_accept)(self.handle, self.chunk.as_ptr(), self.chunk.len() as i32);
            }
            self.last = unsafe { (api.vad_detected)(self.handle) != 0 };
        }

        self.last
    }

    pub fn detected(&self) -> bool {
        self.last
    }

    pub fn reset(&mut self) {
        if let Ok(api) = api() {
            unsafe { (api.vad_reset)(self.handle) };
        }
        self.pending.clear();
        self.last = false;
    }
}

impl Drop for Vad {
    fn drop(&mut self) {
        if let Ok(api) = api() {
            unsafe { (api.vad_destroy)(self.handle) };
        }
    }
}
