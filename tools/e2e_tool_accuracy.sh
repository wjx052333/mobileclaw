#!/usr/bin/env bash
# e2e_tool_accuracy.sh — Run 30-turn tool-call accuracy bench and report results.
#
# Usage:
#   ./tools/e2e_tool_accuracy.sh
#   MCLAW_DATA_DIR=/path/to/data ./tools/e2e_tool_accuracy.sh
#
# Requires:
#   - MCLAW_DATA_DIR env var (will abort with instructions if missing)
#   - Built release binary: target/release/mclaw
#   - Python 3.6+

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

PROMPTS="${REPO_ROOT}/mobileclaw-cli/docs/bench_prompts_tool_e2e.json"
BINARY="${REPO_ROOT}/target/release/mclaw"
ANALYSIS="${SCRIPT_DIR}/analyze_tool_e2e.py"

# ── Check MCLAW_DATA_DIR ─────────────────────────────────────────────────────
if [ -z "${MCLAW_DATA_DIR:-}" ]; then
    echo ""
    echo "ERROR: MCLAW_DATA_DIR is not set."
    echo ""
    echo "Please set it before running this test:"
    echo "  export MCLAW_DATA_DIR=/home/wjx/agent_eyes/bot/mobileclaw/.claude/worktrees/feat+memory-optimization/build"
    echo ""
    echo "Then re-run:"
    echo "  ./tools/e2e_tool_accuracy.sh"
    echo ""
    exit 2
fi

LOG_DIR="${MCLAW_DATA_DIR}"
LOG_FILE="${LOG_DIR}/bench_tool_e2e.jsonl"

# ── Sanity checks ────────────────────────────────────────────────────────────
if [ ! -f "${BINARY}" ]; then
    echo "ERROR: release binary not found at ${BINARY}"
    echo "Build it first:  cargo build --release -p mobileclaw-cli"
    exit 1
fi

if [ ! -f "${PROMPTS}" ]; then
    echo "ERROR: prompts file not found at ${PROMPTS}"
    exit 1
fi

mkdir -p "${LOG_DIR}"

# ── Run bench ────────────────────────────────────────────────────────────────
echo ""
echo "══════════════════════════════════════════════════════════════════════════════"
echo "  mobileclaw Tool-Call E2E Accuracy Test"
echo "  prompts  : ${PROMPTS}"
echo "  log      : ${LOG_FILE}"
echo "  data dir : ${MCLAW_DATA_DIR}"
echo "══════════════════════════════════════════════════════════════════════════════"
echo ""

MCLAW_DATA_DIR="${MCLAW_DATA_DIR}" \
    "${BINARY}" bench \
    --prompts "${PROMPTS}" \
    --interaction-log "${LOG_FILE}" \
    --turn-delay-ms 1500

echo ""
echo "══════════════════════════════════════════════════════════════════════════════"
echo "  Analysis"
echo "══════════════════════════════════════════════════════════════════════════════"

python3 "${ANALYSIS}" "${LOG_FILE}" "${PROMPTS}"
# analyze_tool_e2e.py exits 0 on 100% success, 1 on any failure
RESULT=$?

if [ "${RESULT}" -eq 0 ]; then
    echo "  ✓ E2E tool-call test PASSED"
else
    echo "  ✗ E2E tool-call test FAILED — see details above"
fi

exit "${RESULT}"
