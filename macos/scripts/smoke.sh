#!/bin/sh
# Automated launch check for CI (debug build only, uses --demo fake data):
#   1. the app starts and draws (screenshot saved to smoke.png for a human to look at),
#   2. a second copy exits right away (single instance),
#   3. the normal quit path ends the process within 5 s of being triggered.
#
# Usage: macos/scripts/smoke.sh <path to ClaudeUsageWidget.app>
set -eu

APP_BIN="$1/Contents/MacOS/ClaudeUsageWidget"
QUIT_AFTER=20

# Wait up to $2 seconds for PID $1 to exit. Succeeds if it exited.
wait_exit() {
    i=0
    while kill -0 "$1" 2>/dev/null; do
        i=$((i + 1))
        [ "$i" -gt $(($2 * 10)) ] && return 1
        sleep 0.1
    done
}

"$APP_BIN" --demo --quit-after $QUIT_AFTER &
PID=$!
sleep 10

if ! kill -0 "$PID" 2>/dev/null; then
    echo "FAIL: app exited early"
    exit 1
fi
screencapture -x smoke.png || echo "note: screencapture failed (non-fatal)"

"$APP_BIN" --demo &
SECOND=$!
if ! wait_exit "$SECOND" 2; then
    echo "FAIL: second instance did not exit"
    kill "$SECOND" 2>/dev/null || true
    kill "$PID" 2>/dev/null || true
    exit 1
fi
echo "ok: second instance exited"

# Quit fires at QUIT_AFTER s; allow 5 s on top of that (10 s already elapsed).
if ! wait_exit "$PID" $((QUIT_AFTER - 10 + 5)); then
    echo "FAIL: app did not quit within 5 s"
    kill -9 "$PID" 2>/dev/null || true
    exit 1
fi
wait "$PID"
echo "ok: app quit cleanly (exit code $?)"

# Sign-in window: opens claude.ai's login page (screenshot for a human to look at) and
# must quit cleanly even while that window is open. What the page shows is
# informational only; CI can't sign in.
"$APP_BIN" --signin-smoke --quit-after 25 &
PID=$!
sleep 20
screencapture -x signin.png || echo "note: screencapture failed (non-fatal)"
if ! wait_exit "$PID" 10; then
    echo "FAIL: app did not quit while the sign-in window was open"
    kill -9 "$PID" 2>/dev/null || true
    exit 1
fi
echo "ok: quit with sign-in window open"
