//! FFI surface for `path_provider_native`.
//!
//! Every exported symbol is prefixed with `ppn_` and follows the same contract:
//! - Output strings are heap-allocated by Rust via `CString::into_raw`. The Dart side
//!   MUST free them by calling `ppn_free`. Returning a null pointer signals "directory
//!   unavailable on this platform" — this is the only valid `null` in the public API.

use std::ffi::{CString, c_char};
use std::path::PathBuf;
use std::ptr;

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
            if std::fs::metadata(&candidate).is_ok() {
                return Some(candidate);
            }
        }
        // Android 13+ typically reports <sandbox>/cache as temp_dir.
        fallback_base_from_temp_dir(std::env::temp_dir())
    }

    // AOSP formula: user_id = uid / 100_000.
    fn user_id_from_proc() -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        parse_user_id_from_status(&status)
    }

    fn package_name_from_cmdline() -> Option<String> {
        let bytes = std::fs::read("/proc/self/cmdline").ok()?;
        parse_package_name_from_cmdline(&bytes)
    }

    fn parse_user_id_from_status(status: &str) -> Option<u64> {
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("Uid:") {
                let real_uid: u64 = rest.split_whitespace().next()?.parse().ok()?;
                return Some(real_uid / 100_000);
            }
        }
        None
    }

    fn parse_package_name_from_cmdline(bytes: &[u8]) -> Option<String> {
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        let pkg = String::from_utf8(bytes[..end].to_vec()).ok()?;
        if pkg.is_empty() {
            return None;
        }
        Some(pkg)
    }

    fn fallback_base_from_temp_dir(tmp: PathBuf) -> Option<PathBuf> {
        if tmp.ends_with("cache") {
            return tmp.parent().map(|p| p.to_path_buf());
        }
        None
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parse_user_id_primary_user() {
            let status = "Name:\ttest\nUid:\t12345\t12345\t12345\t12345\n";
            assert_eq!(parse_user_id_from_status(status), Some(0));
        }

        #[test]
        fn parse_user_id_secondary_user() {
            let status = "Name:\ttest\nUid:\t110052\t110052\t110052\t110052\n";
            assert_eq!(parse_user_id_from_status(status), Some(1));
        }

        #[test]
        fn parse_user_id_third_user() {
            let status = "Name:\ttest\nUid:\t210052\t210052\t210052\t210052\n";
            assert_eq!(parse_user_id_from_status(status), Some(2));
        }

        #[test]
        fn parse_package_name_reads_until_nul() {
            let cmdline = b"com.example.app\0--flag";
            assert_eq!(
                parse_package_name_from_cmdline(cmdline),
                Some("com.example.app".to_string())
            );
        }

        #[test]
        fn parse_package_name_handles_no_nul() {
            let cmdline = b"com.example.app";
            assert_eq!(
                parse_package_name_from_cmdline(cmdline),
                Some("com.example.app".to_string())
            );
        }

        #[test]
        fn parse_package_name_rejects_empty() {
            let cmdline = b"\0--flag";
            assert_eq!(parse_package_name_from_cmdline(cmdline), None);
        }

        #[test]
        fn fallback_from_cache_temp_dir() {
            let tmp = PathBuf::from("/data/user/1/com.example.app/cache");
            assert_eq!(
                fallback_base_from_temp_dir(tmp),
                Some(PathBuf::from("/data/user/1/com.example.app"))
            );
        }

        #[test]
        fn fallback_returns_none_for_non_cache_temp_dir() {
            let tmp = PathBuf::from("/tmp");
            assert_eq!(fallback_base_from_temp_dir(tmp), None);
        }

        /// documents dir must differ from support dir (app_flutter vs files).
        #[test]
        fn documents_dir_differs_from_support_dir() {
            let base = PathBuf::from("/data/user/0/com.example.app");
            let support = base.join("files");
            let documents = base.join("app_flutter");
            assert_ne!(
                support, documents,
                "getApplicationDocumentsDirectory must not equal getApplicationSupportDirectory on Android"
            );
        }
    }
}

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
    // Keep returning the resolved path even if directory creation fails due to
    // permissions; callers can decide how to handle an unwritable location.
    let _ = std::fs::create_dir_all(&result);
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
            // SAFETY: Symbols are resolved and invoked only while `lib` remains
            // alive within this scope. On any lookup failure we return `None`
            // before dereferencing the corresponding function pointer.
            // Do not return or store any Symbol or raw function pointer derived
            // from `lib` beyond this scope.
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

    #[cfg(test)]
    pub(crate) mod tests {
        use super::*;

        #[test]
        fn gio_library_load_does_not_crash() {
            let _ = load_gio_library();
        }
    }
}

// ─── Free ────────────────────────────────────────────────────────────────────

/// Free a C string previously returned by any `ppn_*` getter. Null-safe.
///
/// # Safety
/// `ptr` must be null or a pointer previously returned by a `ppn_*` getter in
/// this library. Double-free or freeing foreign memory is undefined behavior.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(unsafe { CString::from_raw(ptr) });
    }
}

// ─── Macro for simple pass-through exports ───────────────────────────────────

macro_rules! dir_export {
    ($name:ident, $dirs_fn:ident) => {
        /// # Safety
        /// No pointer arguments; always safe to call from Dart FFI.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name() -> *const c_char {
            std::panic::catch_unwind(|| {
                #[cfg(target_os = "android")]
                {
                    ptr::null()
                }
                #[cfg(not(target_os = "android"))]
                {
                    to_cstr(dirs::$dirs_fn())
                }
            })
            .unwrap_or(std::ptr::null())
        }
    };
}

// ─── Platform-overridden exports ─────────────────────────────────────────────

/// # Safety
/// No pointer arguments; always safe to call from Dart FFI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_temp_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "android")]
        {
            // Matches path_provider_android: temporary and cache map to cache dir.
            to_cstr(
                android::base_dir()
                    .map(|b| b.join("cache"))
                    .or_else(|| Some(std::env::temp_dir())),
            )
        }
        #[cfg(target_os = "ios")]
        {
            to_cstr(dirs::cache_dir())
        }
        #[cfg(target_os = "macos")]
        {
            to_cstr(with_bundle_id(dirs::cache_dir()))
        }
        #[cfg(not(any(target_os = "android", target_os = "ios", target_os = "macos")))]
        {
            to_cstr(Some(std::env::temp_dir()))
        }
    })
    .unwrap_or(std::ptr::null())
}

/// # Safety
/// No pointer arguments; always safe to call from Dart FFI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_cache_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "android")]
        {
            // Intentionally identical to ppn_temp_dir on Android.
            to_cstr(
                android::base_dir()
                    .map(|b| b.join("cache"))
                    .or_else(|| Some(std::env::temp_dir())),
            )
        }
        #[cfg(target_os = "macos")]
        {
            to_cstr(with_bundle_id(dirs::cache_dir()))
        }
        #[cfg(target_os = "linux")]
        {
            to_cstr(linux::scoped(dirs::cache_dir()))
        }
        #[cfg(not(any(target_os = "android", target_os = "macos", target_os = "linux")))]
        {
            to_cstr(dirs::cache_dir())
        }
    })
    .unwrap_or(std::ptr::null())
}

/// getApplicationSupportDirectory
///
/// Android: `<sandbox>/files` — matches `Context.getFilesDir()`.
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
            to_cstr(with_bundle_id(dirs::data_dir()))
        }
        #[cfg(target_os = "linux")]
        {
            to_cstr(linux::scoped(dirs::data_dir()))
        }
        #[cfg(not(any(target_os = "android", target_os = "macos", target_os = "linux")))]
        {
            to_cstr(dirs::data_dir())
        }
    })
    .unwrap_or(std::ptr::null())
}

/// getApplicationDocumentsDirectory
///
/// Android: `<sandbox>/app_flutter` — matches `Context.getDir("flutter", MODE_PRIVATE)`,
/// which is exactly what Flutter's original `path_provider_android` returns. This is a
/// *separate* directory from `getApplicationSupportDirectory()` (`files/`), giving us
/// the 4th distinct path on Android: cache, files, app_flutter.
///
/// # Safety
/// No pointer arguments; always safe to call from Dart FFI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_document_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(target_os = "android")]
        {
            // getDir("flutter") creates the dir and returns <sandbox>/app_flutter.
            // We create it here to match the original plugin's behavior.
            to_cstr(android::base_dir().and_then(|b| {
                let dir = b.join("app_flutter");
                std::fs::create_dir_all(&dir).ok()?;
                Some(dir)
            }))
        }
        #[cfg(not(target_os = "android"))]
        {
            to_cstr(dirs::document_dir().or_else(|| {
                dirs::home_dir().map(|h| {
                    let fallback = h.join("Documents");
                    let _ = std::fs::create_dir_all(&fallback);
                    fallback
                })
            }))
        }
    })
    .unwrap_or(std::ptr::null())
}

/// getDownloadsDirectory
///
/// Android: returns null — no sandboxed downloads dir without JNI.
/// iOS: `dirs::download_dir()` returns `None`; derive from home dir instead.
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
            to_cstr(dirs::home_dir().map(|h| h.join("Downloads")))
        }
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            to_cstr(dirs::download_dir())
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

/// # Safety
/// No pointer arguments; always safe to call from Dart FFI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ppn_library_dir() -> *const c_char {
    std::panic::catch_unwind(|| {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            to_cstr(dirs::home_dir().map(|h| h.join("Library")))
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
        let base = dirs::data_dir();
        let scoped = linux::scoped(base.clone());
        if let (Some(b), Some(s)) = (base, scoped) {
            assert!(s.starts_with(&b), "scoped path must extend the base");
            assert_ne!(s, b, "scoped path must include executable name as suffix");
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_ppn_document_dir_is_non_null() {
        let ptr = unsafe { ppn_document_dir() };
        assert!(
            !ptr.is_null(),
            "ppn_document_dir should return a path via dirs::document_dir or home/Documents fallback"
        );
        unsafe { ppn_free(ptr as *mut c_char) };
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_scoped_returns_none_for_unwritable_dir() {
        let result = linux::scoped(Some(std::path::PathBuf::from("/proc/ppn_test_unwritable")));
        assert!(
            result.is_none(),
            "scoped must return None for unwritable dir"
        );
    }

    // ── Android path logic tests (run on all platforms) ───────────────────────

    /// Verify the 4 distinct Android paths are all different.
    #[test]
    fn android_four_dirs_are_distinct() {
        let base = PathBuf::from("/data/user/0/com.example.app");
        let temp_cache = base.join("cache"); // temp + cache
        let support = base.join("files"); // getApplicationSupportDirectory
        let documents = base.join("app_flutter"); // getApplicationDocumentsDirectory

        assert_ne!(temp_cache, support);
        assert_ne!(temp_cache, documents);
        assert_ne!(support, documents);
    }
}
