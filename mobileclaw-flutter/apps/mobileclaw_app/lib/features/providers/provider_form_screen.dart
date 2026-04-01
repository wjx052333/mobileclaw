import 'package:flutter/material.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

// TODO: Full implementation in Task 6
class ProviderFormScreen extends StatelessWidget {
  const ProviderFormScreen({super.key, this.existing});
  final ProviderConfigDto? existing;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Add Provider')),
      body: const Column(
        children: [
          Text('Protocol'),
        ],
      ),
    );
  }
}
