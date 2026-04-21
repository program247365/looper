#!/usr/bin/env bash
# Real-time memory monitoring

set -euo pipefail

echo "=== Real-time Looper Memory Monitor ==="
echo "Press Ctrl+C to stop"
echo ""

# Find running looper process
PID=$(pgrep -x looper || echo "")

if [ -z "$PID" ]; then
    echo "No running looper process found."
    echo "Start looper in another terminal first."
    exit 1
fi

echo "Monitoring looper (PID: $PID)"
echo ""
printf "%-20s %10s %10s %10s %8s\n" "TIME" "RSS (KB)" "RSS (MB)" "VSZ (MB)" "CPU%"
printf "%-20s %10s %10s %10s %8s\n" "----" "--------" "--------" "--------" "----"

while true; do
    if ! ps -p $PID > /dev/null 2>&1; then
        echo ""
        echo "Process ended."
        break
    fi
    
    # Get stats
    STATS=$(ps -o rss=,vsz=,%cpu= -p $PID 2>/dev/null || echo "0 0 0")
    RSS=$(echo $STATS | awk '{print $1}')
    VSZ=$(echo $STATS | awk '{print $2}')
    CPU=$(echo $STATS | awk '{print $3}')
    
    RSS_MB=$(echo "scale=2; $RSS / 1024" | bc)
    VSZ_MB=$(echo "scale=2; $VSZ / 1024" | bc)
    
    TIME=$(date +%H:%M:%S)
    
    printf "%-20s %10d %10s %10s %8s\n" "$TIME" "$RSS" "$RSS_MB" "$VSZ_MB" "$CPU"
    
    sleep 1
done
