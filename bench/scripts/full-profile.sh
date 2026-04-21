#!/usr/bin/env bash
# Full profiling session with multiple operations

set -euo pipefail

RESULTS_DIR="$(cd "$(dirname "$0")/../results" && pwd)"
FIXTURE="$(cd "$(dirname "$0")/../../tests/fixtures" && pwd)/sound.mp3"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)

echo "=== Full Profiling Session ==="
echo ""

# Start looper
echo "Starting looper..."
cargo run --release -- play --url "$FIXTURE" &> /dev/null &
PID=$!

# Wait for startup
sleep 3

if ! ps -p $PID > /dev/null 2>&1; then
    echo "Error: looper process died"
    exit 1
fi

OUTPUT="$RESULTS_DIR/full_profile_${TIMESTAMP}.csv"
echo "timestamp,phase,elapsed_sec,mem_kb,mem_mb,cpu_percent,threads" > "$OUTPUT"

log_stats() {
    local phase=$1
    local elapsed=$2
    
    if ! ps -p $PID > /dev/null 2>&1; then
        echo "Process died during $phase"
        return 1
    fi
    
    MEM=$(ps -o rss= -p $PID 2>/dev/null || echo 0)
    MEM_MB=$(echo "scale=2; $MEM / 1024" | bc)
    CPU=$(ps -o %cpu= -p $PID 2>/dev/null || echo 0)
    THREADS=$(ps -o thcount= -p $PID 2>/dev/null || echo 0)
    TS=$(date +%Y-%m-%d\ %H:%M:%S)
    
    echo "$TS,$phase,$elapsed,$MEM,$MEM_MB,$CPU,$THREADS" >> "$OUTPUT"
}

# Phase 1: Initial playback (30s)
echo "Phase 1: Initial playback (30s)..."
for i in $(seq 0 30); do
    log_stats "initial_playback" $i
    echo -n "."
    sleep 1
done
echo ""

# Phase 2: Extended playback (60s)
echo "Phase 2: Extended playback (60s)..."
for i in $(seq 0 60); do
    log_stats "extended_playback" $i
    echo -n "."
    sleep 1
done
echo ""

# Phase 3: Final metrics
echo "Phase 3: Final metrics..."
for i in $(seq 0 10); do
    log_stats "final" $i
    sleep 1
done
echo ""

# Kill looper
kill -9 $PID 2>/dev/null || true
wait $PID 2>/dev/null || true

# Analyze results
echo "Analyzing results..."

# Extract stats per phase
analyze_phase() {
    local phase=$1
    grep ",$phase," "$OUTPUT" | awk -F',' '{
        mem += $4
        cpu += $6
        count++
    } END {
        if (count > 0) {
            avg_mem = mem / count
            avg_mem_mb = avg_mem / 1024
            avg_cpu = cpu / count
            printf "  %s:\n", phase
            printf "    Samples: %d\n", count
            printf "    Avg Memory: %.0f KB (%.2f MB)\n", avg_mem, avg_mem_mb
            printf "    Avg CPU: %.2f%%\n", avg_cpu
        }
    }' phase="$phase"
}

echo ""
echo "Full Profile Results:"
analyze_phase "initial_playback"
analyze_phase "extended_playback"
analyze_phase "final"

# Save summary
{
    echo "Full Profile Results"
    echo "===================="
    echo ""
    analyze_phase "initial_playback"
    analyze_phase "extended_playback"
    analyze_phase "final"
    echo ""
    echo "Detailed data: full_profile_${TIMESTAMP}.csv"
} > "$RESULTS_DIR/full_profile_${TIMESTAMP}_summary.txt"

echo ""
echo "✓ Results saved to:"
echo "    Summary: $RESULTS_DIR/full_profile_${TIMESTAMP}_summary.txt"
echo "    Data:    $OUTPUT"
