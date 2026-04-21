#!/usr/bin/env bash
# Measure memory and CPU during active playback

set -euo pipefail

RESULTS_DIR="$(cd "$(dirname "$0")/../results" && pwd)"
FIXTURE="$(cd "$(dirname "$0")/../../tests/fixtures" && pwd)/sound.mp3"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
DURATION=30  # Run for 30 seconds
SAMPLE_INTERVAL=1  # Sample every second

echo "Benchmarking playback performance (${DURATION}s)..."

# Start looper
cargo run --release -- play --url "$FIXTURE" &> /dev/null &
PID=$!

# Wait for startup
sleep 2

echo "Sampling memory and CPU usage..."

# Output file
OUTPUT="$RESULTS_DIR/playback_${TIMESTAMP}.csv"
echo "timestamp,elapsed_sec,mem_kb,mem_mb,cpu_percent" > "$OUTPUT"

START_TIME=$(date +%s)
MAX_MEM=0
TOTAL_MEM=0
MAX_CPU=0
TOTAL_CPU=0
SAMPLES=0

for i in $(seq 0 $((DURATION - 1))); do
    if ! ps -p $PID > /dev/null 2>&1; then
        echo "Error: looper process died"
        exit 1
    fi
    
    # Get memory (RSS in KB)
    MEM=$(ps -o rss= -p $PID 2>/dev/null || echo 0)
    MEM_MB=$(echo "scale=2; $MEM / 1024" | bc)
    
    # Get CPU percentage
    CPU=$(ps -o %cpu= -p $PID 2>/dev/null || echo 0)
    
    # Track stats
    if [ "$MEM" -gt "$MAX_MEM" ]; then
        MAX_MEM=$MEM
    fi
    TOTAL_MEM=$((TOTAL_MEM + MEM))
    
    CPU_INT=$(echo "$CPU" | cut -d. -f1)
    if [ "$CPU_INT" -gt "$MAX_CPU" ]; then
        MAX_CPU=$CPU_INT
    fi
    TOTAL_CPU=$(echo "$TOTAL_CPU + $CPU" | bc)
    
    SAMPLES=$((SAMPLES + 1))
    
    # Log
    TIMESTAMP=$(date +%Y-%m-%d\ %H:%M:%S)
    echo "$TIMESTAMP,$i,$MEM,$MEM_MB,$CPU" >> "$OUTPUT"
    
    echo -n "."
    sleep $SAMPLE_INTERVAL
done

echo ""

# Kill looper
kill -9 $PID 2>/dev/null || true
wait $PID 2>/dev/null || true

# Calculate averages
AVG_MEM=$((TOTAL_MEM / SAMPLES))
AVG_MEM_MB=$(echo "scale=2; $AVG_MEM / 1024" | bc)
MAX_MEM_MB=$(echo "scale=2; $MAX_MEM / 1024" | bc)

AVG_CPU=$(echo "scale=2; $TOTAL_CPU / $SAMPLES" | bc)

echo ""
echo "Playback Performance Results:"
echo "  Duration: ${DURATION}s"
echo "  Samples: $SAMPLES"
echo "  Memory:"
echo "    Average: ${AVG_MEM}KB (${AVG_MEM_MB}MB)"
echo "    Peak:    ${MAX_MEM}KB (${MAX_MEM_MB}MB)"
echo "  CPU:"
echo "    Average: ${AVG_CPU}%"
echo "    Peak:    ${MAX_CPU}%"

# Save summary
cat > "$RESULTS_DIR/playback_${TIMESTAMP}_summary.txt" <<EOF
Playback Performance Results
============================
Duration: ${DURATION}s
Samples: $SAMPLES

Memory:
  Average: ${AVG_MEM}KB (${AVG_MEM_MB}MB)
  Peak:    ${MAX_MEM}KB (${MAX_MEM_MB}MB)

CPU:
  Average: ${AVG_CPU}%
  Peak:    ${MAX_CPU}%

Detailed data: playback_${TIMESTAMP}.csv
EOF

echo ""
echo "✓ Results saved to:"
echo "    Summary: $RESULTS_DIR/playback_${TIMESTAMP}_summary.txt"
echo "    Data:    $OUTPUT"
