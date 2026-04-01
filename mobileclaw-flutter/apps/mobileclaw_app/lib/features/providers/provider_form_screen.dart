import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

import 'provider_models.dart';
import 'provider_notifier.dart';

enum _TestState { idle, testing, passed, degraded, failed, skipped }

class ProviderFormScreen extends ConsumerStatefulWidget {
  const ProviderFormScreen({
    super.key,
    this.existing,
    this.onSaved,
    this.probeFn = MockMobileclawAgent.probe,
  });

  /// Non-null when editing an existing provider.
  final ProviderConfigDto? existing;

  /// Called after successful save, with the saved provider's id.
  /// If null, [Navigator.pop] is called instead.
  final Future<void> Function(String id)? onSaved;

  /// Probe function. Defaults to [MockMobileclawAgent.probe].
  /// Swap to [MobileclawAgentImpl.probe] after FFI codegen (Task 10).
  final Future<ProbeResultDto> Function({
    required ProviderConfigDto config,
    String? apiKey,
  }) probeFn;

  @override
  ConsumerState<ProviderFormScreen> createState() => _ProviderFormScreenState();
}

class _ProviderFormScreenState extends ConsumerState<ProviderFormScreen> {
  final _formKey = GlobalKey<FormState>();
  late final TextEditingController _nameCtrl;
  late final TextEditingController _urlCtrl;
  late final TextEditingController _modelCtrl;
  late final TextEditingController _keyCtrl;
  late ProviderProtocol _protocol;

  _TestState _testState = _TestState.idle;
  String? _testError;
  int? _testLatencyMs;
  bool _saving = false;

  bool get _isEditing => widget.existing != null;
  bool get _saveEnabled =>
      _testState == _TestState.passed ||
      _testState == _TestState.skipped ||
      _testState == _TestState.degraded;

  @override
  void initState() {
    super.initState();
    final e = widget.existing;
    _protocol = e != null
        ? ProviderProtocol.fromValue(e.protocol)
        : ProviderProtocol.anthropic;
    _nameCtrl = TextEditingController(text: e?.name ?? '');
    _urlCtrl = TextEditingController(text: e?.baseUrl ?? _protocol.urlHint);
    _modelCtrl = TextEditingController(text: e?.model ?? '');
    _keyCtrl = TextEditingController();
  }

  @override
  void dispose() {
    _nameCtrl.dispose();
    _urlCtrl.dispose();
    _modelCtrl.dispose();
    _keyCtrl.dispose();
    super.dispose();
  }

  void _onProtocolChanged(ProviderProtocol? p) {
    if (p == null) return;
    setState(() {
      _protocol = p;
      if (_urlCtrl.text.isEmpty ||
          ProviderProtocol.values.any((v) => v.urlHint == _urlCtrl.text)) {
        _urlCtrl.text = p.urlHint;
      }
      _testState = _TestState.idle;
    });
  }

  Future<void> _runTest() async {
    if (!_formKey.currentState!.validate()) return;
    setState(() => _testState = _TestState.testing);

    final apiKey = _keyCtrl.text.trim().isEmpty ? null : _keyCtrl.text.trim();
    final config = ProviderConfigDto(
      id: widget.existing?.id ?? '',
      name: _nameCtrl.text.trim(),
      protocol: _protocol.value,
      baseUrl: _urlCtrl.text.trim(),
      model: _modelCtrl.text.trim(),
      createdAt: 0,
    );

    final result = await widget.probeFn(config: config, apiKey: apiKey);

    if (mounted) {
      setState(() {
        _testLatencyMs = result.latencyMs;
        _testError = result.error;
        if (!result.ok) {
          _testState = _TestState.failed;
        } else if (result.degraded) {
          _testState = _TestState.degraded;
        } else {
          _testState = _TestState.passed;
        }
      });
    }
  }

  Future<void> _save() async {
    if (!_formKey.currentState!.validate()) return;
    setState(() => _saving = true);

    final agent = ref.read(agentInstanceProvider);
    try {
      final config = ProviderConfigDto(
        id: widget.existing?.id ?? '',
        name: _nameCtrl.text.trim(),
        protocol: _protocol.value,
        baseUrl: _urlCtrl.text.trim(),
        model: _modelCtrl.text.trim(),
        createdAt: widget.existing?.createdAt ?? 0,
      );
      final apiKey = _keyCtrl.text.trim().isEmpty ? null : _keyCtrl.text.trim();
      await agent.providerSave(config: config, apiKey: apiKey);
      if (mounted) {
        if (widget.onSaved != null) {
          final list = await agent.providerList();
          final savedId = config.id.isNotEmpty
              ? config.id
              : list.reduce((a, b) => a.createdAt > b.createdAt ? a : b).id;
          await widget.onSaved!(savedId);
        } else {
          Navigator.of(context).pop();
        }
      }
    } on ClawException catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Save failed: ${e.message}')));
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Text(_isEditing ? 'Edit Provider' : 'Add Provider'),
      ),
      body: SingleChildScrollView(
        padding: const EdgeInsets.all(16),
        child: Form(
          key: _formKey,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              TextFormField(
                key: const Key('field_name'),
                controller: _nameCtrl,
                decoration: const InputDecoration(labelText: 'Name'),
                validator: (v) =>
                    (v == null || v.trim().isEmpty) ? 'Name is required' : null,
              ),
              const SizedBox(height: 16),
              DropdownButtonFormField<ProviderProtocol>(
                value: _protocol,
                decoration: const InputDecoration(labelText: 'Protocol'),
                items: ProviderProtocol.values
                    .map((p) => DropdownMenuItem(
                          value: p,
                          child: Text(p.displayName),
                        ))
                    .toList(),
                onChanged: _onProtocolChanged,
              ),
              const SizedBox(height: 16),
              TextFormField(
                key: const Key('field_url'),
                controller: _urlCtrl,
                decoration: InputDecoration(
                  labelText: 'Base URL',
                  hintText: _protocol.urlHint,
                ),
                keyboardType: TextInputType.url,
                validator: (v) =>
                    (v == null || v.trim().isEmpty) ? 'URL is required' : null,
              ),
              const SizedBox(height: 16),
              TextFormField(
                key: const Key('field_model'),
                controller: _modelCtrl,
                decoration: const InputDecoration(labelText: 'Model'),
                validator: (v) =>
                    (v == null || v.trim().isEmpty) ? 'Model is required' : null,
              ),
              const SizedBox(height: 16),
              TextFormField(
                key: const Key('field_api_key'),
                controller: _keyCtrl,
                obscureText: true,
                decoration: InputDecoration(
                  labelText: 'API Key',
                  hintText: _isEditing ? '••••••••' : 'Leave blank for Ollama',
                ),
              ),
              const SizedBox(height: 24),
              Row(
                children: [
                  Expanded(
                    child: OutlinedButton(
                      onPressed:
                          _testState == _TestState.testing ? null : _runTest,
                      child: _testState == _TestState.testing
                          ? const SizedBox(
                              width: 16,
                              height: 16,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : const Text('Test'),
                    ),
                  ),
                  if (_testState != _TestState.idle &&
                      _testState != _TestState.testing) ...[
                    const SizedBox(width: 8),
                    _TestChip(state: _testState, latencyMs: _testLatencyMs),
                  ],
                ],
              ),
              if (_testState == _TestState.failed && _testError != null)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: Text(
                    _testError!,
                    style: TextStyle(color: Theme.of(context).colorScheme.error),
                  ),
                ),
              if (_testState == _TestState.degraded)
                const Padding(
                  padding: EdgeInsets.only(top: 8),
                  child: Text(
                    'Warning: models endpoint responded but completions are unverified.',
                    style: TextStyle(color: Colors.orange),
                  ),
                ),
              const SizedBox(height: 8),
              if (!_saveEnabled)
                Align(
                  alignment: Alignment.centerRight,
                  child: TextButton(
                    onPressed: () => setState(() => _testState = _TestState.skipped),
                    child: const Text('skip test'),
                  ),
                ),
              const SizedBox(height: 16),
              ElevatedButton(
                onPressed: _saveEnabled && !_saving ? _save : null,
                child: _saving
                    ? const SizedBox(
                        width: 16,
                        height: 16,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Text('Save'),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _TestChip extends StatelessWidget {
  const _TestChip({required this.state, required this.latencyMs});

  final _TestState state;
  final int? latencyMs;

  @override
  Widget build(BuildContext context) {
    final (icon, color, label) = switch (state) {
      _TestState.passed => (Icons.check_circle, Colors.green, 'OK ${latencyMs}ms'),
      _TestState.degraded => (Icons.warning, Colors.orange, 'Degraded'),
      _TestState.failed => (Icons.error, Colors.red, 'Failed'),
      _TestState.skipped => (Icons.skip_next, Colors.grey, 'Skipped'),
      _ => (Icons.help, Colors.grey, ''),
    };
    return Chip(
      avatar: Icon(icon, color: color, size: 16),
      label: Text(label),
    );
  }
}
