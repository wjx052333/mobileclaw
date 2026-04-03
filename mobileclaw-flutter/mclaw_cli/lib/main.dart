/// Command-line tool for testing mobileclaw SDK end-to-end.
///
/// Usage:
///   flutter run -d linux                          # interactive mode
///   flutter run -d linux -- "Hello"               # single message
///   flutter run -d linux -- --so-path <path>      # override .so path
///
/// Runs headlessly — all interaction via stdin/stdout.
/// A tiny invisible window is required by Flutter Linux.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/widgets.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';
import 'package:mobileclaw_sdk/src/bridge/frb_generated.dart';
import 'package:path_provider/path_provider.dart';

// ---------------------------------------------------------------------------
// Path resolution
// ---------------------------------------------------------------------------

String _defaultSoPath() {
  final flutterRoot = Directory.current.path;
  return '$flutterRoot/../../mobileclaw-core/target/release/libmobileclaw_core.so';
}

String _resolveSoPath(List<String> args) {
  for (var i = 0; i < args.length - 1; i++) {
    if (args[i] == '--so-path') return args[i + 1];
  }
  return _defaultSoPath();
}

// ---------------------------------------------------------------------------
// App data directory
// ---------------------------------------------------------------------------

Future<Directory> _dataDir() async {
  final base = await getApplicationSupportDirectory();
  return Directory('${base.path}/mobileclaw_cli');
}

// ---------------------------------------------------------------------------
// Agent creation
// ---------------------------------------------------------------------------

const _devKey = <int>[
  0x6d, 0x63, 0x6c, 0x61, 0x77, 0x2d, 0x64, 0x65,
  0x76, 0x2d, 0x6b, 0x65, 0x79, 0x2d, 0x33, 0x32,
  0x62, 0x79, 0x74, 0x65, 0x73, 0x00, 0x00, 0x00,
  0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

Future<MobileclawAgent> _createAgent(Directory dataDir) async {
  final workspace = Directory('${dataDir.path}/workspace');
  await workspace.create(recursive: true);

  return MobileclawAgentImpl.create(
    apiKey: null,
    dbPath: '${dataDir.path}/claw.db',
    secretsDbPath: '${dataDir.path}/secrets.db',
    encryptionKey: _devKey,
    sandboxDir: workspace.path,
    httpAllowlist: ['https://api.anthropic.com/'],
    logDir: dataDir.path,
  );
}

// ---------------------------------------------------------------------------
// Chat with streaming display
// ---------------------------------------------------------------------------

Future<void> _chat(MobileclawAgent agent, String message) async {
  stdout.write('\nYou: $message\n\nAssistant: ');
  stdout.flush();

  final buffer = StringBuffer();
  var toolCalls = 0;
  var toolErrors = 0;

  try {
    await for (final event in agent.chat(message)) {
      switch (event) {
        case TextDeltaEvent(:final text):
          buffer.write(text);
          stdout.write(text);
          stdout.flush();
        case ToolCallEvent(:final toolName):
          toolCalls++;
          stdout.write('\n  [tool: $toolName] ');
          stdout.flush();
        case ToolResultEvent(success: false):
          toolErrors++;
          stdout.write('FAIL');
          stdout.flush();
        case ToolResultEvent():
          stdout.write('OK');
          stdout.flush();
        case ContextStatsEvent(:final messagesPruned, :final tokensBeforeTurn, :final tokensAfterPrune):
          if (messagesPruned > 0) {
            stdout.write('\n  [pruned: $messagesPruned msgs, $tokensBeforeTurn→$tokensAfterPrune tokens] ');
            stdout.flush();
          }
        case TurnSummaryEvent(:final summary):
          stdout.write('\n  [summary: $summary] ');
          stdout.flush();
        case DoneEvent():
          stdout.writeln('\n');
          if (toolCalls > 0) {
            stdout.writeln('Done ($toolCalls tool${toolErrors > 0 ? ', $toolErrors failed' : ''}, ${buffer.length} chars)');
          } else {
            stdout.writeln('Done (${buffer.length} chars)');
          }
      }
    }
  } catch (e, st) {
    stdout.writeln('\n\nError: $e\n$st');
  }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

void main(List<String> args) async {
  // Parse non-flag args as message text.
  String? singleMessage;
  bool skipNext = false;
  for (final arg in args) {
    if (skipNext) { skipNext = false; continue; }
    if (arg == '--so-path') { skipNext = true; continue; }
    if (arg.startsWith('--')) continue;
    singleMessage = singleMessage == null ? arg : '$singleMessage $arg';
  }

  // Check .so exists.
  final soPath = _resolveSoPath(args);
  if (!File(soPath).existsSync()) {
    stderr.writeln('Error: Native library not found at: $soPath');
    stderr.writeln('Build with: cargo build --release -p mobileclaw-core');
    stderr.writeln('Or pass --so-path <path>');
    exit(1);
  }
  stdout.writeln('[mclaw] Native library: $soPath');

  // Init Flutter binding (required for FFI + path_provider).
  WidgetsFlutterBinding.ensureInitialized();

  // Init FFI bridge with explicit library path.
  await MobileclawCoreBridge.init(
    externalLibrary: ExternalLibrary.open(soPath),
  );
  stdout.writeln('[mclaw] FFI bridge initialized');

  // Create agent.
  final dataDir = await _dataDir();
  await dataDir.create(recursive: true);

  MobileclawAgent agent;
  try {
    agent = await _createAgent(dataDir);
  } catch (e) {
    stderr.writeln('Failed to create agent: $e');
    exit(1);
  }
  stdout.writeln('[mclaw] Data dir: ${dataDir.path}');

  // Show active provider.
  final activeProvider = await agent.providerGetActive();
  if (activeProvider != null) {
    stdout.writeln('[mclaw] Provider: ${activeProvider.name} (${activeProvider.protocol}/${activeProvider.model})');
  } else {
    stdout.writeln('[mclaw] No active provider — configure one in the app first');
  }

  final skills = agent.skills;
  if (skills.isNotEmpty) {
    stdout.writeln('[mclaw] Skills: ${skills.map((s) => s.name).join(', ')}');
  }
  stdout.writeln('[mclaw] Ready.\n');

  if (singleMessage != null) {
    await _chat(agent, singleMessage);
  } else {
    stdout.writeln('Type a message, or Ctrl+D to exit.\n');
    await for (final line in stdin.transform(utf8.decoder).transform(LineSplitter())) {
      final t = line.trim();
      if (t.isEmpty || t == 'quit' || t == 'exit') continue;
      await _chat(agent, t);
      stdout.writeln('---');
    }
  }

  agent.dispose();
  MobileclawCoreBridge.dispose();
  exit(0);
}
