import 'dart:io';

import 'package:path_provider_dart/path_provider_dart.dart';
import 'package:test/test.dart';

void main() {
  test(
    'getTemporaryDirectory returns an existing directory on host',
    () => expect(
      getTemporaryDirectory().existsSync(),
      isTrue,
      reason: 'every supported desktop platform has a temp dir',
    ),
  );

  test(
    'getDownloadsDirectory behaves per-platform',
    () => expect(
      getDownloadsDirectory(),
      Platform.isAndroid || Platform.isIOS ? isNull : isNotNull,
      reason: 'Downloads is sandbox-restricted on mobile',
    ),
    skip: Platform.isLinux ? 'XDG_DOWNLOAD_DIR is unset on headless CI runners' : null,
  );

  test(
    'getLibraryDirectory throws UnsupportedError off iOS/macOS',
    () => expect(getLibraryDirectory, throwsUnsupportedError),
    skip: Platform.isIOS || Platform.isMacOS ? 'Supported on iOS/macOS' : null,
  );

  group(
    'Linux',
    () {
      test(
        'getDownloadsDirectory',
        () => expect(
          getDownloadsDirectory()?.path,
          anyOf(isNull, isNotEmpty),
          reason: 'XDG_DOWNLOAD_DIR may be unset on headless runners',
        ),
      );
      test('getTemporaryDirectory', () => expect(getTemporaryDirectory().path, isNotEmpty));
      test(
        'getApplicationSupportDirectory',
        () => expect(getApplicationSupportDirectory().path, isNotEmpty),
      );
      test(
        'getApplicationDocumentsDirectory',
        () => expect(getApplicationDocumentsDirectory().path, isNotEmpty),
      );
      test(
        'getApplicationCacheDirectory',
        () => expect(getApplicationCacheDirectory().path, isNotEmpty),
      );
    },
    skip: Platform.isLinux, // For example on GitHub Actions' default Linux runner.
  );
}
