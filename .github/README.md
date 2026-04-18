# path_provider_native

Synchronous, await-free drop-in replacement for [`path_provider`](https://pub.dev/packages/path_provider).
Rust-powered via `dart:ffi`; no platform channels, no `Future`s, no platform folders.

## Layout

| Path         | Role                                                                   |
| ------------ | ---------------------------------------------------------------------- |
| `Cargo.toml` / `src/` | Rust crate `path_provider_native` — thin wrapper over `sysdirs` |
| `lib/` / `test/` / `pubspec.yaml` | Pure Dart package published to pub.dev           |
| `hook/build.dart`     | `native_toolchain_rust` build hook (emits code assets)  |
| `example/`            | Flutter sample & integration tests, also depends on Google's `path_provider` for cross-validation |

Rust and Dart are first-class citizens at the repo root. Flutter only enters the
picture through `example/` (and through `package:jni` for the one lazy
`Context.getFilesDir()` call used to initialise `sysdirs` on Android).

## API — drop-in replacement

```dart
// Before (path_provider)
import 'package:path_provider/path_provider.dart';
final dir = await getApplicationCacheDirectory();
final path = dir.path;

// After (path_provider_native)
import 'package:path_provider_native/path_provider_native.dart';
final path = PathProviderNative.applicationCacheDirectory; // String?
```

| `path_provider` (async)                  | `PathProviderNative` (sync)                    |
| ---------------------------------------- | ---------------------------------------------- |
| `await getTemporaryDirectory()`          | `PathProviderNative.temporaryDirectory`        |
| `await getApplicationCacheDirectory()`   | `PathProviderNative.applicationCacheDirectory` |
| `await getApplicationSupportDirectory()` | `PathProviderNative.applicationSupportDirectory` |
| `await getApplicationDocumentsDirectory()` | `PathProviderNative.applicationDocumentsDirectory` |
| `await getDownloadsDirectory()`          | `PathProviderNative.downloadsDirectory`        |

Every getter returns `String?`. `null` means "not available on this platform"
(OS sandbox restriction, not a package limitation).

## Architecture

Three thin layers, all synchronous:

```
┌────────────────────────────────────────────────────────────────────┐
│ lib/src/dirs.dart — PathProviderNative static getters              │
│                                                                    │
│   PathProviderNative.applicationCacheDirectory                     │
│       │                                                            │
│       ▼ (lazy Android init on first call via package:jni)          │
│                                                                    │
│ lib/src/ffi/bindings.dart — hand-written @Native() annotations     │
│       │                                                            │
│       ▼ symbol resolution via @DefaultAsset + native_toolchain_rust│
│                                                                    │
│ src/lib.rs — ppn_* exports wrap the sysdirs crate                  │
└────────────────────────────────────────────────────────────────────┘
```

`DynamicLibrary.open` does not fire `JNI_OnLoad`, so the `android-auto` feature
of `sysdirs` is deliberately disabled. On Android, Dart resolves
`Context.getFilesDir().getAbsolutePath()` synchronously via `package:jni` and
passes it to `ppn_init_android` on the first directory access.

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

The example app mounts both `PathProviderNative` and Google's `path_provider`
side by side so parity can be validated on-device.
