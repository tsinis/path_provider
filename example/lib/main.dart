// ignore_for_file: avoid-ignoring-return-values, we don't need return value from it.
import 'dart:io' show Platform;

import 'package:flutter/widgets.dart';
import 'comparison.dart';

void main() async {
  if (Platform.isAndroid) WidgetsFlutterBinding.ensureInitialized();
  runApp(_HomeScreen(comparisons: await Comparison.runComparison));
}

class _HomeScreen extends StatelessWidget {
  const _HomeScreen({required this.comparisons});

  final Map<String, Comparison> comparisons;

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
  final Comparison comparison;

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
