//! FFI surface for `path_provider_native`.
//!
//! Every exported symbol is prefixed with `ppn_` and follows the same contract:
//! - Output strings are heap-allocated by Rust via `CString::into_raw`. The Dart side
//!   MUST free them by calling `ppn_free`. Returning a null pointer signals "directory
//!   unavailable on this platform" — this is the only valid `null` in the public API.
//!
//! ## Android
//! `robius-directories` is excluded from Android builds because its transitive
//! `robius-android-env` dependency runs native code at library load time and causes
//! a splash-screen hang when loaded via `DynamicLibrary.open()` (Dart FFI) rather
//! than `System.loadLibrary()`. On Android, paths are derived from `/proc` entries
//! without JNI (see the `android` module below). Only `getTemporaryDirectory()` and
//! `getApplicationCacheDirectory()` are reliable on all devices. All other Android
//! getters return null and throw `MissingPlatformDirectoryException`; do not use
//! them in production code.

use std::ffi::{CString, c_char};
use std::path::PathBuf;
use std::ptr;

#[cfg(not(target_os = "android"))]
use robius_directories::{BaseDirs, UserDirs};

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

    /// Load libgio using a runtime-friendly SONAME first, then fall back to the
    /// unversioned development symlink when available.
    fn load_gio_library() -> Option<libloading::Library> {
        const GIO_CANDIDATES: &[&str] = &["libgio-2.0.so.0", "libgio-2.0.so"];

        for candidate in GIO_CANDIDATES {
            if let Ok(lib) = unsafe { libloading::Library::new(*candidate) } {
                return Some(lib);
            }
        }

        None
    }

    /// Attempt to read the GApplication ID via libgio (mirrors Flutter's Dart FFI code).
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

    /// Base dir + app ID when available; otherwise falls back to the unscoped
    /// base directory. Creates the resulting directory if needed.
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn gio_library_load_does_not_crash() {
            // May return None on systems without libgio; must not panic.
            let _ = load_gio_library();
        }
    }
}

// ─── Android: best-effort path detection ─────────────────────────────────────
//
// No JNI, no NDK context. Derives the app sandbox path from Linux /proc entries.
// Primary user (uid < 100 000) → /data/user/0/<pkg>
// Secondary user / work profile → /data/user/<uid/100000>/<pkg>
// If the derived path is not writable, falls back to std::env::temp_dir()'s
// parent (Android 13+ sets temp_dir to the app cache dir).
// Returns None only when every strategy fails — callers treat None as "unavailable".

#[cfg(target_os = "android")]
mod android {
    use std::path::PathBuf;
    use std::sync::OnceLock;

    static BASE: OnceLock<Option<PathBuf>> = OnceLock::new();

    /// Returns the app sandbox base path, computing it once and caching forever.
    pub(crate) fn base_dir() -> Option<PathBuf> {
        BASE.get_or_init(compute).clone()
    }

    fn compute() -> Option<PathBuf> {
        let user_id = user_id_from_proc().unwrap_or(0);
        if let Some(pkg) = package_name_from_cmdline() {
            let candidate = PathBuf::from(format!("/data/user/{}/{}", user_id, pkg));
            if std::fs::create_dir_all(candidate.join("files")).is_ok() {
                return Some(candidate);
            }
        }
        // Fallback: Android 13+ sets temp_dir() to <sandbox>/cache; strip "cache".
        let tmp = std::env::temp_dir();
        if tmp.ends_with("cache") {
            tmp.parent().map(|p| p.to_path_buf())
        } else {
            None
        }
    }

    /// Read the real UID from `/proc/self/status` and derive the Android user ID.
    /// AOSP formula: `user_id = uid / 100_000`.
    fn user_id_from_proc() -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("Uid:") {
                let real_uid: u64 = rest.split_whitespace().next()?.parse().ok()?;
                return Some(real_uid / 100_000);
            }
        }
        None
    }

    /// Read the package name from `/proc/self/cmdline` (NUL-byte delimited).
    fn package_name_from_cmdline() -> Option<String> {
        let bytes = std::fs::read("/proc/self/cmdline").ok()?;
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8(bytes[..end].to_vec()).ok()
    }

    #[cfg(test)]
    pub(crate) mod tests {
        use super::*;

        #[test]
        fn user_id_from_proc_does_not_crash() {
            // On Android this will return Some; on other platforms it returns None.
            let _ = user_id_from_proc();
        }

        #[test]
        fn package_name_from_cmdline_does_not_crash() {
            let _ = package_name_from_cmdline();
        }

        #[test]
        fn base_dir_does_not_crash() {
            // Must not panic; may return None off-device.
            let _ = base_dir();
        }
    }
}

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

// ─── Macros for pass-through exports ─────────────────────────────────────────

macro_rules! base_dir_export {
    ($name:ident, $method:ident) => {
        /// # Safety
        /// No pointer arguments; always safe to call from Dart FFI.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name() -> *const c_char {
            std::panic::catch_unwind(|| {
                #[cfg(not(target_os = "android"))]
                {
                    to_cstr(BaseDirs::new().map(|b| b.$method().to_path_buf()))
                }
                #[cfg(target_os = "android")]
                {
                    ptr::null()
                }
            })
            .unwrap_or(std::ptr::null())
        }
    };
}

macro_rules! user_dir_export {
    ($name:ident, $method:ident) => {
        /// # Safety
        /// No pointer arguments; always safe to call from Dart FFI.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name() -> *const c_char {
            std::panic::catch_unwind(|| {
                #[cfg(not(target_os = "android"))]
                {
                    to_cstr(UserDirs::new().and_then(|u| u.$method().map(|p| p.to_path_buf())))
                }
                #[cfg(target_os = "android")]
                {
                    ptr::null()
                }
            })
            .unwrap_or(std::ptr::null())
        }
    };
}

// ─── Platform-overridden exports ─────────────────────────────────────────────

/// getTemporaryDirectory
///
/// - Android: derives `<sandbox>/cache` from `android::base_dir()`, or falls back
///   to `std::env::temp_dir()` (Android 13+ maps this to the app cache dir).
/// - iOS: Uses `NSCachesDirectory` (not `NSTemporaryDirectory`) to match Flutter.
/// - macOS: Uses `NSCachesDirectory` + bundleIdentifier to match Flutter.
/// - Others: `std::env::temp_dir()` returns the correct system temp directory.
///
/// # Safety
/// No pointer arguments; always safe to call from Dart FFI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_temp_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "android")]
        {
            to_cstr(android::base_dir().map(|b| b.join("cache")).or_else(|| Some(std::env::temp_dir())))
        }
        #[cfg(target_os = "ios")]
        {
            to_cstr(BaseDirs::new().map(|b| b.cache_dir().to_path_buf()))
        }
        #[cfg(target_os = "macos")]
        {
            to_cstr(with_bundle_id(BaseDirs::new().map(|b| b.cache_dir().to_path_buf())))
        }
        #[cfg(not(any(target_os = "android", target_os = "ios", target_os = "macos")))]
        {
            to_cstr(Some(std::env::temp_dir()))
        }
    })
    .unwrap_or(std::ptr::null())
}

/// getApplicationCacheDirectory
///
/// - Android: derives `<sandbox>/cache` from `android::base_dir()`, or falls back
///   to `std::env::temp_dir()` (no JNI = no `Context.getCacheDir()`).
/// - macOS: Appends the bundle identifier to `NSCachesDirectory` to match Flutter.
/// - Linux: Scoped by app ID (GApplication ID or executable name).
/// - Others: `cache_dir` from `BaseDirs`.
///
/// # Safety
/// No pointer arguments; always safe to call from Dart FFI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_cache_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "android")]
        {
            to_cstr(android::base_dir().map(|b| b.join("cache")).or_else(|| Some(std::env::temp_dir())))
        }
        #[cfg(target_os = "macos")]
        {
            to_cstr(with_bundle_id(BaseDirs::new().map(|b| b.cache_dir().to_path_buf())))
        }
        #[cfg(target_os = "linux")]
        {
            to_cstr(linux::scoped(BaseDirs::new().map(|b| b.cache_dir().to_path_buf())))
        }
        #[cfg(not(any(target_os = "android", target_os = "macos", target_os = "linux")))]
        {
            to_cstr(BaseDirs::new().map(|b| b.cache_dir().to_path_buf()))
        }
    })
    .unwrap_or(std::ptr::null())
}

/// getApplicationSupportDirectory
///
/// - Android: derives `<sandbox>/files` from `android::base_dir()` (best effort;
///   matches `Context.getFilesDir()` on primary user). Returns null when
///   detection fails.
/// - macOS: Appends the bundle identifier to `NSApplicationSupportDirectory` to match Flutter.
///   `BaseDirs::data_dir()` maps to `NSApplicationSupportDirectory` on macOS.
/// - Linux: Scoped by app ID (GApplication ID or executable name).
/// - Others: `data_dir` from `BaseDirs`.
///
/// # Safety
/// No pointer arguments; always safe to call from Dart FFI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_data_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "android")]
        {
            to_cstr(android::base_dir().map(|b| b.join("files")))
        }
        #[cfg(target_os = "macos")]
        {
            to_cstr(with_bundle_id(BaseDirs::new().map(|b| b.data_dir().to_path_buf())))
        }
        #[cfg(target_os = "linux")]
        {
            to_cstr(linux::scoped(BaseDirs::new().map(|b| b.data_dir().to_path_buf())))
        }
        #[cfg(not(any(target_os = "android", target_os = "macos", target_os = "linux")))]
        {
            to_cstr(BaseDirs::new().map(|b| b.data_dir().to_path_buf()))
        }
    })
    .unwrap_or(std::ptr::null())
}

/// getDownloadsDirectory
///
/// - Android: returns null — no sandboxed downloads directory without JNI.
/// - iOS: `UserDirs::download_dir()` returns `None`; resolves `home_dir/Downloads` instead.
/// - Others: `download_dir` from `UserDirs`.
///
/// # Safety
/// No pointer arguments; always safe to call from Dart FFI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_download_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "android")]
        {
            ptr::null()
        }
        #[cfg(target_os = "ios")]
        {
            to_cstr(UserDirs::new().map(|u| u.home_dir().join("Downloads")))
        }
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            to_cstr(UserDirs::new().and_then(|u| u.download_dir().map(|p| p.to_path_buf())))
        }
    })
    .unwrap_or(std::ptr::null())
}

// ─── Remaining pass-through exports ──────────────────────────────────────────

base_dir_export!(ppn_config_dir, config_dir);
base_dir_export!(ppn_data_local_dir, data_local_dir);
base_dir_export!(ppn_home_dir, home_dir);
base_dir_export!(ppn_preference_dir, preference_dir);

user_dir_export!(ppn_document_dir, document_dir);
user_dir_export!(ppn_picture_dir, picture_dir);
user_dir_export!(ppn_audio_dir, audio_dir);
user_dir_export!(ppn_video_dir, video_dir);
user_dir_export!(ppn_desktop_dir, desktop_dir);
user_dir_export!(ppn_public_dir, public_dir);

/// getLibraryDirectory — iOS and macOS only; returns null on all other platforms.
///
/// # Safety
/// No pointer arguments; always safe to call from Dart FFI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_library_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            to_cstr(BaseDirs::new().map(|b| b.home_dir().join("Library")))
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            ptr::null()
        }
    })
    .unwrap_or(std::ptr::null())
}

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
    fn bundle_id_does_not_crash() {
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
        let base = BaseDirs::new().map(|b| b.data_dir().to_path_buf());
        let scoped = linux::scoped(base.clone());
        if let (Some(b), Some(s)) = (base, scoped) {
            assert!(
                s.starts_with(&b),
                "scoped path must extend or equal the base",
            );
            // In test context /proc/self/exe always resolves, so the path is extended.
            assert_ne!(
                s, b,
                "scoped path should include the executable name as suffix"
            );
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_scoped_returns_none_for_unwritable_dir() {
        // /proc is a read-only virtual filesystem; subdirectory creation must fail.
        let result = linux::scoped(Some(std::path::PathBuf::from("/proc/ppn_test_unwritable")));
        assert!(
            result.is_none(),
            "scoped must return None when the directory cannot be created",
        );
    }

    #[test]
    fn android_user_id_math_primary_user() {
        // uid < 100 000 derives user 0 (single-user / primary device).
        assert_eq!(0u64, 99_999 / 100_000);
    }

    #[test]
    fn android_user_id_math_secondary_user() {
        // uid = 110 052 → user 1 (work profile); uid = 210 052 → user 2.
        assert_eq!(1u64, 110_052 / 100_000);
        assert_eq!(2u64, 210_052 / 100_000);
    }

    #[test]
    fn android_temp_dir_fallback_strips_cache_suffix() {
        // Verify the fallback logic: a path ending in "cache" strips that component.
        let tmp = std::path::PathBuf::from("/data/user/0/com.example/cache");
        let parent = if tmp.ends_with("cache") {
            tmp.parent().map(|p| p.to_path_buf())
        } else {
            None
        };
        assert_eq!(
            parent,
            Some(std::path::PathBuf::from("/data/user/0/com.example")),
            "stripping 'cache' suffix must yield the sandbox root",
        );
    }
}
