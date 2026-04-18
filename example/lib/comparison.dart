import 'dart:io' show Directory;

import 'package:path_provider/path_provider.dart' as rust;
import 'package:path_provider_native/path_provider_native.dart' as origin;

@pragma('vm:deeply-immutable')
final class Comparison {
  const Comparison(this._ffi, this._original);

  static Future<Comparison> create(Directory? ffi, Future<Directory?> original) async =>
      // ignore: prefer-assigning-await-expressions, we want to keep the original Future.
      Comparison(ffi?.path, (await original)?.path);

  final String? _ffi;
  final String? _original;

  bool? get isMatching {
    if (_original == null && _ffi == null) return true;
    if (_ffi?.isEmpty ?? true) return false;

    return _ffi == _original ? true : null;
  }

  String get ffi => _ffi ?? 'null';
  String get original => _original ?? 'null';

  Map<String, Object?> toJson() => {'match': isMatching, 'orig': _original, 'rust': _ffi};

  @override
  String toString() => toJson().toString();

  /// Run path provider comparison and return results.
  static Future<Map<String, Comparison>> get runComparison async => {
    'applicationCache': await Comparison.create(
      origin.getApplicationCacheDirectory(),
      rust.getApplicationCacheDirectory(),
    ),
    'applicationDocuments': await Comparison.create(
      origin.getApplicationDocumentsDirectory(),
      rust.getApplicationDocumentsDirectory(),
    ),
    'applicationSupport': await Comparison.create(
      origin.getApplicationSupportDirectory(),
      rust.getApplicationSupportDirectory(),
    ),
    'downloads': await Comparison.create(
      origin.getDownloadsDirectory(),
      rust.getDownloadsDirectory(),
    ),
    'temporary': await Comparison.create(
      origin.getTemporaryDirectory(),
      rust.getTemporaryDirectory(),
    ),
  };
}
