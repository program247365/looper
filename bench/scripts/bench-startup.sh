#!/usr/bin/env bash
# Measure startup time and initial memory allocation

set -euo pipefail

RESULTS_DIR="$(cd "$(dirname "$0")/../results" && pwd)"
FIXTURE="$(cd "$(dirname "$0")/../../tests/fixtures" && pwd)/sound.mp3"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)

echo "Benchmarking startup performance..."

# Run 10 startup tests
TOTAL_TIME=0
TOTAL_MEM=0
RUNS=10

for i in $(seq 1 $RUNS); do
    echo -n "  Run $i/$RUNS... "
    
    # Start looper in background
    START=$(date +%s.%N)
    timeout 2s cargo run --release -- play --url "$FIXTURE" &> /dev/null &
    PID=$!
    
    # Wait a moment for it to initialize
    sleep 0.5
    
    # Measure memory
    if ps -p $PID > /dev/null 2>&1; then
        MEM=$(ps -o rss= -p $PID 2>/dev/null || echo 0)
        TOTAL_MEM=$((TOTAL_MEM + MEM))
    else
        MEM=0
    fi
    
    # Kill it
    kill -9 $PID 2>/dev/null || true
    wait $PID 2>/dev/null || true
    
    END=$(date +%s.%N)
    DURATION=$(echo "$END - $START" | bc)
    TOTAL_TIME=$(echo "$TOTAL_TIME + $DURATION" | bc)
    
    echo "${DURATION}s, ${MEM}KB"
done

AVG_TIME=$(echo "scale=3; $TOTAL_TIME / $RUNS" | bc)
AVG_MEM=$((TOTAL_MEM / RUNS))
AVG_MEM_MB=$(echo "scale=2; $AVG_MEM / 1024" | bc)

echo ""
echo "Results:"
echo "  Average startup time: ${AVG_TIME}s"
echo "  Average initial memory: ${AVG_MEM}KB (${AVG_MEM_MB}MB)"

# Save results
cat > "$RESULTS_DIR/startup_${TIMESTAMP}.txt" <<EOF
Startup Benchmark Results
=========================
Runs: $RUNS
Average startup time: ${AVG_TIME}s
Average initial memory: ${AVG_MEM}KB (${AVG_MEM_MB}MB)
EOF

echo ""
echo "✓ Results saved to $RESULTS_DIR/startup_${TIMESTAMP}.txt"
