#!/usr/bin/env bash
# Test script to verify the TUI rendering fix
# Run this manually in a terminal

set -euo pipefail

echo "==================================================================="
echo "Testing TUI Rendering Fix"
echo "==================================================================="
echo ""
echo "This script will:"
echo "  1. Start looper with the fix"
echo "  2. Monitor CPU usage for 30 seconds"
echo "  3. Report results"
echo ""
echo "Press Enter to start looper..."
read

# Start looper in background (requires TTY so must be run in terminal)
echo "Starting looper..."
echo "Press Ctrl+C when you're ready to stop monitoring"
echo ""

# Get the looper PID
sleep 2
PID=$(pgrep -x looper || echo "")

if [ -z "$PID" ]; then
    echo "Error: No looper process found"
    echo "Make sure looper is running in another terminal"
    exit 1
fi

echo "Monitoring looper (PID: $PID) for 30 seconds..."
echo ""

TOTAL_CPU=0
SAMPLES=0
MAX_CPU=0
MIN_CPU=9999

for i in {1..30}; do
    if ! ps -p $PID > /dev/null 2>&1; then
        echo "Process died!"
        exit 1
    fi
    
    MEM=$(ps -o rss= -p $PID 2>/dev/null || echo 0)
    CPU=$(ps -o %cpu= -p $PID 2>/dev/null || echo 0)
    
    echo "Sample $i: ${MEM}KB ($(echo "scale=2; $MEM/1024" | bc)MB), ${CPU}% CPU"
    
    CPU_INT=$(echo "$CPU" | cut -d. -f1)
    TOTAL_CPU=$(echo "$TOTAL_CPU + $CPU" | bc)
    SAMPLES=$((SAMPLES + 1))
    
    if [ "$CPU_INT" -gt "$MAX_CPU" ]; then
        MAX_CPU=$CPU_INT
    fi
    
    if [ "$CPU_INT" -lt "$MIN_CPU" ] && [ "$CPU_INT" -gt 0 ]; then
        MIN_CPU=$CPU_INT
    fi
    
    sleep 1
done

AVG_CPU=$(echo "scale=2; $TOTAL_CPU / $SAMPLES" | bc)
FINAL_MEM=$(ps -o rss= -p $PID 2>/dev/null || echo 0)
FINAL_MEM_MB=$(echo "scale=2; $FINAL_MEM / 1024" | bc)

echo ""
echo "==================================================================="
echo "RESULTS"
echo "==================================================================="
echo "Duration: 30 seconds"
echo "Samples: $SAMPLES"
echo ""
echo "Memory:"
echo "  Final: ${FINAL_MEM}KB (${FINAL_MEM_MB}MB)"
echo ""
echo "CPU:"
echo "  Average: ${AVG_CPU}%"
echo "  Min: ${MIN_CPU}%"
echo "  Max: ${MAX_CPU}%"
echo ""

# Compare to baseline
BASELINE_CPU=66.5

if [ $(echo "$AVG_CPU < 30" | bc) -eq 1 ]; then
    REDUCTION=$(echo "scale=1; 100 * (1 - $AVG_CPU / $BASELINE_CPU)" | bc)
    echo "✅ SUCCESS! CPU reduced from ${BASELINE_CPU}% to ${AVG_CPU}%"
    echo "   Reduction: ${REDUCTION}%"
elif [ $(echo "$AVG_CPU < 50" | bc) -eq 1 ]; then
    REDUCTION=$(echo "scale=1; 100 * (1 - $AVG_CPU / $BASELINE_CPU)" | bc)
    echo "🟡 IMPROVED: CPU reduced from ${BASELINE_CPU}% to ${AVG_CPU}%"
    echo "   Reduction: ${REDUCTION}%"
else
    echo "🔴 NO IMPROVEMENT: CPU still at ${AVG_CPU}% (baseline was ${BASELINE_CPU}%)"
fi
echo ""
