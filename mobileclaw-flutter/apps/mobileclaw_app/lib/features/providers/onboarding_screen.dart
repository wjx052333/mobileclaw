import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

import '../../core/engine_provider.dart';
import '../chat/chat_page.dart';
import 'provider_form_screen.dart';
import 'provider_notifier.dart';

/// First-launch wizard shown when no providers are configured.
///
/// Wraps [ProviderFormScreen] with a welcome header. After the user saves
/// a provider, sets it as active and navigates to [ChatPage].
class OnboardingScreen extends ConsumerWidget {
  const OnboardingScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Scaffold(
      body: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            const Padding(
              padding: EdgeInsets.fromLTRB(16, 32, 16, 8),
              child: Text(
                'Welcome to MobileClaw',
                style: TextStyle(fontSize: 24, fontWeight: FontWeight.bold),
                textAlign: TextAlign.center,
              ),
            ),
            const Padding(
              padding: EdgeInsets.symmetric(horizontal: 16),
              child: Text(
                'Add an LLM provider to get started.',
                textAlign: TextAlign.center,
              ),
            ),
            const SizedBox(height: 16),
            Expanded(
              child: _OnboardingForm(
                onProviderSaved: (id) async {
                  final agent = ref.read(agentInstanceProvider);
                  await agent.providerSetActive(id: id);
                  // Recreate the agent session so it reads the active provider
                  // from secrets.db instead of falling back to legacy Anthropic.
                  await reinitializeAgent(ref);
                  if (context.mounted) {
                    Navigator.of(context).pushReplacement(
                      MaterialPageRoute(builder: (_) => const ChatPage()),
                    );
                  }
                },
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _OnboardingForm extends ConsumerWidget {
  const _OnboardingForm({required this.onProviderSaved});

  final Future<void> Function(String id) onProviderSaved;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return ProviderFormScreen(
      onSaved: onProviderSaved,
      probeFn: Platform.isLinux || Platform.isAndroid
          ? MobileclawAgentImpl.probe
          : MockMobileclawAgent.probe,
    );
  }
}
