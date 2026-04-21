# Looper Performance Benchmarking

This directory contains performance benchmarking tools to measure and optimize looper's resource usage.

## Quick Start

```bash
# Run all benchmarks
make bench-all

# View results
make bench-results

# Watch memory in real-time
make bench-watch
```

## Individual Benchmarks

### Startup Performance
```bash
make bench-startup
```
Measures:
- Average startup time (10 runs)
- Initial memory allocation

### Playback Performance
```bash
make bench-playback
```
Measures over 30 seconds:
- Memory usage (RSS)
- CPU percentage
- Peak and average values

### Pause Behavior
```bash
make bench-pause
```
Measures:
- Memory/CPU while playing
- Expected behavior when paused

### Memory Profiling
```bash
make bench-memory
```
Deep analysis over 2 minutes:
- Memory growth patterns
- Leak detection
- RSS and VSZ tracking

### CPU Profiling
```bash
make bench-cpu
```
Profiles CPU usage with macOS sampling:
- Average and peak CPU
- Thread analysis
- Sample file for Instruments

### Full Profile
```bash
make bench-profile
```
Comprehensive ~100 second session:
- Initial playback phase
- Extended playback phase
- Final metrics

## Real-Time Monitoring

```bash
make bench-watch
```
Watches a running looper process and displays live memory/CPU stats.

## Results

All results are saved to `bench/results/` with timestamps:
- `*_summary.txt` - Human-readable summaries
- `*.csv` - Detailed time-series data
- `*.sample` - CPU profiling data (open with Instruments)

View latest results:
```bash
make bench-results
```

## Analyzing Results

### Memory Issues to Look For

1. **High Initial Memory**
   - Check startup benchmark
   - Should be < 50MB typically

2. **Memory Growth**
   - Check memory profile
   - >20% growth = potential leak
   - >10% growth = investigate

3. **CPU Usage**
   - Playback should be reasonable
   - Check if CPU drops when paused
   - Look for CPU spikes

### Next Steps

1. **Identify hot spots**: Use CPU profiling to find expensive operations
2. **Check for leaks**: Use memory profiling to detect growth patterns
3. **Optimize**: Focus on areas with highest impact
4. **Measure again**: Verify improvements with benchmarks

## Workflow

```bash
# 1. Establish baseline
make bench-all
make bench-results > baseline.txt

# 2. Make optimizations
# ... edit code ...

# 3. Rebuild and benchmark
make build-release
make bench-all

# 4. Compare results
make bench-results > after-optimization.txt
diff baseline.txt after-optimization.txt

# 5. Monitor in real-time during testing
make bench-watch
```

## Tips

- Always use release builds for accurate measurements
- Run benchmarks multiple times for consistency
- Close other resource-intensive apps during benchmarking
- Use `bench-watch` to verify real-world usage patterns
- Save baseline results before making changes

## Cleaning Up

```bash
# Remove all benchmark results
make bench-clean
```
