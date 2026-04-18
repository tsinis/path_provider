// Android-only bootstrap. Invoked from `_path()` in dirs.dart under a runtime
// `Platform.isAndroid` guard. `package:jni`'s Dart surface compiles on every
// platform; only runtime calls to `androidApplicationContext` would
// fail off-Android, and we never reach this code path there.
import 'dart:ffi';

import 'package:ffi/ffi.dart';
import 'package:jni/_internal.dart' show internal;
import 'package:jni/jni.dart' show JClass, JObject, JString, Jni;

import 'ffi/bindings.dart';

@internal
// ignore: prefer-static-class, it's a fine for internal platform bootstraps.
void initAndroidIfNeeded() {
  final ctx = Jni.androidApplicationContext;

  final ctxClass = JClass.forName('android/content/Context');
  final getFilesDir = ctxClass.instanceMethodId('getFilesDir', '()Ljava/io/File;');

  final fileClass = JClass.forName('java/io/File');
  final getAbsolutePath = fileClass.instanceMethodId('getAbsolutePath', '()Ljava/lang/String;');

  final fileObj = getFilesDir(ctx, JObject.type, const []);
  final jStr = getAbsolutePath(fileObj, JString.type, const []);
  final filesDir = jStr.toDartString(releaseOriginal: true);

  final ptr = filesDir.toNativeUtf8();
  try {
    ppn_init_android(ptr.cast<Char>());
  } finally {
    calloc.free(ptr);
  }
}
