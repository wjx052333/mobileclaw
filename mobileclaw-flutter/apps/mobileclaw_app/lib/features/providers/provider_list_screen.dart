import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

import 'provider_notifier.dart';
import 'provider_form_screen.dart';

class ProviderListScreen extends ConsumerStatefulWidget {
  const ProviderListScreen({super.key});

  @override
  ConsumerState<ProviderListScreen> createState() => _ProviderListScreenState();
}

class _ProviderListScreenState extends ConsumerState<ProviderListScreen> {
  String? _activeId;

  @override
  void initState() {
    super.initState();
    _loadActiveId();
  }

  Future<void> _loadActiveId() async {
    final agent = ref.read(agentInstanceProvider);
    final active = await agent.providerGetActive();
    if (mounted) setState(() => _activeId = active?.id);
  }

  Future<void> _setActive(String id) async {
    try {
      await ref.read(providerListProvider.notifier).setActive(id: id);
      if (mounted) {
        setState(() => _activeId = id);
        ScaffoldMessenger.of(context)
            .showSnackBar(const SnackBar(content: Text('Active provider updated')));
      }
    } on ClawException catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Error: ${e.message}')));
      }
    }
  }

  Future<void> _delete(String id) async {
    await ref.read(providerListProvider.notifier).deleteProvider(id: id);
    if (_activeId == id) setState(() => _activeId = null);
  }

  Future<void> _openForm({ProviderConfigDto? existing}) async {
    await Navigator.of(context).push<void>(
      MaterialPageRoute(
        builder: (_) => ProviderFormScreen(existing: existing),
      ),
    );
    // Refresh list after returning from form
    await ref.read(providerListProvider.notifier).refresh();
    await _loadActiveId();
  }

  @override
  Widget build(BuildContext context) {
    final state = ref.watch(providerListProvider);

    return Scaffold(
      appBar: AppBar(title: const Text('LLM Providers')),
      floatingActionButton: FloatingActionButton(
        onPressed: () => _openForm(),
        child: const Icon(Icons.add),
      ),
      body: state.when(
        loading: () => const Center(child: CircularProgressIndicator()),
        error: (e, _) => Center(child: Text('Error: $e')),
        data: (providers) {
          if (providers.isEmpty) {
            return const Center(child: Text('No providers configured'));
          }
          return ListView.builder(
            itemCount: providers.length,
            itemBuilder: (context, index) {
              final p = providers[index];
              final isActive = p.id == _activeId;
              return Dismissible(
                key: ValueKey(p.id),
                direction: DismissDirection.endToStart,
                background: Container(
                  alignment: Alignment.centerRight,
                  color: Colors.red,
                  padding: const EdgeInsets.only(right: 16),
                  child: const Icon(Icons.delete, color: Colors.white),
                ),
                onDismissed: (_) => _delete(p.id),
                child: ListTile(
                  leading: Icon(
                    isActive ? Icons.check_circle : Icons.circle_outlined,
                    color: isActive ? Colors.green : null,
                  ),
                  title: Text(
                    p.name,
                    style: const TextStyle(fontWeight: FontWeight.bold),
                  ),
                  subtitle: Text('${p.protocol} · ${p.model}'),
                  onTap: () => _setActive(p.id),
                  trailing: IconButton(
                    icon: const Icon(Icons.edit),
                    onPressed: () => _openForm(existing: p),
                  ),
                ),
              );
            },
          );
        },
      ),
    );
  }
}
