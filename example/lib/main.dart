import 'comparison.dart';

void main() async {
  final map = await Comparison.runComparison;
  final hasMatch = map.values.every((i) => i.isMatching == true);
  print('Result: ${hasMatch ? 'success' : 'failure: $map'}'); // ignore: avoid_print, it's example.
}
