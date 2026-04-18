import 'dart:io' show Directory, Platform;

import 'package:flutter/widgets.dart';
import 'package:path_provider/path_provider.dart' as pp;
import 'package:path_provider_native/path_provider_native.dart' as ppn;

Future<Map<String, Comparsion>?> main(List<String> args) async {
  // ignore: avoid-ignoring-return-values, we don't need return value from it.
  if (Platform.isAndroid) WidgetsFlutterBinding.ensureInitialized();
  final comparisonMap = {
    'applicationCache': await Comparsion.create(
      ppn.getApplicationCacheDirectory(),
      pp.getApplicationCacheDirectory(),
    ),
    'applicationDocuments': await Comparsion.create(
      ppn.getApplicationDocumentsDirectory(),
      pp.getApplicationDocumentsDirectory(),
    ),
    'applicationSupport': await Comparsion.create(
      ppn.getApplicationSupportDirectory(),
      pp.getApplicationSupportDirectory(),
    ),
    'downloads': await Comparsion.create(ppn.getDownloadsDirectory(), pp.getDownloadsDirectory()),
    'temporary': await Comparsion.create(ppn.getTemporaryDirectory(), pp.getTemporaryDirectory()),
  };
  print('COMPARISON: $comparisonMap'); // ignore: avoid_print, just a demo.
  if (args.isEmpty) runApp(_HomeScreen(comparisons: comparisonMap));

  return comparisonMap;
}

class _HomeScreen extends StatelessWidget {
  const _HomeScreen({required this.comparisons});

  final Map<String, Comparsion> comparisons;

  @override
  Widget build(BuildContext context) => WidgetsApp(
    builder: (_, _) => SafeArea(
      child: DefaultTextStyle(
        style: const TextStyle(color: Color(0xFFEAEAEA), fontSize: 16),
        child: ListView.builder(
          itemBuilder: (_, index) {
            final entry = comparisons.entries.elementAtOrNull(index);
            if (entry == null) return const SizedBox.shrink();

            return _Row(entry.key, comparison: entry.value);
          },
          itemCount: comparisons.length,
          padding: const .all(16),
        ),
      ),
    ),
    color: const Color(0xFF101010),
  );
}

@immutable
// ignore: prefer-single-widget-per-file, demo widget
class _Row extends StatelessWidget {
  const _Row(this.name, {required this.comparison});

  static const _cautionColor = Color(0xFFFFC107);
  static const _errorColor = Color(0xFFF44336);
  static const _okayColor = Color(0xFF4CAF50);

  final String name;
  final Comparsion comparison;

  Color get _statusColor => switch (comparison.isMatching) {
    true => _okayColor,
    false => _errorColor,
    null => _cautionColor,
  };

  String get _statusIcon => switch (comparison.isMatching) {
    true => '[OK]',
    false => '[X]',
    null => '[~]',
  };

  @override
  Widget build(BuildContext context) => Text.rich(
    TextSpan(
      children: [
        TextSpan(
          style: TextStyle(color: _statusColor, fontWeight: .bold),
          text: _statusIcon,
        ),
        TextSpan(
          style: const TextStyle(fontWeight: .bold),
          text: ' $name', // Leading space for padding.
        ),
        const TextSpan(text: '\nrust: '),
        TextSpan(text: comparison.ffi),
        const TextSpan(text: '\norig: '),
        TextSpan(text: comparison.original),
        const TextSpan(text: '\n\n'),
      ],
    ),
  );
}

@pragma('vm:deeply-immutable')
final class Comparsion {
  const Comparsion(this._ffi, this._original);

  static Future<Comparsion> create(Directory? ffi, Future<Directory?> original) async =>
      // ignore: prefer-assigning-await-expressions, to see the future in debug.
      Comparsion(ffi?.path, (await original)?.path);

  final String? _ffi;
  final String? _original;

  bool? get isMatching {
    if (_original == null && _ffi == null) return true;
    if (_ffi?.isEmpty ?? true) return false;

    return _ffi == _original ? true : null;
  }

  String get ffi => _ffi ?? 'null';
  String get original => _original ?? 'null';

  @override
  String toString() => 'orig: $original\nrust: $ffi\n';
}
