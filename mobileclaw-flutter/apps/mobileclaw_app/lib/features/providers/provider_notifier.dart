import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

import '../../core/engine_provider.dart';

/// Exposes the agent singleton so tests can override it.
/// Points at the existing [agentProvider]; tests override this with a mock.
final agentInstanceProvider = Provider<MobileclawAgent>((ref) {
  // Throws StateError if called before agentProvider resolves.
  return ref.watch(agentProvider).requireValue;
});

/// Riverpod provider for the list of saved LLM providers.
///
/// State: AsyncValue<List<ProviderConfigDto>>
///   - AsyncLoading: initial fetch in progress
///   - AsyncData: list loaded (may be empty)
///   - AsyncError: fetch failed
final providerListProvider =
    StateNotifierProvider<ProviderNotifier, AsyncValue<List<ProviderConfigDto>>>(
  (ref) => ProviderNotifier(ref.watch(agentInstanceProvider)),
);

class ProviderNotifier
    extends StateNotifier<AsyncValue<List<ProviderConfigDto>>> {
  ProviderNotifier(this._agent) : super(const AsyncValue.loading()) {
    refresh();
  }

  final MobileclawAgent _agent;

  /// Reload the provider list from the Rust store.
  Future<void> refresh() async {
    state = const AsyncValue.loading();
    state = await AsyncValue.guard(() => _agent.providerList());
  }

  /// Save a new or updated provider and refresh the list.
  Future<void> addProvider({
    required ProviderConfigDto config,
    String? apiKey,
  }) async {
    await _agent.providerSave(config: config, apiKey: apiKey);
    await refresh();
  }

  /// Delete a provider and refresh the list.
  Future<void> deleteProvider({required String id}) async {
    await _agent.providerDelete(id: id);
    await refresh();
  }

  /// Set a provider as active.
  /// Throws [ClawException] if the id does not exist.
  Future<void> setActive({required String id}) async {
    await _agent.providerSetActive(id: id);
  }
}
