// ignore_for_file: depend_on_referenced_packages, prefer-assigning-await-expressions
import 'dart:io' show Platform;

import 'package:flutter_test/flutter_test.dart';
import 'package:path_provider/path_provider.dart' as origin;
import 'package:path_provider_foundation/path_provider_foundation.dart';
import 'package:path_provider_linux/path_provider_linux.dart';
import 'package:path_provider_native/path_provider_native.dart' as rust;
import 'package:path_provider_windows/path_provider_windows.dart';

void main() {
  // ignore: prefer-extracting-function-callbacks, a basic setup call in test.
  setUpAll(() {
    if (Platform.isWindows) {
      PathProviderWindows.registerWith();
    } else if (Platform.isLinux) {
      PathProviderLinux.registerWith();
    } else {
      PathProviderFoundation.registerWith();
    }
  });

  group(
    'all',
    () {
      test(
        'getDownloadsDirectory',
        () async => expect(
          rust.getDownloadsDirectory()?.path,
          (await origin.getDownloadsDirectory())?.path,
        ),
      );
      test(
        'getTemporaryDirectory',
        () async =>
            expect(rust.getTemporaryDirectory().path, (await origin.getTemporaryDirectory()).path),
      );
      test(
        'getApplicationSupportDirectory',
        () async => expect(
          rust.getApplicationSupportDirectory().path,
          (await origin.getApplicationSupportDirectory()).path,
        ),
      );
      test(
        'getApplicationDocumentsDirectory',
        () async => expect(
          rust.getApplicationDocumentsDirectory().path,
          (await origin.getApplicationDocumentsDirectory()).path,
        ),
      );
      test(
        'getApplicationCacheDirectory',
        () async => expect(
          rust.getApplicationCacheDirectory().path,
          (await origin.getApplicationCacheDirectory()).path,
        ),
      );
    },
    skip: Platform.isLinux, // TODO(tsinis) Enable in GitHub Actions (default Linux runner).
  );
}
