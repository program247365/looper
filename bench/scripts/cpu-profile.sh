#!/usr/bin/env bash
# CPU profiling - identify hot code paths

set -euo pipefail

RESULTS_DIR="$(cd "$(dirname "$0")/../results" && pwd)"
FIXTURE="$(cd "$(dirname "$0")/../../tests/fixtures" && pwd)/sound.mp3"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
DURATION=30

echo "CPU profiling (${DURATION}s)..."

# Start looper
cargo run --release -- play --url "$FIXTURE" &> /dev/null &
PID=$!

# Wait for startup
sleep 2

echo "Collecting CPU samples with DTrace/Instruments..."

# Output file
OUTPUT="$RESULTS_DIR/cpu_profile_${TIMESTAMP}.txt"

# Use sample command (macOS profiler)
echo "Running system profiler for ${DURATION}s..."
sample $PID $DURATION -file "$RESULTS_DIR/cpu_profile_${TIMESTAMP}.sample" &> /dev/null &
SAMPLE_PID=$!

# Also collect basic stats
CPU_OUTPUT="$RESULTS_DIR/cpu_stats_${TIMESTAMP}.csv"
echo "timestamp,elapsed_sec,cpu_percent,threads" > "$CPU_OUTPUT"

for i in $(seq 0 $DURATION); do
    if ps -p $PID > /dev/null 2>&1; then
        CPU=$(ps -o %cpu= -p $PID 2>/dev/null || echo 0)
        THREADS=$(ps -o thcount= -p $PID 2>/dev/null || echo 0)
        TS=$(date +%Y-%m-%d\ %H:%M:%S)
        echo "$TS,$i,$CPU,$THREADS" >> "$CPU_OUTPUT"
        echo -n "."
    fi
    sleep 1
done

echo ""

# Wait for sample to finish
wait $SAMPLE_PID 2>/dev/null || true

# Kill looper
kill -9 $PID 2>/dev/null || true
wait $PID 2>/dev/null || true

# Analyze CPU data
TOTAL_CPU=0
SAMPLES=0
MAX_CPU=0
while IFS=, read -r ts elapsed cpu threads; do
    if [ "$elapsed" != "elapsed_sec" ]; then
        TOTAL_CPU=$(echo "$TOTAL_CPU + $cpu" | bc)
        SAMPLES=$((SAMPLES + 1))
        CPU_INT=$(echo "$cpu" | cut -d. -f1)
        [ "$CPU_INT" -gt "$MAX_CPU" ] && MAX_CPU=$CPU_INT
    fi
done < "$CPU_OUTPUT"

AVG_CPU=$(echo "scale=2; $TOTAL_CPU / $SAMPLES" | bc)

echo ""
echo "CPU Profile Results:"
echo "  Duration: ${DURATION}s"
echo "  Average CPU: ${AVG_CPU}%"
echo "  Peak CPU:    ${MAX_CPU}%"
echo ""
echo "  Detailed sample data saved to:"
echo "    cpu_profile_${TIMESTAMP}.sample"
echo "    (Open with: open $RESULTS_DIR/cpu_profile_${TIMESTAMP}.sample)"

# Save summary
cat > "$RESULTS_DIR/cpu_profile_${TIMESTAMP}_summary.txt" <<EOF
CPU Profile Results
===================
Duration: ${DURATION}s
Average CPU: ${AVG_CPU}%
Peak CPU:    ${MAX_CPU}%

To analyze the profile:
  open $RESULTS_DIR/cpu_profile_${TIMESTAMP}.sample

Or use DTrace for more detailed analysis:
  sudo dtrace -n 'profile-997 /pid == $PID/ { @[ustack()] = count(); }'
EOF

echo ""
echo "✓ Results saved to $RESULTS_DIR/cpu_profile_${TIMESTAMP}_summary.txt"
