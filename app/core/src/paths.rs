use std::path::{Path, PathBuf};

/// Directory of the running executable.
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Workspace root: the nearest ancestor that has `Cargo.toml` or `models`.
pub fn root_dir() -> PathBuf {
    let mut dir = exe_dir();
    for _ in 0..5 {
        if dir.join("Cargo.toml").is_file() || dir.join("models").is_dir() {
            return dir;
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => break,
        }
    }
    exe_dir()
}

pub fn native_dir() -> PathBuf {
    root_dir().join("native")
}

pub fn data_dir() -> PathBuf {
    let dir = root_dir().join("data");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn recordings_dir() -> PathBuf {
    let dir = data_dir().join("recordings");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn profiles_dir() -> PathBuf {
    let dir = data_dir().join("profiles");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// `models/voxlock` either inside the app folder or one level up (repo layout).
pub fn models_dir() -> PathBuf {
    let root = root_dir();
    let local = root.join("models").join("voxlock");
    if local.is_dir() {
        return local;
    }

    if let Some(parent) = root.parent() {
        let shared = parent.join("models").join("voxlock");
        if shared.is_dir() {
            return shared;
        }
    }

    local
}

pub fn enroll_model() -> PathBuf {
    models_dir().join("enroll.onnx")
}

pub fn chunk_model() -> PathBuf {
    models_dir().join("chunk_int8.onnx")
}

/// Second-stage denoisers live next to VoxLock: `models/denoise/`.
pub fn denoise_dir() -> PathBuf {
    models_dir()
        .parent()
        .map(|p| p.join("denoise"))
        .unwrap_or_else(|| PathBuf::from("models").join("denoise"))
}

pub fn gtcrn_model() -> PathBuf {
    denoise_dir().join("gtcrn.onnx")
}

/// DPDFNet: the aggressive alternative to GTCRN (same sherpa denoiser API).
///
/// The 4-block variant: the 8-block one is more aggressive on paper but costs
/// RTF 0.4 on this machine, which leaves no headroom next to VoxLock.
pub fn dpdfnet_model() -> PathBuf {
    denoise_dir().join("dpdfnet4.onnx")
}

pub fn vad_model() -> PathBuf {
    denoise_dir().join("silero_vad.onnx")
}

/// Optional bandwidth-extension model (`Air`). Lives next to the denoisers,
/// so a portable build only needs one extra folder.
pub fn bwe_model() -> PathBuf {
    denoise_dir()
        .parent()
        .map(|p| p.join("bwe"))
        .unwrap_or_else(|| PathBuf::from("models").join("bwe"))
        .join("novasr.onnx")
}

/// sherpa-onnx opens files with narrow CRT calls, so non-ASCII paths fail.
/// Returns the 8.3 short form when needed.
#[cfg(windows)]
pub fn native_safe(path: &Path) -> PathBuf {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    #[link(name = "kernel32")]
    extern "system" {
        fn GetShortPathNameW(long_path: *const u16, short_path: *mut u16, len: u32) -> u32;
    }

    let text = path.to_string_lossy();
    if text.is_ascii() {
        return path.to_path_buf();
    }

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut buffer = vec![0u16; 2048];
    let len = unsafe { GetShortPathNameW(wide.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32) };

    if len > 0 && (len as usize) < buffer.len() {
        let os = std::ffi::OsString::from_wide(&buffer[..len as usize]);
        let short = PathBuf::from(os);
        if short.to_string_lossy().is_ascii() {
            return short;
        }
    }

    path.to_path_buf()
}

#[cfg(not(windows))]
pub fn native_safe(path: &Path) -> PathBuf {
    path.to_path_buf()
}

/// Windows only: lets the loader find onnxruntime.dll that sits next to sherpa.
#[cfg(windows)]
pub fn set_native_search_dir(dir: &Path) {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    extern "system" {
        fn SetDllDirectoryW(path: *const u16) -> i32;
    }

    let wide: Vec<u16> = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        SetDllDirectoryW(wide.as_ptr());
    }
}

#[cfg(not(windows))]
pub fn set_native_search_dir(_dir: &Path) {}
