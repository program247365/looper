#!/usr/bin/env bash
# Analyze benchmark results to identify memory hogs

set -euo pipefail

RESULTS_DIR="$(cd "$(dirname "$0")/../results" && pwd)"

echo "=== Looper Performance Analysis ==="
echo ""

if [ ! -d "$RESULTS_DIR" ] || [ -z "$(ls -A "$RESULTS_DIR" 2>/dev/null)" ]; then
    echo "No benchmark results found."
    echo "Run 'make bench-all' first."
    exit 1
fi

# Find latest results
LATEST_STARTUP=$(ls -t "$RESULTS_DIR"/startup_*.txt 2>/dev/null | head -1 || echo "")
LATEST_PLAYBACK=$(ls -t "$RESULTS_DIR"/playback_*_summary.txt 2>/dev/null | head -1 || echo "")
LATEST_MEMORY=$(ls -t "$RESULTS_DIR"/memory_profile_*_summary.txt 2>/dev/null | head -1 || echo "")
LATEST_CPU=$(ls -t "$RESULTS_DIR"/cpu_profile_*_summary.txt 2>/dev/null | head -1 || echo "")

# Extract key metrics
extract_metric() {
    local file=$1
    local pattern=$2
    grep "$pattern" "$file" 2>/dev/null | head -1 || echo ""
}

echo "┌─────────────────────────────────────────────────────────────┐"
echo "│                      MEMORY ANALYSIS                        │"
echo "└─────────────────────────────────────────────────────────────┘"
echo ""

if [ -n "$LATEST_STARTUP" ]; then
    echo "1. STARTUP MEMORY"
    extract_metric "$LATEST_STARTUP" "Average initial memory"
    echo ""
fi

if [ -n "$LATEST_PLAYBACK" ]; then
    echo "2. PLAYBACK MEMORY (30s average)"
    extract_metric "$LATEST_PLAYBACK" "Average:"
    extract_metric "$LATEST_PLAYBACK" "Peak:"
    echo ""
fi

if [ -n "$LATEST_MEMORY" ]; then
    echo "3. MEMORY OVER TIME (2min profile)"
    extract_metric "$LATEST_MEMORY" "Initial:"
    extract_metric "$LATEST_MEMORY" "Final:"
    extract_metric "$LATEST_MEMORY" "Growth:"
    extract_metric "$LATEST_MEMORY" "Average:"
    echo ""
    
    # Check for warnings
    if grep -q "WARNING" "$LATEST_MEMORY" 2>/dev/null; then
        echo "⚠️  MEMORY LEAK DETECTED!"
        grep "WARNING" "$LATEST_MEMORY"
        echo ""
    elif grep -q "NOTICE" "$LATEST_MEMORY" 2>/dev/null; then
        echo "⚠️  Elevated memory growth detected"
        grep "NOTICE" "$LATEST_MEMORY"
        echo ""
    fi
fi

echo "┌─────────────────────────────────────────────────────────────┐"
echo "│                       CPU ANALYSIS                          │"
echo "└─────────────────────────────────────────────────────────────┘"
echo ""

if [ -n "$LATEST_PLAYBACK" ]; then
    echo "CPU USAGE"
    extract_metric "$LATEST_PLAYBACK" "Average:" | grep "CPU" || true
    extract_metric "$LATEST_PLAYBACK" "Peak:" | grep "CPU" || true
    echo ""
fi

echo "┌─────────────────────────────────────────────────────────────┐"
echo "│                    RECOMMENDATIONS                          │"
echo "└─────────────────────────────────────────────────────────────┘"
echo ""

# Analyze and provide recommendations
if [ -n "$LATEST_MEMORY" ]; then
    GROWTH=$(grep "Growth:" "$LATEST_MEMORY" | grep -oE '[0-9.]+%' || echo "0%")
    GROWTH_NUM=$(echo "$GROWTH" | sed 's/%//')
    
    if [ $(echo "$GROWTH_NUM > 20" | bc 2>/dev/null || echo 0) -eq 1 ]; then
        echo "🔴 CRITICAL: Memory leak detected (${GROWTH})"
        echo "   → Check for leaked buffers or circular references"
        echo "   → Look for unclosed resources in audio processing"
        echo "   → Review FFT buffer management"
        echo ""
    elif [ $(echo "$GROWTH_NUM > 10" | bc 2>/dev/null || echo 0) -eq 1 ]; then
        echo "🟡 WARNING: Elevated memory growth (${GROWTH})"
        echo "   → Monitor over longer periods"
        echo "   → Check buffer reuse patterns"
        echo ""
    else
        echo "✅ Memory appears stable (${GROWTH} growth)"
        echo ""
    fi
fi

if [ -n "$LATEST_PLAYBACK" ]; then
    AVG_CPU=$(grep "Average:" "$LATEST_PLAYBACK" | grep "CPU" | grep -oE '[0-9.]+%' | head -1 || echo "0%")
    AVG_CPU_NUM=$(echo "$AVG_CPU" | sed 's/%//' | cut -d. -f1)
    
    if [ "$AVG_CPU_NUM" -gt 50 ]; then
        echo "🔴 HIGH CPU USAGE: ${AVG_CPU}"
        echo "   → Profile with: open $(ls -t "$RESULTS_DIR"/*.sample 2>/dev/null | head -1)"
        echo "   → Check audio processing efficiency"
        echo "   → Review FFT computation frequency"
        echo ""
    elif [ "$AVG_CPU_NUM" -gt 30 ]; then
        echo "🟡 MODERATE CPU USAGE: ${AVG_CPU}"
        echo "   → Room for optimization"
        echo "   → Check TUI render frequency"
        echo ""
    else
        echo "✅ CPU usage is reasonable (${AVG_CPU})"
        echo ""
    fi
fi

echo "┌─────────────────────────────────────────────────────────────┐"
echo "│                    NEXT STEPS                               │"
echo "└─────────────────────────────────────────────────────────────┘"
echo ""
echo "1. Review detailed CSV data:"
echo "   - $(ls -t "$RESULTS_DIR"/*.csv 2>/dev/null | head -1 || echo "No CSV files")"
echo ""
echo "2. Watch real-time memory:"
echo "   make bench-watch"
echo ""
echo "3. Profile with Instruments:"
if [ -n "$(ls -t "$RESULTS_DIR"/*.sample 2>/dev/null | head -1)" ]; then
    echo "   open $(ls -t "$RESULTS_DIR"/*.sample 2>/dev/null | head -1)"
else
    echo "   make bench-cpu"
fi
echo ""
echo "4. Re-run benchmarks after changes:"
echo "   make bench-all"
echo ""
