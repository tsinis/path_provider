# path_provider_native example

Minimal Flutter app that mounts **path_provider_native** next to Google's
**path_provider** and renders the two side by side. A green check means the
two resolved to the same path on the running device.

```bash
flutter pub get
flutter run
```

## Integration tests

On-device parity checks live under `integration_test/`:

```bash
flutter test integration_test/
```

Run these against an Android emulator and an iOS simulator before publishing a
new `path_provider_native` release — host `dart test` cannot exercise either
sandbox.
