#!/usr/bin/env bash
# Run Flutter integration tests against a real Android device/emulator.
#
# Required environment variables:
#   MCLAW_SECRET   — absolute path to secrets.db on THIS MACHINE
#                    (the file will be pushed to the device automatically)
#                    The secrets.db must contain an active LLM provider with API key.
#
# Optional:
#   MCLAW_TEST_TARGET — path or glob of test files to run
#                       (default: integration_test/)
#
# Usage:
#   export MCLAW_SECRET=/home/you/mobileclaw/build/secrets.db
#   bash scripts/run_integration_tests.sh
#   bash scripts/run_integration_tests.sh integration_test/camera_test.dart

set -euo pipefail

# ---------------------------------------------------------------------------
# 1. Validate required env vars
# ---------------------------------------------------------------------------

if [[ -z "${MCLAW_SECRET:-}" ]]; then
    echo ""
    echo "ERROR: Missing required environment variable: MCLAW_SECRET"
    echo ""
    echo "Set it before running this script:"
    echo "  export MCLAW_SECRET=/home/\$(whoami)/mobileclaw/build/secrets.db"
    echo ""
    echo "  MCLAW_SECRET — path to your pre-populated secrets.db on this machine"
    echo "                 Must contain an active LLM provider with API key."
    exit 1
fi

# ---------------------------------------------------------------------------
# 2. Validate secrets.db exists and looks like a SQLite database
# ---------------------------------------------------------------------------

if [[ ! -f "$MCLAW_SECRET" ]]; then
    echo ""
    echo "ERROR: MCLAW_SECRET does not point to an existing file."
    echo "  MCLAW_SECRET=$MCLAW_SECRET"
    echo ""
    echo "Expected a SQLite database file (the one mobileclaw uses to store"
    echo "encrypted credentials). Check the path and try again."
    exit 1
fi

# Quick magic-byte check: SQLite files start with "SQLite format 3\000"
if ! head -c 6 "$MCLAW_SECRET" | grep -q "SQLite"; then
    echo ""
    echo "ERROR: MCLAW_SECRET does not appear to be a SQLite database."
    echo "  MCLAW_SECRET=$MCLAW_SECRET"
    echo ""
    echo "This file should be the mobileclaw secrets.db created by the app"
    echo "or the CLI (mclaw). Make sure you're pointing to the right file."
    exit 1
fi

# ---------------------------------------------------------------------------
# 3. Check adb is available
# ---------------------------------------------------------------------------

if ! command -v adb &>/dev/null; then
    echo ""
    echo "ERROR: adb not found in PATH."
    echo ""
    echo "Install Android SDK platform-tools and add them to your PATH:"
    echo "  https://developer.android.com/tools/releases/platform-tools"
    exit 1
fi

# ---------------------------------------------------------------------------
# 4. Check a device is connected
# ---------------------------------------------------------------------------

DEVICE=$(adb devices | awk 'NR>1 && /\tdevice$/ { print $1; exit }')

if [[ -z "$DEVICE" ]]; then
    echo ""
    echo "ERROR: No Android device or emulator is connected."
    echo ""
    echo "Options:"
    echo "  Start an emulator:  flutter emulators --launch <name>"
    echo "  List emulators:     flutter emulators"
    echo "  Physical device:    enable USB debugging and connect via USB"
    echo ""
    echo "Then re-run this script."
    exit 1
fi

echo "Using device: $DEVICE"

# ---------------------------------------------------------------------------
# 5. Push secrets.db to the device
# ---------------------------------------------------------------------------

DEVICE_SECRETS_PATH="/data/local/tmp/mclaw_secrets.db"

echo "Pushing secrets.db → $DEVICE_SECRETS_PATH ..."
adb -s "$DEVICE" push "$MCLAW_SECRET" "$DEVICE_SECRETS_PATH"
echo "  done."

# ---------------------------------------------------------------------------
# 6. Run flutter test with credentials injected via --dart-define
# ---------------------------------------------------------------------------

TEST_TARGET="${1:-integration_test/}"

echo ""
echo "Running: flutter test $TEST_TARGET"
echo "  MCLAW_SECRETS_DB_PATH=$DEVICE_SECRETS_PATH"
echo ""

cd "$(dirname "$0")/.."   # run from the app root

# ---------------------------------------------------------------------------
# 7. Pre-grant CAMERA permission for real-camera tests
#
# Android 6+ requires runtime permission. On a headless emulator there is no
# UI to accept the dialog, so we grant it via adb before launching the tests.
# This is a no-op on targets that don't declare CAMERA in their manifest.
# ---------------------------------------------------------------------------

APP_ID="com.mobileclaw.mobileclaw_app"

# Build and install the debug APK first so pm grant has something to act on.
echo "Building and installing debug APK for permission pre-grant..."
flutter build apk --debug --dart-define="MCLAW_SECRETS_DB_PATH=$DEVICE_SECRETS_PATH" 2>&1 | tail -3
adb -s "$DEVICE" install -r build/app/outputs/flutter-apk/app-debug.apk

echo "Granting CAMERA permission to $APP_ID ..."
adb -s "$DEVICE" shell pm grant "$APP_ID" android.permission.CAMERA 2>/dev/null \
  && echo "  CAMERA granted." \
  || echo "  CAMERA grant skipped (app may not declare it, or already granted)."

flutter test "$TEST_TARGET" \
    --dart-define="MCLAW_SECRETS_DB_PATH=$DEVICE_SECRETS_PATH"
