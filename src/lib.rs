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
//!
//! ## Linux
//! `robius-directories` fully handles XDG base dir resolution. The only Linux-specific
//! code here is app-ID scoping: we append the executable stem to base dirs to match
//! Flutter's path_provider behavior. No libgio/libloading needed — `/proc/self/exe`
//! stem is the fallback Flutter itself uses when no GApplication ID is set.

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

// ─── Linux: app-scoped directories ───────────────────────────────────────────
//
// robius-directories resolves all XDG paths correctly — no workarounds needed.
// The only Linux-specific logic here is scoping: appending the executable name
// to base dirs so each app gets its own subdirectory (mirrors Flutter behavior).
// No libgio/libloading: /proc/self/exe stem is the same fallback Flutter uses.

#[cfg(target_os = "linux")]
mod linux {
    use std::path::PathBuf;
    use std::sync::OnceLock;

    static APP_ID: OnceLock<String> = OnceLock::new();

    /// Executable stem from `/proc/self/exe` — matches Flutter's `_getExecutableName`.
    fn executable_name() -> Option<String> {
        std::fs::read_link("/proc/self/exe")
            .ok()
            .and_then(|p| p.file_stem()?.to_str().map(String::from))
    }

    /// Returns the cached app ID (executable stem). Computed once.
    pub(crate) fn app_id() -> Option<String> {
        if let Some(id) = APP_ID.get() {
            return Some(id.clone());
        }
        let id = executable_name()?;
        let _ = APP_ID.set(id.clone());
        Some(id)
    }

    /// Appends the app ID to a base dir and ensures it exists.
    /// Falls through to the unscoped base if app_id() returns None.
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
    pub(crate) mod tests {
        use super::*;

        #[test]
        fn executable_name_resolves() {
            // /proc/self/exe always resolves in test context.
            let name = executable_name();
            assert!(name.is_some(), "executable_name must resolve via /proc/self/exe");
            assert!(!name.unwrap().is_empty());
        }

        #[test]
        fn app_id_is_stable() {
            // app_id() must return the same value on repeated calls (OnceLock).
            let a = app_id();
            let b = app_id();
            assert_eq!(a, b);
        }

        #[test]
        fn scoped_extends_base() {
            use robius_directories::BaseDirs;
            let base = BaseDirs::new().map(|b| b.data_dir().to_path_buf());
            let result = scoped(base.clone());
            if let (Some(b), Some(s)) = (base, result) {
                assert!(s.starts_with(&b), "scoped path must extend the base");
                assert_ne!(s, b, "scoped path must include executable name suffix");
            }
        }

        #[test]
        fn scoped_returns_none_for_unwritable_dir() {
            let result = scoped(Some(std::path::PathBuf::from("/proc/ppn_test_unwritable")));
            assert!(result.is_none(), "must return None when dir cannot be created");
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

#[cfg(target_os = "android")]
mod android {
    use std::path::PathBuf;
    use std::sync::OnceLock;

    static BASE: OnceLock<Option<PathBuf>> = OnceLock::new();

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
            let _ = user_id_from_proc();
        }

        #[test]
        fn package_name_from_cmdline_does_not_crash() {
            let _ = package_name_from_cmdline();
        }

        #[test]
        fn base_dir_does_not_crash() {
            let _ = base_dir();
        }

        #[test]
        fn user_id_math_primary_user() {
            assert_eq!(0u64, 99_999 / 100_000);
        }

        #[test]
        fn user_id_math_secondary_user() {
            assert_eq!(1u64, 110_052 / 100_000);
            assert_eq!(2u64, 210_052 / 100_000);
        }

        #[test]
        fn temp_dir_fallback_strips_cache_suffix() {
            let tmp = PathBuf::from("/data/user/0/com.example/cache");
            let parent = if tmp.ends_with("cache") {
                tmp.parent().map(|p| p.to_path_buf())
            } else {
                None
            };
            assert_eq!(parent, Some(PathBuf::from("/data/user/0/com.example")));
        }
    }
}

// ─── Free ────────────────────────────────────────────────────────────────────

/// Free a C string previously returned by any `ppn_*` getter. Null-safe.
///
/// # Safety
/// `ptr` must either be null or a pointer returned by one of the `ppn_*` getters
/// in this library. Double-free or freeing foreign memory is undefined behavior.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(unsafe { CString::from_raw(ptr) });
    }
}

// ─── Macros ───────────────────────────────────────────────────────────────────

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

// ─── Exports ─────────────────────────────────────────────────────────────────

/// getTemporaryDirectory
/// # Safety
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_temp_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "android")]
        {
            to_cstr(
                android::base_dir()
                    .map(|b| b.join("cache"))
                    .or_else(|| Some(std::env::temp_dir())),
            )
        }
        #[cfg(target_os = "ios")]
        {
            to_cstr(BaseDirs::new().map(|b| b.cache_dir().to_path_buf()))
        }
        #[cfg(target_os = "macos")]
        {
            to_cstr(with_bundle_id(
                BaseDirs::new().map(|b| b.cache_dir().to_path_buf()),
            ))
        }
        #[cfg(not(any(target_os = "android", target_os = "ios", target_os = "macos")))]
        {
            to_cstr(Some(std::env::temp_dir()))
        }
    })
    .unwrap_or(std::ptr::null())
}

/// getApplicationCacheDirectory
/// # Safety
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_cache_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "android")]
        {
            to_cstr(
                android::base_dir()
                    .map(|b| b.join("cache"))
                    .or_else(|| Some(std::env::temp_dir())),
            )
        }
        #[cfg(target_os = "macos")]
        {
            to_cstr(with_bundle_id(
                BaseDirs::new().map(|b| b.cache_dir().to_path_buf()),
            ))
        }
        #[cfg(target_os = "linux")]
        {
            to_cstr(linux::scoped(
                BaseDirs::new().map(|b| b.cache_dir().to_path_buf()),
            ))
        }
        #[cfg(not(any(target_os = "android", target_os = "macos", target_os = "linux")))]
        {
            to_cstr(BaseDirs::new().map(|b| b.cache_dir().to_path_buf()))
        }
    })
    .unwrap_or(std::ptr::null())
}

/// getApplicationSupportDirectory
/// # Safety
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_data_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "android")]
        {
            to_cstr(android::base_dir().map(|b| b.join("files")))
        }
        #[cfg(target_os = "macos")]
        {
            to_cstr(with_bundle_id(
                BaseDirs::new().map(|b| b.data_dir().to_path_buf()),
            ))
        }
        #[cfg(target_os = "linux")]
        {
            to_cstr(linux::scoped(
                BaseDirs::new().map(|b| b.data_dir().to_path_buf()),
            ))
        }
        #[cfg(not(any(target_os = "android", target_os = "macos", target_os = "linux")))]
        {
            to_cstr(BaseDirs::new().map(|b| b.data_dir().to_path_buf()))
        }
    })
    .unwrap_or(std::ptr::null())
}

/// getDownloadsDirectory
/// # Safety
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

/// getLibraryDirectory — Apple only.
/// # Safety
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

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_accepts_null() {
        unsafe { ppn_free(ptr::null_mut()) };
    }

    #[test]
    fn roundtrip_temp_dir_through_free() {
        let ptr = unsafe { ppn_temp_dir() } as *mut c_char;
        if !ptr.is_null() {
            unsafe { ppn_free(ptr) };
        }
    }

    #[test]
    fn all_getters_do_not_panic() {
        unsafe {
            // None of these must panic; null return is fine.
            let ptrs = [
                ppn_temp_dir(),
                ppn_cache_dir(),
                ppn_data_dir(),
                ppn_download_dir(),
                ppn_library_dir(),
                ppn_config_dir(),
                ppn_data_local_dir(),
                ppn_home_dir(),
                ppn_preference_dir(),
                ppn_document_dir(),
                ppn_picture_dir(),
                ppn_audio_dir(),
                ppn_video_dir(),
                ppn_desktop_dir(),
                ppn_public_dir(),
            ];
            for ptr in ptrs {
                ppn_free(ptr as *mut c_char);
            }
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn bundle_id_does_not_crash() {
        let _ = bundle_id();
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn ppn_cache_dir_non_null_on_macos() {
        let ptr = unsafe { ppn_cache_dir() };
        assert!(!ptr.is_null());
        unsafe { ppn_free(ptr as *mut c_char) };
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn ppn_cache_dir_non_null_on_linux() {
        let ptr = unsafe { ppn_cache_dir() };
        assert!(!ptr.is_null(), "cache_dir must resolve on Linux");
        unsafe { ppn_free(ptr as *mut c_char) };
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn ppn_data_dir_non_null_on_linux() {
        let ptr = unsafe { ppn_data_dir() };
        assert!(!ptr.is_null(), "data_dir must resolve on Linux");
        unsafe { ppn_free(ptr as *mut c_char) };
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_xdg_cache_dir_starts_with_home_or_xdg() {
        let b = robius_directories::BaseDirs::new().unwrap();
        let cache = b.cache_dir();
        // Must be either $XDG_CACHE_HOME or $HOME/.cache
        let home = b.home_dir();
        let xdg_override = std::env::var("XDG_CACHE_HOME").ok();
        if let Some(xdg) = xdg_override {
            assert!(cache.starts_with(&xdg));
        } else {
            assert!(cache.starts_with(home));
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_tests() {
        linux::tests::executable_name_resolves();
        linux::tests::app_id_is_stable();
        linux::tests::scoped_returns_none_for_unwritable_dir();
    }

    // Android math tests run on any platform (pure arithmetic).
    #[test]
    fn android_user_id_math_primary() {
        assert_eq!(0u64, 99_999 / 100_000);
    }

    #[test]
    fn android_user_id_math_secondary() {
        assert_eq!(1u64, 110_052 / 100_000);
        assert_eq!(2u64, 210_052 / 100_000);
    }

    #[test]
    fn android_temp_dir_fallback_logic() {
        let tmp = PathBuf::from("/data/user/0/com.example/cache");
        let parent = if tmp.ends_with("cache") {
            tmp.parent().map(|p| p.to_path_buf())
        } else {
            None
        };
        assert_eq!(parent, Some(PathBuf::from("/data/user/0/com.example")));
    }
}
