import 'dart:convert' show jsonEncode;
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:path_provider_native_example/comparison.dart' as app;

void main() => test('paths match', () async {
  final _ = IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  final comparisonMap = await app.Comparison.runComparison;
  final isSuccess = comparisonMap.values.every((i) => i.isMatching == true);
  final reason = jsonEncode(comparisonMap.map((k, v) => MapEntry(k, v.toJson())));
  expect(isSuccess, isTrue, reason: reason);
});
