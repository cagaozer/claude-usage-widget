#!/bin/sh
# Memory-leak check for CI: runs the debug app with demo data for ~11 minutes, samples
# its resident memory every 30 s into leak.csv, and fails if memory grew by more than
# 2 MB after warm-up (sample at minute 2 vs. the peak of the last 10 samples).
#
# Usage: macos/scripts/leakcheck.sh <path to ClaudeUsageWidget.app>
set -eu

APP_BIN="$1/Contents/MacOS/ClaudeUsageWidget"
LIMIT_KB=2048

"$APP_BIN" --demo --quit-after 660 &
PID=$!
echo "seconds,rss_kb" > leak.csv
t=0
while kill -0 "$PID" 2>/dev/null; do
    rss=$(ps -o rss= -p "$PID" | tr -d ' ' || true)
    [ -n "$rss" ] && echo "$t,$rss" >> leak.csv
    sleep 30
    t=$((t + 30))
done
wait "$PID" || true

# Baseline: the sample taken at 120 s. Peak: the highest of the last 10 samples.
base=$(awk -F, '$1 == 120 { print $2 }' leak.csv)
peak=$(tail -n 10 leak.csv | awk -F, 'BEGIN { m = 0 } $2 > m { m = $2 } END { print m }')
growth=$((peak - base))
echo "baseline ${base} KB at 2 min, late peak ${peak} KB, growth ${growth} KB (limit ${LIMIT_KB} KB)"
if [ "$growth" -gt "$LIMIT_KB" ]; then
    echo "FAIL: memory grew by more than 2 MB"
    exit 1
fi
echo "ok: no leak detected"
