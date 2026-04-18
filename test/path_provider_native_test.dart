import 'dart:io';

import 'package:path_provider_native/path_provider_native.dart';
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
  );

  test('getLibraryDirectory throws UnsupportedError off iOS/macOS', () {
    if (Platform.isIOS || Platform.isMacOS) return;
    expect(getLibraryDirectory, throwsUnsupportedError);
  });
}
