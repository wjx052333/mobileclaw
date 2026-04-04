// Tests for the FFI boundary: AgentEventDto variants, event list handling,
// and the exact scenario that caused the 2026-04-03 bug
// (Rust returns a multi-event Vec, Dart must receive every event in order).
//
// Run:
//   flutter test test/ffi_event_boundary_test.dart
//

import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';
import 'package:mobileclaw_sdk/src/bridge/ffi.dart' as ffi;

void main() {
  // ---------------------------------------------------------------------------
  // Unit tests: AgentEventDto construction and pattern matching for all variants
  // ---------------------------------------------------------------------------
  group('AgentEventDto variants', () {
    test('TextDelta can be constructed and matched', () {
      final dto = const ffi.AgentEventDto.textDelta(text: 'Hello world');
      expect(dto.when(
        textDelta: (t) => t,
        toolCall: (_) => throw StateError('unexpected'),
        toolResult: (_, __) => throw StateError('unexpected'),
        contextStats: (_, __, ___, ____, _____) => throw StateError('unexpected'),
        turnSummary: (_) => throw StateError('unexpected'),
        cameraAuthRequired: () => throw StateError('unexpected'),
        done: () => throw StateError('unexpected'),
      ), 'Hello world');
    });

    test('ToolCall can be constructed and matched', () {
      final dto = const ffi.AgentEventDto.toolCall(name: 'memory_search');
      expect(dto.when(
        textDelta: (_) => throw StateError('unexpected'),
        toolCall: (n) => n,
        toolResult: (_, __) => throw StateError('unexpected'),
        contextStats: (_, __, ___, ____, _____) => throw StateError('unexpected'),
        turnSummary: (_) => throw StateError('unexpected'),
        cameraAuthRequired: () => throw StateError('unexpected'),
        done: () => throw StateError('unexpected'),
      ), 'memory_search');
    });

    test('ToolResult success=true', () {
      final dto = const ffi.AgentEventDto.toolResult(name: 'file_read', success: true);
      expect(dto.when(
        textDelta: (_) => throw StateError('unexpected'),
        toolCall: (_) => throw StateError('unexpected'),
        toolResult: (n, s) => n,
        contextStats: (_, __, ___, ____, _____) => throw StateError('unexpected'),
        turnSummary: (_) => throw StateError('unexpected'),
        cameraAuthRequired: () => throw StateError('unexpected'),
        done: () => throw StateError('unexpected'),
      ), 'file_read');
      expect(dto.when(
        textDelta: (_) => throw StateError('unexpected'),
        toolCall: (_) => throw StateError('unexpected'),
        toolResult: (n, s) => s,
        contextStats: (_, __, ___, ____, _____) => throw StateError('unexpected'),
        turnSummary: (_) => throw StateError('unexpected'),
        cameraAuthRequired: () => throw StateError('unexpected'),
        done: () => throw StateError('unexpected'),
      ), isTrue);
    });

    test('ToolResult success=false', () {
      final dto = const ffi.AgentEventDto.toolResult(name: 'http_request', success: false);
      expect(dto.when(
        textDelta: (_) => throw StateError('unexpected'),
        toolCall: (_) => throw StateError('unexpected'),
        toolResult: (n, s) => s,
        contextStats: (_, __, ___, ____, _____) => throw StateError('unexpected'),
        turnSummary: (_) => throw StateError('unexpected'),
        cameraAuthRequired: () => throw StateError('unexpected'),
        done: () => throw StateError('unexpected'),
      ), isFalse);
    });

    test('ContextStats with BigInt fields', () {
      // usize fields from Rust are decoded as BigInt in Dart.
      final dto = ffi.AgentEventDto.contextStats(
        tokensBeforeTurn: BigInt.from(12345),
        tokensAfterPrune: BigInt.from(10000),
        messagesPruned: BigInt.from(3),
        historyLen: BigInt.from(15),
        pruningThreshold: BigInt.from(16000),
      );
      expect(dto.when(
        textDelta: (_) => throw StateError('unexpected'),
        toolCall: (_) => throw StateError('unexpected'),
        toolResult: (_, __) => throw StateError('unexpected'),
        contextStats: (tbt, tap, mp, hl, pt) => tbt.toInt(),
        turnSummary: (_) => throw StateError('unexpected'),
        cameraAuthRequired: () => throw StateError('unexpected'),
        done: () => throw StateError('unexpected'),
      ), 12345);
      expect(dto.when(
        textDelta: (_) => throw StateError('unexpected'),
        toolCall: (_) => throw StateError('unexpected'),
        toolResult: (_, __) => throw StateError('unexpected'),
        contextStats: (tbt, tap, mp, hl, pt) => tap.toInt(),
        turnSummary: (_) => throw StateError('unexpected'),
        cameraAuthRequired: () => throw StateError('unexpected'),
        done: () => throw StateError('unexpected'),
      ), 10000);
      expect(dto.when(
        textDelta: (_) => throw StateError('unexpected'),
        toolCall: (_) => throw StateError('unexpected'),
        toolResult: (_, __) => throw StateError('unexpected'),
        contextStats: (tbt, tap, mp, hl, pt) => mp.toInt(),
        turnSummary: (_) => throw StateError('unexpected'),
        cameraAuthRequired: () => throw StateError('unexpected'),
        done: () => throw StateError('unexpected'),
      ), 3);
      expect(dto.when(
        textDelta: (_) => throw StateError('unexpected'),
        toolCall: (_) => throw StateError('unexpected'),
        toolResult: (_, __) => throw StateError('unexpected'),
        contextStats: (tbt, tap, mp, hl, pt) => hl.toInt(),
        turnSummary: (_) => throw StateError('unexpected'),
        cameraAuthRequired: () => throw StateError('unexpected'),
        done: () => throw StateError('unexpected'),
      ), 15);
      expect(dto.when(
        textDelta: (_) => throw StateError('unexpected'),
        toolCall: (_) => throw StateError('unexpected'),
        toolResult: (_, __) => throw StateError('unexpected'),
        contextStats: (tbt, tap, mp, hl, pt) => pt.toInt(),
        turnSummary: (_) => throw StateError('unexpected'),
        cameraAuthRequired: () => throw StateError('unexpected'),
        done: () => throw StateError('unexpected'),
      ), 16000);
    });

    test('TurnSummary can be constructed and matched', () {
      final dto = const ffi.AgentEventDto.turnSummary(
        summary: 'User asked about X; assistant explained Y.',
      );
      expect(dto.when(
        textDelta: (_) => throw StateError('unexpected'),
        toolCall: (_) => throw StateError('unexpected'),
        toolResult: (_, __) => throw StateError('unexpected'),
        contextStats: (_, __, ___, ____, _____) => throw StateError('unexpected'),
        turnSummary: (s) => s,
        cameraAuthRequired: () => throw StateError('unexpected'),
        done: () => throw StateError('unexpected'),
      ), contains('asked about X'));
    });

    test('CameraAuthRequired can be constructed and matched', () {
      const dto = ffi.AgentEventDto.cameraAuthRequired();
      final matched = dto.when(
        textDelta: (_) => false,
        toolCall: (_) => false,
        toolResult: (_, __) => false,
        contextStats: (_, __, ___, ____, _____) => false,
        turnSummary: (_) => false,
        cameraAuthRequired: () => true,
        done: () => false,
      );
      expect(matched, isTrue);
    });

    test('Done is last event', () {
      final dto = const ffi.AgentEventDto.done();
      expect(dto.when(
        textDelta: (_) => false,
        toolCall: (_) => false,
        toolResult: (_, __) => false,
        contextStats: (_, __, ___, ____, _____) => false,
        turnSummary: (_) => false,
        cameraAuthRequired: () => false,
        done: () => true,
      ), isTrue);
    });
  });

  // ---------------------------------------------------------------------------
  // AgentEventDto → AgentEvent conversion (the _eventFromDto function)
  // ---------------------------------------------------------------------------
  group('AgentEventDto → AgentEvent conversion', () {
    test('all AgentEventDto variants produce non-null AgentEvent', () {
      final dtos = <ffi.AgentEventDto>[
        const ffi.AgentEventDto.textDelta(text: 'hi'),
        const ffi.AgentEventDto.toolCall(name: 'tool'),
        const ffi.AgentEventDto.toolResult(name: 'tool', success: true),
        ffi.AgentEventDto.contextStats(
          tokensBeforeTurn: BigInt.from(1),
          tokensAfterPrune: BigInt.from(2),
          messagesPruned: BigInt.from(3),
          historyLen: BigInt.from(4),
          pruningThreshold: BigInt.from(5),
        ),
        const ffi.AgentEventDto.turnSummary(summary: 'sum'),
        const ffi.AgentEventDto.cameraAuthRequired(),
        const ffi.AgentEventDto.done(),
      ];

      for (final dto in dtos) {
        final event = _eventFromDto(dto);
        expect(event, isA<AgentEvent>(), reason: 'dto ${dto.when(
          textDelta: (_) => 'TextDelta',
          toolCall: (_) => 'ToolCall',
          toolResult: (_, __) => 'ToolResult',
          contextStats: (_, __, ___, ____, _____) => 'ContextStats',
          turnSummary: (_) => 'TurnSummary',
          cameraAuthRequired: () => 'CameraAuthRequired',
          done: () => 'Done',
        )} did not produce an AgentEvent');
      }
    });

    test('CameraAuthRequired DTO produces CameraAuthRequiredEvent', () {
      const dto = ffi.AgentEventDto.cameraAuthRequired();
      final event = _eventFromDto(dto);
      expect(event, isA<CameraAuthRequiredEvent>());
    });
  });

  // ---------------------------------------------------------------------------
  // Event stream consumption: simulates the Timer.run pattern used in
  // MobileclawAgentImpl.chat() to verify no events are lost.
  // ---------------------------------------------------------------------------
  group('Event stream consumption (Timer.run pattern)', () {
    test('multi-round scenario: all 92 events received in order', () async {
      // Reproduce the exact event sequence from the 2026-04-03 debugging session:
      // Round 0: ToolCall(memory_search) → ToolResult(memory_search)
      // Round 1: TextDelta × 87 (LLM response)
      // End: ContextStats → TurnSummary → Done
      final dtos = <ffi.AgentEventDto>[
        const ffi.AgentEventDto.toolCall(name: 'memory_search'),
        const ffi.AgentEventDto.toolResult(name: 'memory_search', success: true),
      ];
      for (var i = 0; i < 87; i++) {
        dtos.add(ffi.AgentEventDto.textDelta(text: 'word$i '));
      }
      dtos.add(ffi.AgentEventDto.contextStats(
        tokensBeforeTurn: BigInt.from(8000),
        tokensAfterPrune: BigInt.from(7500),
        messagesPruned: BigInt.from(2),
        historyLen: BigInt.from(12),
        pruningThreshold: BigInt.from(16000),
      ));
      dtos.add(const ffi.AgentEventDto.turnSummary(
        summary: 'User asked about end-to-end testing.',
      ));
      dtos.add(const ffi.AgentEventDto.done());

      // Reproduce the exact StreamController + Timer.run pattern from agent_impl.dart
      final controller = StreamController<AgentEvent>.broadcast();
      Timer.run(() async {
        try {
          int index = 0;
          void emitNext() {
            if (index >= dtos.length) {
              controller.close();
              return;
            }
            controller.add(_eventFromDto(dtos[index]));
            index++;
            Timer.run(emitNext);
          }
          emitNext();
        } catch (e, st) {
          controller.addError(e, st);
          controller.close();
        }
      });

      final events = await controller.stream.toList();

      expect(events.length, 92); // 2 + 87 + 1 + 1 + 1

      // Verify tool events
      expect(events[0], isA<ToolCallEvent>());
      expect((events[0] as ToolCallEvent).toolName, 'memory_search');
      expect(events[1], isA<ToolResultEvent>());
      expect((events[1] as ToolResultEvent).success, isTrue);

      // Verify all 87 text events survived
      final textEvents = events.whereType<TextDeltaEvent>().toList();
      expect(textEvents.length, 87);
      for (var i = 0; i < 87; i++) {
        expect(textEvents[i].text, contains('word$i'));
      }

      // Verify ContextStats at index 89
      expect(events[89], isA<ContextStatsEvent>());
      final stats = events[89] as ContextStatsEvent;
      expect(stats.tokensBeforeTurn, 8000);
      expect(stats.tokensAfterPrune, 7500);
      expect(stats.messagesPruned, 2);

      // Verify TurnSummary at index 90
      expect(events[90], isA<TurnSummaryEvent>());
      expect((events[90] as TurnSummaryEvent).summary, contains('end-to-end'));

      // Verify Done is last
      expect(events[91], isA<DoneEvent>());
    });

    test('error propagation through Timer.run pattern', () async {
      final controller = StreamController<AgentEvent>.broadcast();
      Timer.run(() async {
        try {
          throw ClawException(type: 'test_error', message: 'simulated error');
        } catch (e, st) {
          controller.addError(e, st);
          controller.close();
        }
      });

      await expectLater(
        controller.stream.toList(),
        throwsA(isA<ClawException>()),
      );
    });
  });

  // ---------------------------------------------------------------------------
  // Stream timing regression: verify the original async* bug is understood
  // ---------------------------------------------------------------------------
  group('Stream timing regression', () {
    test('async* yield: all events are collected by toList()', () async {
      // This tests that even with the async* pattern (which batches all yields
      // in one microtask), toList() still collects all events. The bug was not
      // that events were lost from the stream — it was that StreamBuilder only
      // saw the final snapshot because Flutter renders between event queue items,
      // not between microtasks.
      final events = <ffi.AgentEventDto>[
        const ffi.AgentEventDto.textDelta(text: 'A'),
        const ffi.AgentEventDto.textDelta(text: 'B'),
        const ffi.AgentEventDto.done(),
      ];

      Stream<AgentEvent> generate() async* {
        for (final dto in events) {
          yield _eventFromDto(dto);
        }
      }

      final collected = await generate().toList();
      expect(collected.length, 3);
      expect(collected[0], isA<TextDeltaEvent>());
      expect((collected[0] as TextDeltaEvent).text, 'A');
      expect((collected[1] as TextDeltaEvent).text, 'B');
      expect(collected[2], isA<DoneEvent>());
    });
  });
}

// Local copy of the conversion function to avoid accessing private methods.
// This mirrors _eventFromDto in agent_impl.dart.
AgentEvent _eventFromDto(ffi.AgentEventDto dto) => dto.when(
      textDelta: (text) => TextDeltaEvent(text: text),
      toolCall: (name) => ToolCallEvent(toolName: name),
      toolResult: (name, success) =>
          ToolResultEvent(toolName: name, success: success),
      contextStats: (tokensBeforeTurn, tokensAfterPrune, messagesPruned, historyLen, pruningThreshold) =>
          ContextStatsEvent(
            tokensBeforeTurn: tokensBeforeTurn.toInt(),
            tokensAfterPrune: tokensAfterPrune.toInt(),
            messagesPruned: messagesPruned.toInt(),
            historyLen: historyLen.toInt(),
            pruningThreshold: pruningThreshold.toInt(),
          ),
      turnSummary: (summary) => TurnSummaryEvent(summary: summary),
      cameraAuthRequired: () => const CameraAuthRequiredEvent(),
      done: () => const DoneEvent(),
    );
