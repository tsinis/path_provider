// FFI bindings — symbol names dictated by Rust #[unsafe(no_mangle)] pub extern "C" exports.
// Hand-written (not ffigen): Rust signatures are trivial and annotations stay readable.
// ignore_for_file: non_constant_identifier_names, prefer-static-class
// ignore_for_file: prefer-typedefs-for-callbacks
@DefaultAsset('package:path_provider_native/src/ffi/bindings.dart')
library;

import 'dart:ffi';

import 'package:ffi/ffi.dart';

@Native<Void Function(Pointer<Char>)>(isLeaf: true)
external void ppn_free(Pointer<Char> ptr);

@Native<Pointer<Char> Function()>(isLeaf: true)
external Pointer<Char> ppn_temp_dir();

@Native<Pointer<Char> Function()>(isLeaf: true)
external Pointer<Char> ppn_cache_dir();

@Native<Pointer<Char> Function()>(isLeaf: true)
external Pointer<Char> ppn_config_dir();

@Native<Pointer<Char> Function()>(isLeaf: true)
external Pointer<Char> ppn_data_dir();

@Native<Pointer<Char> Function()>(isLeaf: true)
external Pointer<Char> ppn_data_local_dir();

@Native<Pointer<Char> Function()>(isLeaf: true)
external Pointer<Char> ppn_home_dir();

@Native<Pointer<Char> Function()>(isLeaf: true)
external Pointer<Char> ppn_document_dir();

@Native<Pointer<Char> Function()>(isLeaf: true)
external Pointer<Char> ppn_download_dir();

@Native<Pointer<Char> Function()>(isLeaf: true)
external Pointer<Char> ppn_picture_dir();

@Native<Pointer<Char> Function()>(isLeaf: true)
external Pointer<Char> ppn_audio_dir();

@Native<Pointer<Char> Function()>(isLeaf: true)
external Pointer<Char> ppn_video_dir();

@Native<Pointer<Char> Function()>(isLeaf: true)
external Pointer<Char> ppn_desktop_dir();

@Native<Pointer<Char> Function()>(isLeaf: true)
external Pointer<Char> ppn_public_dir();

@Native<Pointer<Char> Function()>(isLeaf: true)
external Pointer<Char> ppn_preference_dir();

@Native<Pointer<Char> Function()>(isLeaf: true)
external Pointer<Char> ppn_library_dir();

/// Call a `ppn_*_dir` function, copy the result into a Dart string, and free
/// the Rust-allocated buffer. Returns `null` when Rust returned null (directory
/// not available on this platform).
String? callDir(Pointer<Char> Function() fn) {
  final ptr = fn();
  if (ptr == nullptr) return null;
  try {
    return ptr.cast<Utf8>().toDartString();
  } finally {
    ppn_free(ptr);
  }
}
