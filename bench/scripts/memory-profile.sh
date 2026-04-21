#!/usr/bin/env bash
# Deep memory profiling - look for leaks and growth patterns

set -euo pipefail

RESULTS_DIR="$(cd "$(dirname "$0")/../results" && pwd)"
FIXTURE="$(cd "$(dirname "$0")/../../tests/fixtures" && pwd)/sound.mp3"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
DURATION=120  # Run for 2 minutes
SAMPLE_INTERVAL=2  # Sample every 2 seconds

echo "Deep memory profiling (${DURATION}s)..."
echo "Looking for memory leaks and growth patterns..."

# Start looper
cargo run --release -- play --url "$FIXTURE" &> /dev/null &
PID=$!

# Wait for startup
sleep 3

echo "Collecting memory samples..."

# Output file
OUTPUT="$RESULTS_DIR/memory_profile_${TIMESTAMP}.csv"
echo "timestamp,elapsed_sec,rss_kb,vsz_kb,rss_mb,vsz_mb" > "$OUTPUT"

START_TIME=$(date +%s)
SAMPLES=()
TIMES=()

for i in $(seq 0 $SAMPLE_INTERVAL $((DURATION))); do
    if ! ps -p $PID > /dev/null 2>&1; then
        echo "Error: looper process died"
        exit 1
    fi
    
    # Get detailed memory stats
    MEM_STATS=$(ps -o rss=,vsz= -p $PID 2>/dev/null || echo "0 0")
    RSS=$(echo $MEM_STATS | awk '{print $1}')
    VSZ=$(echo $MEM_STATS | awk '{print $2}')
    
    RSS_MB=$(echo "scale=2; $RSS / 1024" | bc)
    VSZ_MB=$(echo "scale=2; $VSZ / 1024" | bc)
    
    SAMPLES+=($RSS)
    TIMES+=($i)
    
    # Log
    TS=$(date +%Y-%m-%d\ %H:%M:%S)
    echo "$TS,$i,$RSS,$VSZ,$RSS_MB,$VSZ_MB" >> "$OUTPUT"
    
    echo -n "."
    sleep $SAMPLE_INTERVAL
done

echo ""

# Kill looper
kill -9 $PID 2>/dev/null || true
wait $PID 2>/dev/null || true

# Analyze memory growth
FIRST_MEM=${SAMPLES[0]}
LAST_MEM=${SAMPLES[-1]}
GROWTH=$((LAST_MEM - FIRST_MEM))
GROWTH_MB=$(echo "scale=2; $GROWTH / 1024" | bc)
GROWTH_PERCENT=$(echo "scale=2; ($GROWTH / $FIRST_MEM) * 100" | bc)

# Calculate average
TOTAL=0
for mem in "${SAMPLES[@]}"; do
    TOTAL=$((TOTAL + mem))
done
AVG=$((TOTAL / ${#SAMPLES[@]}))
AVG_MB=$(echo "scale=2; $AVG / 1024" | bc)

# Find min/max
MIN=${SAMPLES[0]}
MAX=${SAMPLES[0]}
for mem in "${SAMPLES[@]}"; do
    [ $mem -lt $MIN ] && MIN=$mem
    [ $mem -gt $MAX ] && MAX=$mem
done
MIN_MB=$(echo "scale=2; $MIN / 1024" | bc)
MAX_MB=$(echo "scale=2; $MAX / 1024" | bc)

echo ""
echo "Memory Profile Results:"
echo "  Duration: ${DURATION}s"
echo "  Samples: ${#SAMPLES[@]}"
echo ""
echo "  Memory (RSS):"
echo "    Initial: ${FIRST_MEM}KB"
echo "    Final:   ${LAST_MEM}KB"
echo "    Growth:  ${GROWTH}KB (${GROWTH_MB}MB, ${GROWTH_PERCENT}%)"
echo "    Average: ${AVG}KB (${AVG_MB}MB)"
echo "    Min:     ${MIN}KB (${MIN_MB}MB)"
echo "    Max:     ${MAX}KB (${MAX_MB}MB)"

# Detect potential leak
if [ $(echo "$GROWTH_PERCENT > 20" | bc) -eq 1 ]; then
    echo ""
    echo "⚠️  WARNING: Memory grew by ${GROWTH_PERCENT}% - potential leak!"
elif [ $(echo "$GROWTH_PERCENT > 10" | bc) -eq 1 ]; then
    echo ""
    echo "⚠️  NOTICE: Memory grew by ${GROWTH_PERCENT}% - worth investigating"
else
    echo ""
    echo "✓ Memory appears stable (${GROWTH_PERCENT}% growth)"
fi

# Save summary
cat > "$RESULTS_DIR/memory_profile_${TIMESTAMP}_summary.txt" <<EOF
Memory Profile Results
======================
Duration: ${DURATION}s
Samples: ${#SAMPLES[@]}

Memory (RSS):
  Initial: ${FIRST_MEM}KB
  Final:   ${LAST_MEM}KB
  Growth:  ${GROWTH}KB (${GROWTH_MB}MB, ${GROWTH_PERCENT}%)
  Average: ${AVG}KB (${AVG_MB}MB)
  Min:     ${MIN}KB (${MIN_MB}MB)
  Max:     ${MAX}KB (${MAX_MB}MB)

Detailed data: memory_profile_${TIMESTAMP}.csv
EOF

echo ""
echo "✓ Results saved to:"
echo "    Summary: $RESULTS_DIR/memory_profile_${TIMESTAMP}_summary.txt"
echo "    Data:    $OUTPUT"
