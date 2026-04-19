import 'dart:io' show Directory, Platform;

import 'package:path_provider_native/path_provider_native.dart';

void main() {
  final map = <String, Directory?>{
    'getApplicationCacheDirectory': getApplicationCacheDirectory(),
    'getApplicationSupportDirectory': getApplicationSupportDirectory(),
    'getDownloadsDirectory': getDownloadsDirectory(),
    'getTemporaryDirectory': getTemporaryDirectory(),
    if (!Platform.isAndroid) 'getApplicationDocumentsDirectory': getApplicationDocumentsDirectory(),
    if (Platform.isIOS || Platform.isMacOS) 'getLibraryDirectory': getLibraryDirectory(),
  };

  print('Result: $map'); // ignore: avoid_print, it's example.
}
