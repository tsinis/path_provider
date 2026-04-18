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
    'Avoid `brew` or any other non-rustup installation, use version defined in rust-toolchain.toml'
    '\nFor example: rustup toolchain install 1.94.1 &&\n'
    'rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios '
    '--toolchain 1.94.1-aarch64-apple-darwin &&\n'
    'rm -rf target';
