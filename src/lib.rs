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

// ─── macOS: bundle identifier helper ─────────────────────────────────────────

#[cfg(target_os = "macos")]
fn bundle_id() -> Option<String> {
    use objc2_foundation::NSBundle;
    let bundle = NSBundle::mainBundle();
    bundle.bundleIdentifier().map(|s| s.to_string())
}

#[cfg(target_os = "macos")]
fn with_bundle_id(base: Option<PathBuf>) -> Option<PathBuf> {
    let path = base?;
    let result = match bundle_id() {
        Some(id) => path.join(id),
        None => path,
    };
    std::fs::create_dir_all(&result).ok()?;
    Some(result)
}

// ─── Linux helpers ────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod linux {
    use std::path::PathBuf;
    use std::sync::OnceLock;

    static APP_ID: OnceLock<String> = OnceLock::new();

    fn load_gio_library() -> Option<libloading::Library> {
        const GIO_CANDIDATES: &[&str] = &["libgio-2.0.so.0", "libgio-2.0.so"];
        for candidate in GIO_CANDIDATES {
            if let Ok(lib) = unsafe { libloading::Library::new(*candidate) } {
                return Some(lib);
            }
        }
        None
    }

    fn gio_application_id() -> Option<String> {
        unsafe {
            let lib = load_gio_library()?;
            let get_default: libloading::Symbol<unsafe extern "C" fn() -> *mut std::ffi::c_void> =
                lib.get(b"g_application_get_default").ok()?;
            let app = get_default();
            if app.is_null() {
                return None;
            }
            let get_id: libloading::Symbol<
                unsafe extern "C" fn(*mut std::ffi::c_void) -> *const std::ffi::c_char,
            > = lib.get(b"g_application_get_application_id").ok()?;
            let id_ptr = get_id(app);
            if id_ptr.is_null() {
                return None;
            }
            std::ffi::CStr::from_ptr(id_ptr)
                .to_str()
                .ok()
                .map(String::from)
        }
    }

    fn executable_name() -> Option<String> {
        std::fs::read_link("/proc/self/exe")
            .ok()
            .and_then(|p| p.file_stem()?.to_str().map(String::from))
    }

    pub(crate) fn app_id() -> Option<String> {
        if let Some(id) = APP_ID.get() {
            return Some(id.clone());
        }
        let id = gio_application_id().or_else(executable_name)?;
        let _ = APP_ID.set(id.clone());
        Some(id)
    }

    pub(crate) fn scoped(base: Option<PathBuf>) -> Option<PathBuf> {
        let path = base?;
        let result = match app_id() {
            Some(id) => path.join(id),
            None => path,
        };
        if std::fs::create_dir_all(&result).is_err() {
            return None;
        }
        Some(result)
    }

    /// Parse a user directory from `~/.config/user-dirs.dirs` for the given XDG key
    /// (e.g. `"DOCUMENTS"`, `"DOWNLOAD"`). Mirrors what the `xdg` Dart package does
    /// via `getUserDirectory()`.
    ///
    /// The XDG env vars (`$XDG_DOCUMENTS_DIR` etc.) are only set during a full
    /// desktop login session. The file is always present and is the authoritative
    /// source, so we parse it directly as a fallback — same as Google's implementation.
    pub(crate) fn user_dir(key: &str) -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        let config_path = PathBuf::from(&home).join(".config/user-dirs.dirs");
        let content = std::fs::read_to_string(config_path).ok()?;
        let search = format!("XDG_{}_DIR=", key);
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix(&search) {
                let val = rest.trim_matches('"');
                let expanded = val.replace("$HOME", &home);
                if !expanded.is_empty() {
                    return Some(PathBuf::from(expanded));
                }
            }
        }
        None
    }

    #[cfg(test)]
    pub(crate) mod tests {
        use super::*;

        #[test]
        fn gio_library_load_does_not_crash() {
            let _ = load_gio_library();
        }

        /// user_dir must parse the file correctly when it exists.
        #[test]
        fn user_dir_documents_is_absolute_when_present() {
            if let Some(p) = user_dir("DOCUMENTS") {
                assert!(p.is_absolute(), "XDG DOCUMENTS must be absolute: {:?}", p);
            }
        }

        #[test]
        fn user_dir_download_is_absolute_when_present() {
            if let Some(p) = user_dir("DOWNLOAD") {
                assert!(p.is_absolute(), "XDG DOWNLOAD must be absolute: {:?}", p);
            }
        }

        /// Simulate missing file — must return None, not panic.
        #[test]
        fn user_dir_returns_none_for_unknown_key() {
            // Extremely unlikely to exist in any user-dirs.dirs.
            let result = user_dir("NONEXISTENT_KEY_XYZ");
            assert!(result.is_none());
        }
    }
}

// ─── Init (Android) ──────────────────────────────────────────────────────────

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_init_android(files_dir: *const c_char) {
    if files_dir.is_null() {
        return;
    }
    let Ok(path) = (unsafe { CStr::from_ptr(files_dir) }).to_str() else {
        return;
    };
    let path = path.to_owned();
    let _ = std::panic::catch_unwind(|| sysdirs::init_android(path.as_str()));
}

#[cfg(not(target_os = "android"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_init_android(_files_dir: *const c_char) {}

// ─── Free ────────────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(unsafe { CString::from_raw(ptr) });
    }
}

// ─── Macro for simple pass-through exports ───────────────────────────────────

macro_rules! dir_export {
    ($name:ident, $sysdirs_fn:ident) => {
        /// # Safety
        /// No pointer arguments; always safe to call from Dart FFI.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name() -> *const c_char {
            std::panic::catch_unwind(|| to_cstr(sysdirs::$sysdirs_fn())).unwrap_or(std::ptr::null())
        }
    };
}

// ─── Platform-overridden exports ─────────────────────────────────────────────

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_temp_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "ios")]
        { to_cstr(sysdirs::cache_dir()) }
        #[cfg(target_os = "macos")]
        { to_cstr(with_bundle_id(sysdirs::cache_dir())) }
        #[cfg(not(any(target_os = "ios", target_os = "macos")))]
        { to_cstr(sysdirs::temp_dir()) }
    })
    .unwrap_or(std::ptr::null())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_cache_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "macos")]
        { to_cstr(with_bundle_id(sysdirs::cache_dir())) }
        #[cfg(target_os = "linux")]
        { to_cstr(linux::scoped(sysdirs::cache_dir())) }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        { to_cstr(sysdirs::cache_dir()) }
    })
    .unwrap_or(std::ptr::null())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_data_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "macos")]
        { to_cstr(with_bundle_id(sysdirs::data_dir())) }
        #[cfg(target_os = "linux")]
        { to_cstr(linux::scoped(sysdirs::data_dir())) }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        { to_cstr(sysdirs::data_dir()) }
    })
    .unwrap_or(std::ptr::null())
}

/// getApplicationDocumentsDirectory
///
/// Linux fallback chain (matches Google's `xdg` Dart package behavior):
///   1. Parse `~/.config/user-dirs.dirs` for `XDG_DOCUMENTS_DIR` (primary — file
///      is always present regardless of session type).
///   2. Fall back to `sysdirs::document_dir()` which reads the env var
///      `$XDG_DOCUMENTS_DIR` (only set in full desktop login sessions).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_document_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "linux")]
        {
            // XDG file first (always available), then env var fallback.
            to_cstr(linux::user_dir("DOCUMENTS").or_else(sysdirs::document_dir))
        }
        #[cfg(not(target_os = "linux"))]
        {
            to_cstr(sysdirs::document_dir())
        }
    })
    .unwrap_or(std::ptr::null())
}

/// getDownloadsDirectory
///
/// Linux fallback chain:
///   1. `sysdirs::download_dir()` — reads `$XDG_DOWNLOAD_DIR` env var (current
///      approach, returns null when env var absent).
///   2. Fall back to parsing `~/.config/user-dirs.dirs` for `XDG_DOWNLOAD_DIR`
///      (same source Google's `xdg` Dart package uses).
///
/// iOS: `sysdirs::download_dir()` returns `None`; derive from home dir instead.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_download_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "linux")]
        {
            // Env var first (current approach), then XDG file fallback.
            to_cstr(sysdirs::download_dir().or_else(|| linux::user_dir("DOWNLOAD")))
        }
        #[cfg(target_os = "ios")]
        {
            to_cstr(sysdirs::home_dir().map(|h| h.join("Downloads")))
        }
        #[cfg(not(any(target_os = "linux", target_os = "ios")))]
        {
            to_cstr(sysdirs::download_dir())
        }
    })
    .unwrap_or(std::ptr::null())
}

// ─── Remaining pass-through exports ──────────────────────────────────────────

dir_export!(ppn_config_dir, config_dir);
dir_export!(ppn_data_local_dir, data_local_dir);
dir_export!(ppn_home_dir, home_dir);
dir_export!(ppn_picture_dir, picture_dir);
dir_export!(ppn_audio_dir, audio_dir);
dir_export!(ppn_video_dir, video_dir);
dir_export!(ppn_desktop_dir, desktop_dir);
dir_export!(ppn_public_dir, public_dir);
dir_export!(ppn_preference_dir, preference_dir);
dir_export!(ppn_library_dir, library_dir);

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    unsafe fn ptr_to_string(ptr: *const c_char) -> Option<String> {
        if ptr.is_null() {
            return None;
        }
        let s = unsafe { CStr::from_ptr(ptr) }
            .to_str()
            .expect("valid UTF-8")
            .to_owned();
        unsafe { ppn_free(ptr as *mut c_char) };
        Some(s)
    }

    #[test]
    fn free_accepts_null() {
        unsafe { ppn_free(ptr::null_mut()) };
    }

    #[test]
    fn roundtrip_through_free() {
        let ptr = unsafe { ppn_temp_dir() } as *mut c_char;
        if !ptr.is_null() {
            unsafe { ppn_free(ptr) };
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn bundle_id_does_not_crash() {
        let _ = bundle_id();
    }

    // ── Linux-specific tests ──────────────────────────────────────────────────

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_app_id_does_not_crash() {
        let id = linux::app_id();
        assert!(id.is_some(), "should at least resolve executable name");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_scoped_appends_id() {
        let base = sysdirs::data_dir();
        let scoped = linux::scoped(base.clone());
        if let (Some(b), Some(s)) = (base, scoped) {
            assert!(s.starts_with(&b), "scoped path must extend the base");
            assert_ne!(s, b, "scoped path must include executable name as suffix");
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_scoped_returns_none_for_unwritable_dir() {
        let result = linux::scoped(Some(std::path::PathBuf::from("/proc/ppn_test_unwritable")));
        assert!(result.is_none(), "scoped must return None for unwritable dir");
    }

    /// ppn_document_dir on Linux: XDG file is the primary source, so this must
    /// resolve on any desktop Linux with a home dir — even without a full login
    /// session (i.e. even when $XDG_DOCUMENTS_DIR env var is unset).
    #[test]
    #[cfg(target_os = "linux")]
    fn linux_document_dir_resolves_without_env_var() {
        // Unset the env var to simulate flutter test / headless CI.
        std::env::remove_var("XDG_DOCUMENTS_DIR");
        let ptr = unsafe { ppn_document_dir() };
        let path = unsafe { ptr_to_string(ptr) };
        assert!(
            path.is_some(),
            "ppn_document_dir must resolve via ~/.config/user-dirs.dirs when env var is absent"
        );
        assert!(
            std::path::Path::new(path.as_deref().unwrap()).is_absolute(),
            "resolved document dir must be absolute"
        );
    }

    /// ppn_download_dir on Linux: env var path (current approach) is tried first.
    /// When env var is absent, must fall back to XDG file.
    #[test]
    #[cfg(target_os = "linux")]
    fn linux_download_dir_falls_back_to_xdg_file_when_env_absent() {
        std::env::remove_var("XDG_DOWNLOAD_DIR");
        let ptr = unsafe { ppn_download_dir() };
        let path = unsafe { ptr_to_string(ptr) };
        assert!(
            path.is_some(),
            "ppn_download_dir must fall back to ~/.config/user-dirs.dirs when env var is absent"
        );
        assert!(
            std::path::Path::new(path.as_deref().unwrap()).is_absolute(),
            "resolved download dir must be absolute"
        );
    }

    /// When env var IS set, ppn_download_dir must prefer it over the file.
    #[test]
    #[cfg(target_os = "linux")]
    fn linux_download_dir_prefers_env_var() {
        std::env::set_var("XDG_DOWNLOAD_DIR", "/tmp/test_downloads");
        let ptr = unsafe { ppn_download_dir() };
        let path = unsafe { ptr_to_string(ptr) };
        std::env::remove_var("XDG_DOWNLOAD_DIR");
        assert_eq!(
            path.as_deref(),
            Some("/tmp/test_downloads"),
            "ppn_download_dir must return env var value when set"
        );
    }

    // Re-export linux submodule tests so they run with `cargo test`.
    #[cfg(target_os = "linux")]
    mod linux_inner {
        use super::super::linux::tests::*;
        // The #[test] fns in linux::tests are already annotated; they run automatically.
    }
}
