//! FFI surface for `path_provider_dart`.
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
        super::fallback_base_from_temp_dir(std::env::temp_dir())
    }

    // AOSP formula: user_id = uid / 100_000.
    fn user_id_from_proc() -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        super::parse_user_id_from_status(&status)
    }

    fn package_name_from_cmdline() -> Option<String> {
        let bytes = std::fs::read("/proc/self/cmdline").ok()?;
        super::parse_package_name_from_cmdline(&bytes)
    }
}

// Pure parsing helpers used by the Android module at runtime and by tests on all platforms.
#[cfg(any(target_os = "android", test))]
fn parse_user_id_from_status(status: &str) -> Option<u64> {
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            let real_uid: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(real_uid / 100_000);
        }
    }
    None
}

#[cfg(any(target_os = "android", test))]
fn parse_package_name_from_cmdline(bytes: &[u8]) -> Option<String> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let pkg = String::from_utf8(bytes[..end].to_vec()).ok()?;
    if pkg.is_empty() {
        return None;
    }
    Some(pkg)
}

#[cfg(any(target_os = "android", test))]
fn fallback_base_from_temp_dir(tmp: PathBuf) -> Option<PathBuf> {
    if tmp.ends_with("cache") {
        return tmp.parent().map(|p| p.to_path_buf());
    }
    None
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
        // Attempt eager creation; return the path regardless so callers can
        // decide how to handle an unwritable location (consistent with macOS).
        let _ = std::fs::create_dir_all(&result);
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

// ─── Windows helpers ─────────────────────────────────────────────────────────

/// Sanitize a string for use as a Windows path component.
/// Replaces illegal characters, enforces 255 UTF-16 code units (NTFS limit),
/// and prefixes Windows reserved device names with `_`.
#[cfg(any(target_os = "windows", test))]
fn sanitize(s: String) -> String {
    const ILLEGAL: &str = "<>:\"/\\|?*";
    // Replace path-illegal characters and ASCII control characters.
    let s: String = s
        .chars()
        .map(|c| {
            if c.is_control() || ILLEGAL.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Enforce 255 UTF-16 code units (Windows NTFS per-component limit).
    // BMP chars cost 1 unit; supplementary-plane chars (emoji, rare scripts) cost 2.
    // Iterating over `char` values never splits a surrogate pair.
    let s: String = s
        .trim_end_matches(|c: char| c == '.' || c.is_whitespace())
        .chars()
        .scan(0usize, |units, c| {
            let next = *units + c.len_utf16();
            if next > 255 {
                None
            } else {
                *units = next;
                Some(c)
            }
        })
        .collect();
    // Windows reserved device names must never be used as file/dir components.
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.iter().any(|&r| s.eq_ignore_ascii_case(r)) {
        return format!("_{s}");
    }
    s
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::sanitize;
    use std::path::PathBuf;

    /// Returns `"CompanyName\ProductName"` read from the running exe's version
    /// resource, falling back to just the exe filename stem when unavailable.
    pub(crate) fn app_subfolder() -> String {
        version_info_subfolder().unwrap_or_else(|| exe_stem().unwrap_or_else(|| "App".to_string()))
    }

    fn version_info_subfolder() -> Option<String> {
        use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW};
        use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
        use windows::core::PCWSTR;

        unsafe {
            // Use 261 to always have a null slot beyond the 260-char write window.
            // If the path fills all 260 slots it was truncated — reject it.
            let mut buf = [0u16; 261];
            let len = GetModuleFileNameW(None, &mut buf[..260]);
            if len == 0 || len >= 260 {
                return None;
            }
            // buf[len] is 0 from zero-initialization, so PCWSTR is safe.
            let exe_path = PCWSTR(buf.as_ptr());

            let mut dummy = 0u32;
            let size = GetFileVersionInfoSizeW(exe_path, Some(&mut dummy));
            if size == 0 {
                return None;
            }

            let mut info = vec![0u8; size as usize];
            GetFileVersionInfoW(exe_path, Some(0), size, info.as_mut_ptr() as *mut _).ok()?;

            let product = sanitize(query_string_value(&info, "ProductName")?);
            if product.is_empty() {
                return None;
            }
            let company = query_string_value(&info, "CompanyName")
                .map(sanitize)
                .filter(|s| !s.is_empty());

            Some(match company {
                Some(c) => format!("{}\\{}", c, product),
                None => product,
            })
        }
    }

    fn query_string_value(info: &[u8], key: &str) -> Option<String> {
        use windows::Win32::Storage::FileSystem::VerQueryValueW;
        use windows::core::PCWSTR;

        // Read the available language+codepage pairs from the version resource.
        // Each pair is a u32 encoded as (language: u16, codepage: u16).
        let translations: Vec<String> = {
            let sub_block: Vec<u16> = "\\VarFileInfo\\Translation\0".encode_utf16().collect();
            let mut ptr = std::ptr::null_mut::<std::ffi::c_void>();
            let mut len = 0u32;
            let found = unsafe {
                VerQueryValueW(
                    info.as_ptr() as *const _,
                    PCWSTR(sub_block.as_ptr()),
                    &mut ptr,
                    &mut len,
                )
                .as_bool()
            };
            if found && len >= 4 && !ptr.is_null() {
                let count = len as usize / 4;
                unsafe {
                    std::slice::from_raw_parts(ptr as *const u32, count)
                        .iter()
                        .map(|&pair| {
                            // Win32 stores pairs as LOWORD=language, HIWORD=codepage.
                            let lang = (pair & 0xFFFF) as u16;
                            let cp = (pair >> 16) as u16;
                            format!("{:04x}{:04x}", lang, cp)
                        })
                        .collect()
                }
            } else {
                Vec::new()
            }
        };

        // Iterate discovered translations, then fall back to common en-US encodings.
        let fallback = &["04090000", "040904e4", "040904b0"];
        let candidates: Vec<&str> = translations
            .iter()
            .map(String::as_str)
            .chain(fallback.iter().copied())
            .collect();

        for enc in candidates {
            let sub_block: Vec<u16> = format!("\\StringFileInfo\\{}\\{}\0", enc, key)
                .encode_utf16()
                .collect();
            let mut ptr = std::ptr::null_mut::<std::ffi::c_void>();
            let mut len = 0u32;
            let found = unsafe {
                VerQueryValueW(
                    info.as_ptr() as *const _,
                    PCWSTR(sub_block.as_ptr()),
                    &mut ptr,
                    &mut len,
                )
                .as_bool()
            };
            if found && len > 0 && !ptr.is_null() {
                let s = unsafe {
                    let slice = std::slice::from_raw_parts(ptr as *const u16, len as usize);
                    let end = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
                    String::from_utf16(&slice[..end]).ok()
                };
                if s.is_some() {
                    return s;
                }
            }
        }
        None
    }

    fn exe_stem() -> Option<String> {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.file_stem()?.to_str().map(String::from))
    }

    /// Base dir + app subfolder. Creates the directory eagerly (matches macOS behavior).
    pub(crate) fn scoped(base: Option<PathBuf>) -> Option<PathBuf> {
        let path = base?;
        let result = path.join(app_subfolder());
        let _ = std::fs::create_dir_all(&result);
        Some(result)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn exe_stem_is_non_empty() {
            assert!(exe_stem().is_some_and(|s| !s.is_empty()));
        }

        #[test]
        fn app_subfolder_is_non_empty() {
            // Under flutter_tester.exe there is no version resource, so this
            // falls back to the exe stem — still must be non-empty.
            assert!(!app_subfolder().is_empty());
        }

        #[test]
        fn scoped_extends_base() {
            if let Some(base) = dirs::cache_dir() {
                let result = scoped(Some(base.clone()));
                assert!(result.is_some(), "scoped must return Some for a valid base");
                let s = result.unwrap();
                assert!(s.starts_with(&base), "scoped path must extend the base");
                assert_ne!(s, base, "scoped path must include the app subfolder");
            }
        }
    }
}

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
        #[cfg(target_os = "windows")]
        {
            // std::env::temp_dir() appends a trailing separator on Windows;
            // strip it to match what path_provider_windows returns, but never
            // strip the root separator of a bare drive (e.g. "C:\").
            let tmp = std::env::temp_dir();
            let Some(s) = tmp.to_str() else {
                return to_cstr(None);
            };
            // A bare drive root like "C:\" must keep its backslash; trim only
            // when doing so would not leave a naked drive letter.
            let is_drive_root = s.len() == 3 && s.ends_with(":\\");
            let s = if is_drive_root {
                s
            } else {
                s.trim_end_matches(['\\', '/'])
            };
            to_cstr(Some(PathBuf::from(s)))
        }
        #[cfg(not(any(
            target_os = "android",
            target_os = "ios",
            target_os = "macos",
            target_os = "windows"
        )))]
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
        #[cfg(target_os = "windows")]
        {
            to_cstr(windows_impl::scoped(dirs::cache_dir()))
        }
        #[cfg(not(any(
            target_os = "android",
            target_os = "macos",
            target_os = "linux",
            target_os = "windows"
        )))]
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
        #[cfg(target_os = "windows")]
        {
            to_cstr(windows_impl::scoped(dirs::data_dir()))
        }
        #[cfg(not(any(
            target_os = "android",
            target_os = "macos",
            target_os = "linux",
            target_os = "windows"
        )))]
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
    fn linux_scoped_returns_path_even_when_dir_unwritable() {
        // scoped must return Some regardless of create_dir_all success —
        // consistent with macOS: callers decide how to handle unwritable paths.
        let result = linux::scoped(Some(std::path::PathBuf::from("/proc/ppn_test_unwritable")));
        assert!(
            result.is_some(),
            "scoped must return Some even when the directory cannot be created",
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

    // ── Windows-specific tests ────────────────────────────────────────────────

    /// This logic runs on every platform — validates the separator-stripping
    /// formula used inside ppn_temp_dir on Windows, including the root-drive guard.
    #[test]
    fn temp_dir_separator_stripping_logic() {
        // Normal paths: trailing separator must be removed.
        for (input, expected) in &[
            ("C:\\Windows\\Temp\\", "C:\\Windows\\Temp"),
            ("C:\\Windows\\Temp", "C:\\Windows\\Temp"),
            ("/tmp/", "/tmp"),
            ("/tmp", "/tmp"),
        ] {
            let is_drive_root = input.len() == 3 && input.ends_with(":\\");
            let stripped = if is_drive_root {
                input
            } else {
                input.trim_end_matches(['\\', '/'])
            };
            assert_eq!(stripped, *expected, "input: {input:?}");
        }
        // Root drives must NOT have their separator stripped.
        for root in &["C:\\", "D:\\", "Z:\\"] {
            let is_drive_root = root.len() == 3 && root.ends_with(":\\");
            let stripped = if is_drive_root {
                *root
            } else {
                root.trim_end_matches(['\\', '/'])
            };
            assert_eq!(
                stripped, *root,
                "root drive {root:?} must keep its separator"
            );
        }
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn windows_temp_dir_has_no_trailing_separator() {
        let ptr = unsafe { ppn_temp_dir() };
        assert!(
            !ptr.is_null(),
            "ppn_temp_dir must not return null on Windows"
        );
        let s = unsafe { std::ffi::CStr::from_ptr(ptr) }.to_str().unwrap();
        assert!(
            !s.ends_with('\\') && !s.ends_with('/'),
            "trailing separator must be stripped, got: {s:?}",
        );
        unsafe { ppn_free(ptr as *mut c_char) };
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn windows_cache_dir_is_scoped() {
        let base = dirs::cache_dir().expect("cache dir must exist on Windows");
        let ptr = unsafe { ppn_cache_dir() };
        assert!(!ptr.is_null());
        let s = unsafe { std::ffi::CStr::from_ptr(ptr) }.to_str().unwrap();
        let scoped = std::path::Path::new(s);
        assert!(
            scoped.starts_with(&base),
            "cache dir must be inside the base: {s:?}"
        );
        assert_ne!(
            scoped, base,
            "cache dir must include the app subfolder suffix"
        );
        unsafe { ppn_free(ptr as *mut c_char) };
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn windows_data_dir_is_scoped() {
        let base = dirs::data_dir().expect("data dir must exist on Windows");
        let ptr = unsafe { ppn_data_dir() };
        assert!(!ptr.is_null());
        let s = unsafe { std::ffi::CStr::from_ptr(ptr) }.to_str().unwrap();
        let scoped = std::path::Path::new(s);
        assert!(
            scoped.starts_with(&base),
            "data dir must be inside the base: {s:?}"
        );
        assert_ne!(
            scoped, base,
            "data dir must include the app subfolder suffix"
        );
        unsafe { ppn_free(ptr as *mut c_char) };
    }

    // ── sanitize tests (run on all platforms — pure string logic) ─────────────

    #[test]
    fn sanitize_replaces_illegal_chars() {
        assert_eq!(sanitize("My:Company".to_string()), "My_Company");
        assert_eq!(sanitize("App/Name".to_string()), "App_Name");
        assert_eq!(sanitize("A<B>C".to_string()), "A_B_C");
    }

    #[test]
    fn sanitize_replaces_control_chars() {
        assert_eq!(sanitize("App\x00Name".to_string()), "App_Name");
        assert_eq!(sanitize("App\tName".to_string()), "App_Name");
        assert_eq!(sanitize("App\nName".to_string()), "App_Name");
    }

    #[test]
    fn sanitize_trims_trailing_dots_and_spaces() {
        assert_eq!(sanitize("App  ".to_string()), "App");
        assert_eq!(sanitize("App...".to_string()), "App");
        assert_eq!(sanitize("App. ".to_string()), "App");
    }

    #[test]
    fn sanitize_limits_to_255_utf16_code_units() {
        // ASCII: 1 char = 1 UTF-16 code unit — 300 chars capped to 255.
        let ascii = "a".repeat(300);
        let result = sanitize(ascii);
        assert_eq!(result.len(), 255, "ASCII: expected 255 bytes");
        let utf16: usize = result.chars().map(|c| c.len_utf16()).sum();
        assert_eq!(utf16, 255, "ASCII: expected 255 UTF-16 code units");

        // CJK: 1 char = 1 UTF-16 code unit — same cap.
        let cjk = "中".repeat(300);
        let result = sanitize(cjk);
        let utf16: usize = result.chars().map(|c| c.len_utf16()).sum();
        assert_eq!(utf16, 255, "CJK: expected 255 UTF-16 code units");

        // Emoji: 1 char = 2 UTF-16 code units — 128 emoji = 256 units, so cap is 127.
        let emoji = "😀".repeat(200);
        let result = sanitize(emoji);
        let utf16: usize = result.chars().map(|c| c.len_utf16()).sum();
        assert!(
            utf16 <= 255,
            "emoji: UTF-16 code units must not exceed 255, got {utf16}",
        );
        // 127 emoji = 254 code units; 128th would push it to 256 — verify exact cap.
        assert_eq!(
            utf16, 254,
            "emoji: expected 254 UTF-16 code units (127 × 2)"
        );
    }

    #[test]
    fn sanitize_prefixes_reserved_names() {
        for name in &["CON", "PRN", "AUX", "NUL", "con", "nul"] {
            let result = sanitize(name.to_string());
            assert!(
                result.starts_with('_'),
                "reserved name {name:?} must be prefixed, got {result:?}",
            );
        }
        for n in 1..=9u8 {
            for prefix in &["COM", "LPT"] {
                let name = format!("{prefix}{n}");
                let result = sanitize(name.clone());
                assert!(
                    result.starts_with('_'),
                    "reserved name {name:?} must be prefixed, got {result:?}",
                );
            }
        }
    }

    #[test]
    fn sanitize_keeps_safe_names() {
        assert_eq!(sanitize("MyCompany".to_string()), "MyCompany");
        assert_eq!(sanitize("CONSOLE".to_string()), "CONSOLE");
        assert_eq!(sanitize("COM10".to_string()), "COM10");
    }
}
