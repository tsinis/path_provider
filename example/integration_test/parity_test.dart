import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:path_provider/path_provider.dart' as pp;
import 'package:path_provider_native/path_provider_native.dart' as ppn;

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  group('path_provider parity', () {
    testWidgets('getTemporaryDirectory matches', (_) async {
      final google = await pp.getTemporaryDirectory();
      expect(
        ppn.getTemporaryDirectory().path,
        google.path,
        reason: 'native must match google path_provider exactly',
      );
    });

    testWidgets('getApplicationCacheDirectory matches', (_) async {
      final google = await pp.getApplicationCacheDirectory();
      expect(ppn.getApplicationCacheDirectory().path, google.path);
    });

    testWidgets('getApplicationSupportDirectory matches', (_) async {
      final google = await pp.getApplicationSupportDirectory();
      expect(ppn.getApplicationSupportDirectory().path, google.path);
    });

    testWidgets('getApplicationDocumentsDirectory matches', (_) async {
      final google = await pp.getApplicationDocumentsDirectory();
      expect(ppn.getApplicationDocumentsDirectory().path, google.path);
    });
  });
}
