// ignore_for_file: depend_on_referenced_packages, prefer-assigning-await-expressions
import 'dart:io' show Platform;

import 'package:flutter_test/flutter_test.dart';
import 'package:path_provider/path_provider.dart' as origin;
import 'package:path_provider_foundation/path_provider_foundation.dart';
import 'package:path_provider_linux/path_provider_linux.dart';
import 'package:path_provider_dart/path_provider_dart.dart' as rust;
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

  group('real platform', () {
    test('getDownloadsDirectory', () async {
      final theirs = (await origin.getDownloadsDirectory())?.path;
      final ours = rust.getDownloadsDirectory()?.path;
      expect(theirs, ours);
    });

    test('getTemporaryDirectory', () async {
      final theirs = (await origin.getTemporaryDirectory()).path;
      final ours = rust.getTemporaryDirectory().path;
      expect(theirs, ours);
    });

    test('getApplicationSupportDirectory', () async {
      final theirs = (await origin.getApplicationSupportDirectory()).path;
      final ours = rust.getApplicationSupportDirectory().path;
      expect(theirs, ours);
    });

    test('getApplicationDocumentsDirectory', () async {
      final theirs = (await origin.getApplicationDocumentsDirectory()).path;
      final ours = rust.getApplicationDocumentsDirectory().path;
      expect(theirs, ours);
    });

    test('getApplicationCacheDirectory', () async {
      final theirs = (await origin.getApplicationCacheDirectory()).path;
      final ours = rust.getApplicationCacheDirectory().path;
      expect(theirs, ours);
    });
  });
}
