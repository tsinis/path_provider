// ignore_for_file: prefer-static-class, it's a convention for build hooks.

import 'package:hooks/hooks.dart';
import 'package:native_toolchain_rust/native_toolchain_rust.dart';

Future<void> main(List<String> args) => build(args, _buildRust);

Future<void> _buildRust(BuildInput input, BuildOutputBuilder output) async {
  try {
    await const RustBuilder(
      assetName: 'src/ffi/bindings.dart',
      cratePath: '.',
    ).run(input: input, output: output);
  } on Object catch (error, stackTrace) {
    final message = error.toString();
    if (_isMissingTargetStdlib(message)) {
      Error.throwWithStackTrace(StateError('$message\n\n${_rustupHint()}'), stackTrace);
    }

    rethrow;
  }
}

bool _isMissingTargetStdlib(String message) =>
    message.contains("can't find crate for 'std'") ||
    message.contains("can't find crate for `std`") ||
    message.contains('target may not be installed');

String _rustupHint() =>
    'Avoid `brew` or any other non-rustup Rust install; use the toolchain pinned in '
    'rust-toolchain.toml. Typical fix:\n'
    '  rustup show    # installs the pinned toolchain and lists installed targets\n'
    '  rustup target add <target-triple>   # e.g. aarch64-apple-ios, '
    'x86_64-unknown-linux-gnu, x86_64-pc-windows-msvc, aarch64-linux-android\n'
    '  rm -rf target';
