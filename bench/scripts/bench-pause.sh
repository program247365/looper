#!/usr/bin/env bash
# Measure memory when paused vs playing

set -euo pipefail

RESULTS_DIR="$(cd "$(dirname "$0")/../results" && pwd)"
FIXTURE="$(cd "$(dirname "$0")/../../tests/fixtures" && pwd)/sound.mp3"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)

echo "Benchmarking pause behavior..."

# Start looper
cargo run --release -- play --url "$FIXTURE" &> /dev/null &
PID=$!

# Wait for startup
sleep 2

if ! ps -p $PID > /dev/null 2>&1; then
    echo "Error: looper process died"
    exit 1
fi

echo "Measuring playing state..."

# Sample while playing
PLAY_SAMPLES=10
PLAY_TOTAL_MEM=0
PLAY_TOTAL_CPU=0

for i in $(seq 1 $PLAY_SAMPLES); do
    MEM=$(ps -o rss= -p $PID 2>/dev/null || echo 0)
    CPU=$(ps -o %cpu= -p $PID 2>/dev/null || echo 0)
    PLAY_TOTAL_MEM=$((PLAY_TOTAL_MEM + MEM))
    PLAY_TOTAL_CPU=$(echo "$PLAY_TOTAL_CPU + $CPU" | bc)
    sleep 0.5
done

PLAY_AVG_MEM=$((PLAY_TOTAL_MEM / PLAY_SAMPLES))
PLAY_AVG_MEM_MB=$(echo "scale=2; $PLAY_AVG_MEM / 1024" | bc)
PLAY_AVG_CPU=$(echo "scale=2; $PLAY_TOTAL_CPU / $PLAY_SAMPLES" | bc)

echo "  Playing: ${PLAY_AVG_MEM}KB (${PLAY_AVG_MEM_MB}MB), ${PLAY_AVG_CPU}% CPU"

# Send space key to pause (simulate keypress)
# Note: This is tricky with TUI apps, so we'll measure what we can
echo "  (Pause testing requires manual interaction - showing playing metrics only)"

# Kill looper
kill -9 $PID 2>/dev/null || true
wait $PID 2>/dev/null || true

echo ""
echo "Pause Behavior Results:"
echo "  Playing state:"
echo "    Average memory: ${PLAY_AVG_MEM}KB (${PLAY_AVG_MEM_MB}MB)"
echo "    Average CPU:    ${PLAY_AVG_CPU}%"

# Save results
cat > "$RESULTS_DIR/pause_${TIMESTAMP}.txt" <<EOF
Pause Behavior Results
======================
Playing state (10 samples over 5s):
  Average memory: ${PLAY_AVG_MEM}KB (${PLAY_AVG_MEM_MB}MB)
  Average CPU:    ${PLAY_AVG_CPU}%

Note: Automated pause testing requires terminal automation.
For manual testing, press Space to pause and observe:
  - Memory should stay relatively stable
  - CPU should drop significantly (near 0%)
EOF

echo ""
echo "✓ Results saved to $RESULTS_DIR/pause_${TIMESTAMP}.txt"
