# path_provider_dart

Synchronous, await-free pure Dart drop-in replacement for [`path_provider`](https://pub.dev/packages/path_provider).
Rust-powered via `dart:ffi`; no platform channels, no `Future`s, no platform folders.

## Layout

| Path                              | Role                                                                                                |
| --------------------------------- | --------------------------------------------------------------------------------------------------- |
| `Cargo.toml` / `src/`             | Rust crate `path_provider_dart` — wraps `dirs` (non-Android) and `/proc`-based detection (Android)  |
| `lib/` / `test/` / `pubspec.yaml` | Pure Dart package published to pub.dev                                                              |
| `hook/build.dart`                 | `native_toolchain_rust` build hook (emits code assets)                                              |
| `example/`                        | Flutter sample and integration tests; also depends on Google's `path_provider` for cross-validation |

Rust and Dart are first-class citizens at the repo root. Flutter only enters the picture through `example/`.

> **Android note:** `getTemporaryDirectory()`, `getApplicationCacheDirectory()`,
> `getApplicationSupportDirectory()`, and `getApplicationDocumentsDirectory()` are
> reliable on Android. `ppn_data_dir` maps to `<base>/files`; `ppn_document_dir`
> maps to `<base>/app_flutter` and creates the directory. Paths are derived from
> `/proc` entries with no JNI and no platform channels. Optional getters return
> `null`; throws are only for unsupported APIs (for example, `getLibraryDirectory`
> off iOS/macOS).

## API — drop-in replacement

```dart
// Before (path_provider)
import 'package:path_provider/path_provider.dart';
final dir = await getApplicationCacheDirectory();

// After (path_provider_dart)
import 'package:path_provider_dart/path_provider_dart.dart';
final dir = getApplicationCacheDirectory();
```

Same function names, same `Directory` return types, same nullability, same
`MissingPlatformDirectoryException` / `UnsupportedError` semantics — just no
`Future` and no `await`.

| `path_provider` (async)                    | `path_provider_dart` (sync)          |
| ------------------------------------------ | ------------------------------------ |
| `await getTemporaryDirectory()`            | `getTemporaryDirectory()`            |
| `await getApplicationCacheDirectory()`     | `getApplicationCacheDirectory()`     |
| `await getApplicationSupportDirectory()`   | `getApplicationSupportDirectory()`   |
| `await getApplicationDocumentsDirectory()` | `getApplicationDocumentsDirectory()` |
| `await getLibraryDirectory()`              | `getLibraryDirectory()`              |
| `await getDownloadsDirectory()`            | `getDownloadsDirectory()`            |

## Architecture

Three thin layers, all synchronous:

```text
┌────────────────────────────────────────────────────────────────────┐
│ lib/src/dirs.dart — global sync functions                          │
│                                                                    │
│   getApplicationCacheDirectory()                                   │
│       │                                                            │
│       ▼                                                            │
│                                                                    │
│ lib/src/ffi/bindings.dart — hand-written @Native() annotations     │
│       │                                                            │
│       ▼ symbol resolution via @DefaultAsset + native_toolchain_rust│
│                                                                    │
│ src/lib.rs — ppn_* exports (dirs + /proc on Android)               │
└────────────────────────────────────────────────────────────────────┘
```

On Android, `dirs` is excluded to keep Android resolution in the custom `/proc` path.
The `android` module in `src/lib.rs` reads `/proc/self/status` (UID → user ID)
and `/proc/self/cmdline` (package name) to derive the sandbox path without JNI.

## Testing

Prerequisites: the Rust toolchain pinned by `rust-toolchain.toml`, the Dart SDK,
and (for `example/`) the Flutter SDK.

### Rust crate — at the repo root

```bash
cargo test                                               # unit tests
cargo clippy --all-targets --all-features -- -D warnings # strict lints
cargo fmt --all -- --check                               # formatter check
```

### Dart package — at the repo root

```bash
dart pub upgrade
dart analyze                                             # analyzer (lints)
dart test                                                # unit + FFI tests
dart format --output=none --set-exit-if-changed .        # formatter check
```

The first `dart test` run triggers the build hook and compiles the Rust crate
for the host platform — later runs are cached.

### Flutter example — in `example/`

```bash
cd example
flutter pub get
flutter run                                              # pick a device
flutter test integration_test/                           # integration tests
```

The example app mounts both `path_provider_dart` and Google's `path_provider`
side by side so parity can be validated on-device.
