// ignore_for_file: prefer-static-class, to match `path_provider`'s API style.

import 'dart:ffi';
import 'dart:io' show Directory, Platform;

import 'android.dart';
import 'ffi/bindings.dart';

bool _isAndroidInitialized = false;

String? _path(Pointer<Char> Function() fn) {
  if (Platform.isAndroid && !_isAndroidInitialized) {
    _isAndroidInitialized = true;
    initAndroidIfNeeded();
  }

  return callDir(fn);
}

Directory _required(Pointer<Char> Function() fn, String name) {
  final path = _path(fn);
  if (path == null) throw MissingPlatformDirectoryException('Unable to get $name directory');

  return Directory(path);
}

Directory? _optional(Pointer<Char> Function() fn) {
  final path = _path(fn);

  return path == null ? null : Directory(path);
}

/// Thrown when a directory that should always be available on the current platform
/// cannot be obtained. Mirrors `path_provider`'s exception of the same name.
// ignore: prefer-match-file-name, to match `path_provider`'s API style.
class MissingPlatformDirectoryException implements Exception {
  const MissingPlatformDirectoryException(this.message, {this.details});

  final String message;

  final Object? details; // ignore:no-object-declaration, matches `path_provider`

  @override
  String toString() {
    // ignore: no-empty-string, reproducing path_provider's exception message.
    final tail = details == null ? '' : ': $details';

    return 'MissingPlatformDirectoryException($message)$tail';
  }
}

/// Synchronous mirror of `path_provider.getTemporaryDirectory()`.
Directory getTemporaryDirectory() => _required(ppn_temp_dir, 'temporary');

/// Synchronous mirror of `path_provider.getApplicationSupportDirectory()`.
Directory getApplicationSupportDirectory() => _required(ppn_data_dir, 'application support');

/// Synchronous mirror of `path_provider.getApplicationDocumentsDirectory()`.
/// On Android, `sysdirs` has no native concept of a documents directory, so we
/// fall back to the files directory — matching what Google's `path_provider`
/// returns (`Context.getFilesDir()`).
Directory getApplicationDocumentsDirectory() =>
    _required(Platform.isAndroid ? ppn_data_dir : ppn_document_dir, 'application documents');

/// Synchronous mirror of `path_provider.getApplicationCacheDirectory()`.
Directory getApplicationCacheDirectory() => _required(ppn_cache_dir, 'application cache');

/// Synchronous mirror of `path_provider.getLibraryDirectory()`. iOS/macOS only.
Directory getLibraryDirectory() {
  if (!Platform.isIOS && !Platform.isMacOS) {
    throw UnsupportedError('Library directory is only available on iOS and macOS');
  }

  return _required(ppn_library_dir, 'library');
}

/// Synchronous mirror of `path_provider.getDownloadsDirectory()`. Returns null on
/// platforms where the OS sandbox does not expose a downloads directory.
Directory? getDownloadsDirectory() => _optional(ppn_download_dir);

// Additional directories exposed by `sysdirs` beyond Google's `path_provider` surface.

Directory? getHomeDirectory() => _optional(ppn_home_dir);

Directory? getConfigDirectory() => _optional(ppn_config_dir);

Directory? getDataLocalDirectory() => _optional(ppn_data_local_dir);

Directory? getPicturesDirectory() => _optional(ppn_picture_dir);

Directory? getAudioDirectory() => _optional(ppn_audio_dir);

Directory? getVideoDirectory() => _optional(ppn_video_dir);

Directory? getDesktopDirectory() => _optional(ppn_desktop_dir);

Directory? getPublicDirectory() => _optional(ppn_public_dir);

Directory? getPreferenceDirectory() => _optional(ppn_preference_dir);
