import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

import '../../core/engine_provider.dart';
import '../settings/settings_page.dart';

class ChatPage extends ConsumerStatefulWidget {
  const ChatPage({super.key});

  @override
  ConsumerState<ChatPage> createState() => _ChatPageState();
}

class _ChatPageState extends ConsumerState<ChatPage> {
  final _controller = TextEditingController();
  final _buffer = StringBuffer();
  String _displayText = '';
  bool _busy = false;
  Stream<AgentEvent>? _stream;

  Future<void> _send() async {
    final input = _controller.text.trim();
    if (input.isEmpty || _busy) return;
    _controller.clear();
    _buffer.clear();
    setState(() {
      _displayText = '';
      _busy = true;
      _stream = ref.read(agentProvider).value?.chat(input);
    });
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('MobileClaw'),
        actions: [
          IconButton(
            icon: const Icon(Icons.settings),
            onPressed: () => Navigator.of(context).push<void>(
              MaterialPageRoute(builder: (_) => const SettingsPage()),
            ),
          ),
        ],
      ),
      body: Column(
        children: [
          Expanded(
            child: StreamBuilder<AgentEvent>(
              stream: _stream,
              builder: (context, snapshot) {
                if (snapshot.hasError) {
                  final e = snapshot.error;
                  return Center(
                    child: Text(
                      e is ClawException
                          ? 'Error [${e.type}]: ${e.message}'
                          : 'Error: $e',
                      style: const TextStyle(color: Colors.red),
                    ),
                  );
                }
                if (snapshot.hasData) {
                  final event = snapshot.data!;
                  switch (event) {
                    case TextDeltaEvent(:final text):
                      _buffer.write(text);
                      WidgetsBinding.instance.addPostFrameCallback((_) {
                        if (mounted) {
                          setState(() => _displayText = _buffer.toString());
                        }
                      });
                    case ToolCallEvent(:final toolName):
                      debugPrint('Running tool: $toolName');
                    case ToolResultEvent(:final toolName, :final success):
                      debugPrint('Tool $toolName finished (success=$success)');
                    case DoneEvent():
                      WidgetsBinding.instance.addPostFrameCallback((_) {
                        if (mounted) setState(() => _busy = false);
                      });
                  }
                }
                return SingleChildScrollView(
                  padding: const EdgeInsets.all(16),
                  child: Text(_displayText),
                );
              },
            ),
          ),
          _InputBar(controller: _controller, busy: _busy, onSend: _send),
        ],
      ),
    );
  }
}

class _InputBar extends StatelessWidget {
  const _InputBar({
    required this.controller,
    required this.busy,
    required this.onSend,
  });

  final TextEditingController controller;
  final bool busy;
  final VoidCallback onSend;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 0, 4, 12),
      child: Row(
        children: [
          Expanded(
            child: TextField(
              controller: controller,
              decoration: const InputDecoration(hintText: 'Message…'),
              onSubmitted: (_) => onSend(),
            ),
          ),
          IconButton(
            icon: busy
                ? const SizedBox(
                    width: 20,
                    height: 20,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.send),
            onPressed: busy ? null : onSend,
          ),
        ],
      ),
    );
  }
}
