import 'dart:io' show Platform;

import 'package:path_provider_dart/path_provider_dart.dart';

void main() {
  final map = {
    'getApplicationCacheDirectory': getApplicationCacheDirectory(),
    'getApplicationDocumentsDirectory': getApplicationDocumentsDirectory(),
    'getApplicationSupportDirectory': getApplicationSupportDirectory(),
    'getDownloadsDirectory': getDownloadsDirectory(),
    'getTemporaryDirectory': getTemporaryDirectory(),
    if (Platform.isIOS || Platform.isMacOS) 'getLibraryDirectory': getLibraryDirectory(),
  };

  print('Result: $map'); // ignore: avoid_print, it's example.
}
