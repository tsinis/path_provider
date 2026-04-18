//! FFI surface for `path_provider_native`.
//!
//! Every exported symbol is prefixed with `ppn_` and follows the same contract:
//! - Input strings (Android init) are borrowed `*const c_char` — we copy, never retain.
//! - Output strings are heap-allocated by Rust via `CString::into_raw`. The Dart side
//!   MUST free them by calling `ppn_free`. Returning a null pointer signals "directory
//!   unavailable on this platform" — this is the only valid `null` in the public API.

use std::ffi::{CString, c_char};
use std::path::PathBuf;
use std::ptr;

#[cfg(target_os = "android")]
use std::ffi::CStr;

/// Convert an optional path to a heap-allocated C string. Returns `null` for `None`
/// (directory not available on this platform), for non-UTF-8 paths (rejected to avoid
/// silent data corruption), or when the path contains an interior NUL byte.
fn to_cstr(opt: Option<PathBuf>) -> *const c_char {
    let Some(path) = opt else { return ptr::null() };
    let Some(s) = path.to_str() else {
        return ptr::null();
    };
    match CString::new(s) {
        Ok(c) => c.into_raw() as *const c_char,
        Err(_) => ptr::null(),
    }
}

/// Initialize `sysdirs` on Android with the app's `filesDir` path. Idempotent.
///
/// # Safety
/// `files_dir` must be either null or a valid, NUL-terminated UTF-8 string. Called from
/// Dart via `package:jni` once, lazily, on first directory access.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_init_android(files_dir: *const c_char) {
    if files_dir.is_null() {
        return;
    }
    let Ok(path) = (unsafe { CStr::from_ptr(files_dir) }).to_str() else {
        return;
    };
    sysdirs::init_android(path);
}

/// No-op on non-Android targets so the symbol always exists.
///
/// # Safety
/// This is always safe on non-Android targets, as it is a no-op. On Android, see the
/// Android-specific variant for safety requirements.
#[cfg(not(target_os = "android"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_init_android(_files_dir: *const c_char) {}

/// Free a C string previously returned by any `ppn_*` getter. Null-safe.
///
/// # Safety
/// `ptr` must either be null or a pointer returned by one of the `ppn_*` getters in this
/// library. Double-free or freeing foreign memory is undefined operation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(unsafe { CString::from_raw(ptr) });
    }
}

macro_rules! dir_export {
    ($name:ident, $sysdirs_fn:ident) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name() -> *const c_char {
            to_cstr(sysdirs::$sysdirs_fn())
        }
    };
}

dir_export!(ppn_temp_dir, temp_dir);
dir_export!(ppn_cache_dir, cache_dir);
dir_export!(ppn_config_dir, config_dir);
dir_export!(ppn_data_dir, data_dir);
dir_export!(ppn_data_local_dir, data_local_dir);
dir_export!(ppn_home_dir, home_dir);
dir_export!(ppn_document_dir, document_dir);
dir_export!(ppn_download_dir, download_dir);
dir_export!(ppn_picture_dir, picture_dir);
dir_export!(ppn_audio_dir, audio_dir);
dir_export!(ppn_video_dir, video_dir);
dir_export!(ppn_desktop_dir, desktop_dir);
dir_export!(ppn_public_dir, public_dir);
dir_export!(ppn_preference_dir, preference_dir);
dir_export!(ppn_library_dir, library_dir);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_accepts_null() {
        unsafe { ppn_free(ptr::null_mut()) };
    }

    #[test]
    fn roundtrip_through_free() {
        let ptr = ppn_temp_dir() as *mut c_char;
        if !ptr.is_null() {
            unsafe { ppn_free(ptr) };
        }
    }
}
