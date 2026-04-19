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

/// On macOS, Flutter appends `NSBundle.mainBundle.bundleIdentifier` to
/// `NSCachesDirectory` and `NSApplicationSupportDirectory`. We replicate that
/// behavior here.
#[cfg(target_os = "macos")]
fn bundle_id() -> Option<String> {
    use objc2_foundation::NSBundle;
    let bundle = NSBundle::mainBundle();
    bundle.bundleIdentifier().map(|s| s.to_string())
}

/// Append the bundle identifier to a base path (macOS only). Returns the base
/// path unchanged when the bundle ID is unavailable (e.g. CLI tools).
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

// ─── Linux: app-scoped directories via libgio ────────────────────────────────

#[cfg(target_os = "linux")]
mod linux {
    use std::path::PathBuf;
    use std::sync::OnceLock;

    static APP_ID: OnceLock<String> = OnceLock::new();

    /// Attempt to read the GApplication ID via libgio (mirrors Flutter's Dart FFI code).
    fn gio_application_id() -> Option<String> {
        unsafe {
            let lib = libloading::Library::new("libgio-2.0.so").ok()?;

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

            // Borrowed pointer from GLib — do not free.
            std::ffi::CStr::from_ptr(id_ptr)
                .to_str()
                .ok()
                .map(String::from)
        }
    }

    /// Executable name fallback (matches Flutter's `_getExecutableName`).
    fn executable_name() -> Option<String> {
        std::fs::read_link("/proc/self/exe")
            .ok()
            .and_then(|p| p.file_stem()?.to_str().map(String::from))
    }

    /// Returns the app ID: GApplication ID → executable name fallback.
    pub(crate) fn app_id() -> Option<String> {
        if let Some(id) = APP_ID.get() {
            return Some(id.clone());
        }
        let id = gio_application_id().or_else(executable_name)?;
        let _ = APP_ID.set(id.clone());
        Some(id)
    }

    /// Base dir + app ID. Creates the directory if needed.
    pub(crate) fn scoped(base: Option<PathBuf>) -> Option<PathBuf> {
        let path = base?;
        let id = app_id()?;
        let result = path.join(id);
        std::fs::create_dir_all(&result).ok();
        Some(result)
    }
}

// ─── Init (Android) ──────────────────────────────────────────────────────────

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
    let path = path.to_owned();
    let _ = std::panic::catch_unwind(|| sysdirs::init_android(path.as_str()));
}

/// No-op on non-Android targets so the symbol always exists.
///
/// # Safety
/// This is always safe on non-Android targets, as it is a no-op. On Android, see the
/// Android-specific variant for safety requirements.
#[cfg(not(target_os = "android"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_init_android(_files_dir: *const c_char) {}

// ─── Free ────────────────────────────────────────────────────────────────────

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

/// getTemporaryDirectory
///
/// - iOS: Flutter uses `NSCachesDirectory` (not `NSTemporaryDirectory` / `<sandbox>/tmp`).
/// - macOS: Flutter uses `NSCachesDirectory` + bundleIdentifier.
/// - Others: `sysdirs::temp_dir()` already returns the correct value.
///
/// # Safety
/// No pointer arguments; always safe to call from Dart FFI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_temp_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "ios")]
        {
            to_cstr(sysdirs::cache_dir())
        }
        #[cfg(target_os = "macos")]
        {
            to_cstr(with_bundle_id(sysdirs::cache_dir()))
        }
        #[cfg(not(any(target_os = "ios", target_os = "macos")))]
        {
            to_cstr(sysdirs::temp_dir())
        }
    })
    .unwrap_or(std::ptr::null())
}

/// getApplicationCacheDirectory
///
/// - macOS: Flutter appends the bundle identifier to `NSCachesDirectory`.
/// - Others: `sysdirs::cache_dir()` is correct as-is.
///
/// # Safety
/// No pointer arguments; always safe to call from Dart FFI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_cache_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "macos")]
        {
            to_cstr(with_bundle_id(sysdirs::cache_dir()))
        }
        #[cfg(target_os = "linux")]
        {
            to_cstr(linux::scoped(sysdirs::cache_dir()))
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            to_cstr(sysdirs::cache_dir())
        }
    })
    .unwrap_or(std::ptr::null())
}

/// getApplicationSupportDirectory
///
/// - macOS: Flutter appends the bundle identifier to `NSApplicationSupportDirectory`.
///   `sysdirs::data_dir()` maps to `NSApplicationSupportDirectory` on macOS.
/// - Others: `sysdirs::data_dir()` is correct as-is.
///
/// # Safety
/// No pointer arguments; always safe to call from Dart FFI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_data_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "macos")]
        {
            to_cstr(with_bundle_id(sysdirs::data_dir()))
        }
        #[cfg(target_os = "linux")]
        {
            to_cstr(linux::scoped(sysdirs::data_dir()))
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            to_cstr(sysdirs::data_dir())
        }
    })
    .unwrap_or(std::ptr::null())
}

/// getDownloadsDirectory
///
/// - iOS: `sysdirs::download_dir()` returns `None`. Flutter resolves
///   `NSDownloadsDirectory` → `<sandbox>/Downloads`.
/// - Others: `sysdirs::download_dir()` is correct.
///
/// # Safety
/// No pointer arguments; always safe to call from Dart FFI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_download_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "ios")]
        {
            to_cstr(sysdirs::home_dir().map(|h| h.join("Downloads")))
        }
        #[cfg(not(target_os = "ios"))]
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
dir_export!(ppn_document_dir, document_dir);
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
    fn bundle_id_returns_something() {
        // In a test binary there may not be a bundle ID, so just verify it doesn't crash.
        let _ = bundle_id();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_app_id_does_not_crash() {
        // In test context GApplication won't exist — should fall back to executable name.
        let id = linux::app_id();
        assert!(id.is_some(), "should at least resolve executable name");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_scoped_appends_id() {
        let base = sysdirs::data_dir();
        let scoped = linux::scoped(base.clone());
        if let (Some(b), Some(s)) = (base, scoped) {
            assert!(s.starts_with(&b), "scoped path should extend base");
            assert_ne!(s, b, "scoped path should differ from base");
        }
    }
}
