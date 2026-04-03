#!/usr/bin/env python3
"""
analyze_tool_e2e.py — Parse bench interaction log and compute tool-call accuracy.

Usage:
    python3 tools/analyze_tool_e2e.py <interaction_log.jsonl> <bench_prompts_tool_e2e.json>

Exit code:
    0  if all expected tools were called (100% success)
    1  if any expected tool was missing

Outputs:
    Per-turn result table + summary with success rate.
"""

import sys
import json
import os
import argparse

MCLAW_DATA_DIR_VAR = "MCLAW_DATA_DIR"


def check_env():
    val = os.environ.get(MCLAW_DATA_DIR_VAR)
    if not val:
        print(f"ERROR: environment variable {MCLAW_DATA_DIR_VAR} is not set.", file=sys.stderr)
        print(f"Please run:", file=sys.stderr)
        print(f"  export {MCLAW_DATA_DIR_VAR}=/home/wjx/agent_eyes/bot/mobileclaw/.claude/worktrees/feat+memory-optimization/build",
              file=sys.stderr)
        print(f"Then re-run the bench:", file=sys.stderr)
        print(f"  MCLAW_DATA_DIR=... ./target/release/mclaw bench \\", file=sys.stderr)
        print(f"    --prompts mobileclaw-cli/docs/bench_prompts_tool_e2e.json \\", file=sys.stderr)
        print(f"    --interaction-log ./build/bench_tool_e2e.jsonl \\", file=sys.stderr)
        print(f"    --turn-delay-ms 1000", file=sys.stderr)
        sys.exit(2)
    return val


def load_expected(prompts_path: str) -> dict[int, dict]:
    """Returns {turn_id: {label, expected_tools: list[str]}}"""
    with open(prompts_path) as f:
        data = json.load(f)
    result = {}
    for turn in data["turns"]:
        result[turn["id"]] = {
            "label": turn["label"],
            "expected_tools": turn.get("expected_tools", []),
        }
    return result


def load_log(log_path: str) -> list[dict]:
    """Parse JSONL interaction log."""
    records = []
    with open(log_path) as f:
        for i, line in enumerate(f, 1):
            line = line.strip()
            if not line:
                continue
            try:
                records.append(json.loads(line))
            except json.JSONDecodeError as e:
                print(f"WARN: line {i} is not valid JSON: {e}", file=sys.stderr)
    return records


def check_turn(record: dict, expected: dict) -> tuple[bool, list[str], list[str]]:
    """
    Returns (success, expected_tools, actual_tools).
    success = True if every expected tool appears in actual tools (order-insensitive,
    allows extras — some prompts trigger additional tool calls).
    """
    actual = record.get("tool_calls_made", [])
    expected_tools = expected["expected_tools"]
    if not expected_tools:
        return True, [], actual  # no expectation = pass

    # Check each expected tool is present at least once
    actual_set = set(actual)
    missing = [t for t in expected_tools if t not in actual_set]
    return len(missing) == 0, expected_tools, actual


PASS = "\033[32mPASS\033[0m"
FAIL = "\033[31mFAIL\033[0m"


def main():
    parser = argparse.ArgumentParser(description="Analyze tool-call accuracy from bench log")
    parser.add_argument("log", help="JSONL interaction log from mclaw bench")
    parser.add_argument("prompts", help="bench_prompts_tool_e2e.json with expected_tools")
    parser.add_argument("--no-color", action="store_true", help="Disable ANSI color output")
    args = parser.parse_args()

    check_env()

    pass_label = "PASS" if args.no_color else PASS
    fail_label = "FAIL" if args.no_color else FAIL

    expected_by_id = load_expected(args.prompts)
    records = load_log(args.log)

    if not records:
        print("ERROR: interaction log is empty or not found", file=sys.stderr)
        sys.exit(1)

    print(f"\n{'─'*78}")
    print(f"  Tool-Call E2E Accuracy Report")
    print(f"  log:     {args.log}")
    print(f"  prompts: {args.prompts}")
    print(f"{'─'*78}")
    print(f"  {'ID':>3}  {'Label':<34}  {'Expected':<22}  {'Actual':<22}  {'Result'}")
    print(f"{'─'*78}")

    total = 0
    passed = 0
    failed_turns = []

    for record in records:
        turn_id = record.get("turn_id", 0)
        label = record.get("label", "")[:34]
        expected = expected_by_id.get(turn_id)

        if expected is None:
            # Turn not in prompts file — skip
            continue

        total += 1
        success, exp_tools, act_tools = check_turn(record, expected)

        if success:
            passed += 1
            result_str = pass_label
        else:
            failed_turns.append((turn_id, label, exp_tools, act_tools))
            result_str = fail_label

        exp_str = ",".join(exp_tools) if exp_tools else "(any)"
        act_str = ",".join(act_tools) if act_tools else "(none)"
        # Truncate for display
        exp_str = exp_str[:22]
        act_str = act_str[:22]

        print(f"  {turn_id:>3}  {label:<34}  {exp_str:<22}  {act_str:<22}  {result_str}")

    print(f"{'─'*78}")

    rate = passed / total * 100 if total > 0 else 0.0
    print(f"\n  Total turns evaluated : {total}")
    print(f"  Passed                : {passed}")
    print(f"  Failed                : {total - passed}")
    print(f"  Success rate          : {rate:.1f}%")

    if failed_turns:
        print(f"\n  Failed turns detail:")
        for turn_id, label, exp, act in failed_turns:
            missing = [t for t in exp if t not in set(act)]
            print(f"    Turn {turn_id:>2} ({label})")
            print(f"      expected : {', '.join(exp)}")
            print(f"      actual   : {', '.join(act) if act else '(none)'}")
            print(f"      missing  : {', '.join(missing)}")
    else:
        print(f"\n  All expected tool calls were made. ✓")

    print()

    # Exit 1 if any failures (useful for CI)
    sys.exit(0 if total > 0 and passed == total else 1)


if __name__ == "__main__":
    main()
