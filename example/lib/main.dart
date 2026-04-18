import 'dart:io' show Directory;

import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart' as pp;
import 'package:path_provider_native/path_provider_native.dart' as ppn;

void main() => runApp(const PathProviderCompareApp());

class PathProviderCompareApp extends StatelessWidget {
  const PathProviderCompareApp({super.key});

  @override
  Widget build(BuildContext context) => MaterialApp(
    home: const _HomeScreen(),
    theme: ThemeData.dark(useMaterial3: true),
    title: 'path_provider_native',
  );
}

class _HomeScreen extends StatefulWidget {
  const _HomeScreen();

  @override
  State<_HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends State<_HomeScreen> {
  List<_Row> _rows = const <_Row>[];

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final native = <String, Directory?>{
      'applicationCache': ppn.getApplicationCacheDirectory(),
      'applicationDocuments': ppn.getApplicationDocumentsDirectory(),
      'applicationSupport': ppn.getApplicationSupportDirectory(),
      'downloads': ppn.getDownloadsDirectory(),
      'temporary': ppn.getTemporaryDirectory(),
    };

    final google = <String, Directory?>{
      'applicationCache': await pp.getApplicationCacheDirectory(),
      'applicationDocuments': await pp.getApplicationDocumentsDirectory(),
      'applicationSupport': await pp.getApplicationSupportDirectory(),
      'downloads': await pp.getDownloadsDirectory(),
      'temporary': await pp.getTemporaryDirectory(),
    };

    setState(() {
      _rows = native.keys
          .map((k) => _Row(google: google[k], name: k, native: native[k]))
          .toList(growable: false);
    });
  }

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(title: const Text('path_provider parity')),
    body: ListView.separated(
      itemBuilder: (_, i) {
        final row = _rows[i];

        return ListTile(
          subtitle: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text('native: ${row.native?.path ?? '∅'}'),
              Text('google: ${row.google?.path ?? '∅'}'),
            ],
          ),
          title: Text(row.name),
          trailing: Icon(
            row.matches ? Icons.check_circle : Icons.error,
            color: row.matches ? Colors.green : Colors.orange,
          ),
        );
      },
      itemCount: _rows.length,
      separatorBuilder: (_, _) => const Divider(height: 1),
    ),
  );
}

@immutable
class _Row {
  const _Row({required this.google, required this.name, required this.native});

  final String name;
  final Directory? native;
  final Directory? google;

  bool get matches => native?.path == google?.path;
}
