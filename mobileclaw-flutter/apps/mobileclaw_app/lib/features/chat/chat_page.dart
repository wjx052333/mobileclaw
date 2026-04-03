import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';
import 'package:path_provider/path_provider.dart';

import '../../core/engine_provider.dart';
import '../settings/settings_page.dart';

/// Write an error to flutter.log on disk. Used for errors caught inside
/// widget builders that bypass [FlutterError.onError].
Future<void> _logError(String message) async {
  debugPrint(message);
  try {
    final dir = await getApplicationSupportDirectory();
    final file = File('${dir.path}/flutter.log');
    final line = '${DateTime.now().toIso8601String()} [ERROR] $message\n';
    await file.writeAsString(line, mode: FileMode.append, flush: true);
  } catch (_) {}
}

class ChatPage extends ConsumerStatefulWidget {
  const ChatPage({super.key});

  @override
  ConsumerState<ChatPage> createState() => _ChatPageState();
}

class _ChatPageState extends ConsumerState<ChatPage> {
  final _controller = TextEditingController();
  String _displayText = '';
  bool _busy = false;
  StreamSubscription<AgentEvent>? _subscription;

  @override
  void dispose() {
    _subscription?.cancel();
    super.dispose();
  }

  Future<void> _send() async {
    final input = _controller.text.trim();
    if (input.isEmpty || _busy) return;
    _controller.clear();
    await _subscription?.cancel();
    debugPrint('ChatPage._send: input="$input"');
    setState(() {
      _displayText += 'You: $input\n\n';
      _busy = true;
    });

    final stream = ref.read(agentProvider).value?.chat(input);
    if (stream == null) {
      debugPrint('ChatPage._send: agent is null');
      setState(() => _busy = false);
      return;
    }

    _subscription = stream.listen(
      (event) {
        debugPrint('ChatPage.listen: event=${event.runtimeType}');
        switch (event) {
          case TextDeltaEvent(:final text):
            setState(() => _displayText += text);
          case ToolCallEvent(:final toolName):
            debugPrint('Running tool: $toolName');
          case ToolResultEvent(:final toolName, :final success):
            debugPrint('Tool $toolName finished (success=$success)');
          case ContextStatsEvent():
            // Observability event, not shown to user.
          case TurnSummaryEvent():
            // Persisted to memory, not shown to user.
          case DoneEvent():
            setState(() {
              _displayText += '\n\n---\n\n';
              _busy = false;
            });
        }
      },
      onError: (e, st) {
        final msg = e is ClawException
            ? 'Chat error [${e.type}]: ${e.message}'
            : 'Chat error: $e';
        _logError('$msg\n$st');
        setState(() {
          _displayText += 'Error: $msg\n\n---\n\n';
          _busy = false;
        });
      },
    );
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
            child: SingleChildScrollView(
              padding: const EdgeInsets.all(16),
              child: Text(_displayText),
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
