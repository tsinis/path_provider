import 'dart:convert' show jsonEncode;
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:path_provider_dart_example/comparison.dart' as app;

void main() {
  // ignore: avoid-ignoring-return-values, we don't need return value from it.
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  test('paths match', () async {
    final comparison = await app.Comparison.runComparison;
    final hasMatch = comparison.values.every((i) => i.isMatching == true);
    expect(hasMatch, isTrue, reason: jsonEncode(comparison.map((k, v) => MapEntry(k, v.toJson()))));
  });
}
