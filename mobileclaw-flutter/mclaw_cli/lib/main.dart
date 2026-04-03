/// Command-line tool for testing mobileclaw SDK end-to-end.
///
/// Usage:
///   mclaw_cli                          # interactive chat REPL
///   mclaw_cli "Hello"                  # single message
///   mclaw_cli --so-path <path> "Hello" # override .so path
///
/// Provider management:
///   mclaw_cli providers                # list all providers
///   mclaw_cli providers add            # add a provider (interactive)
///   mclaw_cli providers set <name>     # set active provider by display name
///   mclaw_cli providers delete <name>  # delete a provider by display name

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
  // When running from the bundle (release build), look in lib/ relative to executable
  final bundleLibPath = '${Directory.current.path}/lib/libmobileclaw_core.so';
  if (File(bundleLibPath).existsSync()) {
    return bundleLibPath;
  }

  // Fallback: development path
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
  print('\nYou: $message\n\nAssistant: ');

  final buffer = StringBuffer();
  var toolCalls = 0;
  var toolErrors = 0;

  try {
    await for (final event in agent.chat(message)) {
      switch (event) {
        case TextDeltaEvent(:final text):
          buffer.write(text);
          print(text);
        case ToolCallEvent(:final toolName):
          toolCalls++;
          print('\n  [tool: $toolName] ');
        case ToolResultEvent(success: false):
          toolErrors++;
          print('FAIL');
        case ToolResultEvent():
          print('OK');
        case ContextStatsEvent(:final messagesPruned, :final tokensBeforeTurn, :final tokensAfterPrune):
          if (messagesPruned > 0) {
            print('\n  [pruned: $messagesPruned msgs, $tokensBeforeTurn→$tokensAfterPrune tokens] ');
          }
        case TurnSummaryEvent(:final summary):
          print('\n  [summary: $summary] ');
        case DoneEvent():
          print('\n');
          if (toolCalls > 0) {
            print('Done ($toolCalls tool${toolErrors > 0 ? ', $toolErrors failed' : ''}, ${buffer.length} chars)');
          } else {
            print('Done (${buffer.length} chars)');
          }
      }
    }
  } catch (e, st) {
    print('\n\nError: $e\n$st');
  }
}

// ---------------------------------------------------------------------------
// Provider management helpers
// ---------------------------------------------------------------------------

/// Read a line from stdin synchronously.
String _readLine() {
  final line = stdin.readLineSync();
  return line ?? '';
}

Future<void> _cmdProviderList(MobileclawAgent agent) async {
  final providers = await agent.providerList();
  final active = await agent.providerGetActive();
  if (providers.isEmpty) {
    print('No providers configured. Use "providers add" to add one.');
    return;
  }
  for (final p in providers) {
    final isActive = active != null && active.id == p.id;
    print('  ${isActive ? "* " : "  "}${p.name} (${p.protocol}/${p.model}) at ${p.baseUrl}');
  }
  if (active == null) {
    print('\nNo active provider set. Use "providers set <name>" to activate one.');
  } else {
    print('\nActive provider: ${active.name}');
  }
}

Future<void> _cmdProviderAdd(MobileclawAgent agent) async {
  final presets = <String, Map<String, String>>{
    '1': {'name': 'Anthropic', 'protocol': 'anthropic', 'baseUrl': 'https://api.anthropic.com', 'model': 'claude-opus-4-6'},
    '2': {'name': 'OpenAI', 'protocol': 'openai_compat', 'baseUrl': 'https://api.openai.com', 'model': 'gpt-4o'},
    '3': {'name': 'Ollama (local)', 'protocol': 'ollama', 'baseUrl': 'http://localhost:11434', 'model': 'llama3'},
    '4': {'name': 'Custom', 'protocol': 'openai_compat', 'baseUrl': '', 'model': ''},
  };

  print('\nChoose a preset or enter "custom":');
  print('  1) Anthropic (claude-opus-4-6)');
  print('  2) OpenAI (gpt-4o)');
  print('  3) Ollama (localhost:11434 / llama3)');
  print('  4) Custom\n');

  print('Select: ');
  final choice = _readLine();
  final preset = presets[choice];

  String name, protocol, baseUrl, model;
  if (preset != null) {
    print('Display name [${preset['name']}]: ');
    name = _readLine();
    if (name.isEmpty) name = preset['name']!;
    protocol = preset['protocol']!;
    print('Base URL [${preset['baseUrl']}]: ');
    baseUrl = _readLine();
    if (baseUrl.isEmpty) baseUrl = preset['baseUrl']!;
    print('Model [${preset['model']}]: ');
    model = _readLine();
    if (model.isEmpty) model = preset['model']!;
  } else {
    print('Display name: ');
    name = _readLine();
    print('Protocol (anthropic|openai_compat|ollama) [openai_compat]: ');
    protocol = _readLine();
    if (protocol.isEmpty) protocol = 'openai_compat';
    print('Base URL: ');
    baseUrl = _readLine();
    print('Model: ');
    model = _readLine();
  }

  if (name.isEmpty || baseUrl.isEmpty || model.isEmpty) {
    stderr.writeln('Error: name, base URL, and model are required.');
    exit(1);
  }

  // Validate protocol
  if (!['anthropic', 'openai_compat', 'ollama'].contains(protocol)) {
    stderr.writeln('Error: protocol must be one of: anthropic, openai_compat, ollama');
    exit(1);
  }

  // Check if provider with same name already exists
  final existing = await agent.providerList();
  final dup = existing.where((p) => p.name == name).toList();
  if (dup.isNotEmpty) {
    stderr.writeln('Error: provider "$name" already exists. Use a different name.');
    exit(1);
  }

  print('\nNow enter the API key (will be stored encrypted):');
  print('API key: ');
  final apiKey = _readLine();
  if (apiKey.isEmpty) {
    stderr.writeln('Warning: no API key provided. You can add one later by re-saving the provider.');
  }

  final config = ProviderConfigDto(
    id: '',  // Rust generates UUID on first save
    name: name,
    protocol: protocol,
    baseUrl: baseUrl,
    model: model,
    createdAt: DateTime.now().millisecondsSinceEpoch ~/ 1000,
  );

  await agent.providerSave(config: config, apiKey: apiKey.isEmpty ? null : apiKey);
  print('Provider "$name" saved.');

  // If this is the first provider, set it as active
  final active = await agent.providerGetActive();
  if (active == null) {
    await agent.providerSetActive(id: config.id);
    // Re-read to get the generated ID
    final saved = await agent.providerList();
    final me = saved.where((p) => p.name == name).firstOrNull;
    if (me != null) {
      await agent.providerSetActive(id: me.id);
      print('Provider "$name" set as active.');
    }
  } else {
    print('Active provider is still "${active.name}". Use "providers set $name" to switch.');
  }
}

Future<void> _cmdProviderSet(MobileclawAgent agent, String targetName) async {
  final providers = await agent.providerList();
  final match = providers.where((p) => p.name == targetName).toList();
  if (match.isEmpty) {
    stderr.writeln('Error: no provider named "$targetName".');
    stderr.writeln('Available: ${providers.map((p) => p.name).join(', ')}');
    exit(1);
  }
  await agent.providerSetActive(id: match[0].id);
  print('Provider "$targetName" set as active. Restart the chat to apply.');
}

Future<void> _cmdProviderDelete(MobileclawAgent agent, String targetName) async {
  final providers = await agent.providerList();
  final match = providers.where((p) => p.name == targetName).toList();
  if (match.isEmpty) {
    stderr.writeln('Error: no provider named "$targetName".');
    exit(1);
  }
  await agent.providerDelete(id: match[0].id);
  print('Provider "$targetName" deleted.');
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

void main(List<String> args) async {
  // Parse flags first.
  bool skipNext = false;
  final nonFlagArgs = <String>[];
  for (final arg in args) {
    if (skipNext) { skipNext = false; continue; }
    if (arg == '--so-path') { skipNext = true; continue; }
    nonFlagArgs.add(arg);
  }
  final singleMessage = nonFlagArgs.join(' ');

  // Check .so exists.
  final soPath = _resolveSoPath(args);
  if (!File(soPath).existsSync()) {
    stderr.writeln('Error: Native library not found at: $soPath');
    stderr.writeln('Build with: cargo build --release -p mobileclaw-core');
    stderr.writeln('Or pass --so-path <path>');
    exit(1);
  }

  // Init Flutter binding (required for FFI + path_provider).
  WidgetsFlutterBinding.ensureInitialized();

  // Init FFI bridge with explicit library path.
  await MobileclawCoreBridge.init(
    externalLibrary: ExternalLibrary.open(soPath),
  );

  // Create agent (needed for all operations).
  final dataDir = await _dataDir();
  await dataDir.create(recursive: true);

  MobileclawAgent agent;
  try {
    agent = await _createAgent(dataDir);
  } catch (e) {
    stderr.writeln('Failed to create agent: $e');
    exit(1);
  }

  // Dispatch subcommands.
  if (nonFlagArgs.isNotEmpty && nonFlagArgs[0] == 'providers') {
    final subcmd = nonFlagArgs.length > 1 ? nonFlagArgs[1] : '';
    switch (subcmd) {
      case '':
      case 'list':
        await _cmdProviderList(agent);
      case 'add':
        await _cmdProviderAdd(agent);
      case 'set':
        if (nonFlagArgs.length < 3) {
          stderr.writeln('Usage: mclaw_cli providers set <name>');
          exit(1);
        }
        await _cmdProviderSet(agent, nonFlagArgs.sublist(2).join(' '));
      case 'delete':
        if (nonFlagArgs.length < 3) {
          stderr.writeln('Usage: mclaw_cli providers delete <name>');
          exit(1);
        }
        await _cmdProviderDelete(agent, nonFlagArgs.sublist(2).join(' '));
      default:
        stderr.writeln('Unknown subcommand: $subcmd');
        stderr.writeln('Usage: mclaw_cli providers [list|add|set <name>|delete <name>]');
        exit(1);
    }
    agent.dispose();
    MobileclawCoreBridge.dispose();
    exit(0);
  }

  // Chat mode.
  print('[mclaw] Native library: $soPath');
  print('[mclaw] FFI bridge initialized');
  print('[mclaw] Data dir: ${dataDir.path}');

  final activeProvider = await agent.providerGetActive();
  if (activeProvider != null) {
    print('[mclaw] Provider: ${activeProvider.name} (${activeProvider.protocol}/${activeProvider.model})');
  } else {
    print('[mclaw] No active provider — run "providers add" to configure one');
  }

  final skills = agent.skills;
  if (skills.isNotEmpty) {
    print('[mclaw] Skills: ${skills.map((s) => s.name).join(', ')}');
  }
  print('[mclaw] Ready.\n');

  if (singleMessage.isNotEmpty) {
    if (activeProvider == null) {
      stderr.writeln('Error: No LLM provider configured. Run "mclaw_cli providers add" first.');
      exit(1);
    }
    await _chat(agent, singleMessage);
  } else {
    print('Type a message, or Ctrl+D to exit.\n');
    await for (final line in stdin.transform(utf8.decoder).transform(LineSplitter())) {
      final t = line.trim();
      if (t.isEmpty || t == 'quit' || t == 'exit') continue;
      if (activeProvider == null) {
        print('Error: No LLM provider configured. Run "providers add" first.');
        continue;
      }
      await _chat(agent, t);
      print('---');
    }
  }

  agent.dispose();
  MobileclawCoreBridge.dispose();
  exit(0);
}
