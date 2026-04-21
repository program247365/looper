# ✅ Benchmark Setup Complete

## What Was Created

### Directory Structure
```
bench/
├── QUICKSTART.md          # Quick start guide
├── README.md              # Full documentation
├── SETUP_COMPLETE.md      # This file
├── scripts/               # Benchmark scripts
│   ├── bench-startup.sh      # Startup performance
│   ├── bench-playback.sh     # Playback performance
│   ├── bench-pause.sh        # Pause behavior
│   ├── memory-profile.sh     # Memory profiling
│   ├── cpu-profile.sh        # CPU profiling
│   ├── full-profile.sh       # Comprehensive profile
│   ├── watch-memory.sh       # Real-time monitoring
│   └── analyze-results.sh    # Result analysis
├── results/               # Benchmark results (generated)
└── data/                  # Test data (empty)
```

### Makefile Targets Added
```bash
make bench-all       # Run all benchmarks
make bench-startup   # Measure startup
make bench-playback  # Measure playback (30s)
make bench-pause     # Measure pause behavior
make bench-memory    # Deep memory profiling (2min)
make bench-cpu       # CPU profiling (30s)
make bench-profile   # Full profile (~100s)
make bench-watch     # Real-time monitoring
make bench-results   # Show latest results
make bench-analyze   # Analyze and identify issues
make bench-clean     # Clean results
```

## Next Steps

### 1. Run Your First Benchmark

```bash
# Quick test (30s playback)
make bench-playback

# Full benchmark suite (~5 minutes)
make bench-all

# Analyze results
make bench-analyze
```

### 2. Watch Real-Time Performance

Open a terminal and run:
```bash
make bench-watch
```

Then in another terminal, use looper normally and watch the memory/CPU stats.

### 3. Identify Memory Hogs

The benchmarks will help you find:

1. **Memory Leaks**
   - Run `make bench-memory` (2 min test)
   - Look for >20% memory growth
   - Check the analysis for warnings

2. **High CPU Usage**
   - Run `make bench-cpu`
   - Open .sample file with Instruments
   - Find hot code paths

3. **Startup Issues**
   - Run `make bench-startup`
   - Should be < 50MB initial memory
   - Check what's loaded at startup

## Workflow Example

```bash
# 1. Establish baseline
make bench-all
make bench-analyze > /tmp/baseline.txt

# 2. Make changes to fix issues
# ... edit src/audio.rs or whatever ...

# 3. Rebuild and test
make build-release
make bench-all
make bench-analyze > /tmp/after-fix.txt

# 4. Compare
diff /tmp/baseline.txt /tmp/after-fix.txt

# 5. Iterate on worst offenders first
```

## What Gets Measured

### Memory Metrics
- **RSS (Resident Set Size)**: Actual RAM used
- **VSZ (Virtual Size)**: Total virtual memory
- **Growth**: Memory increase over time
- **Peak**: Maximum memory used

### CPU Metrics
- **Average CPU**: Mean CPU usage
- **Peak CPU**: Maximum CPU usage
- **Thread count**: Number of active threads

### Performance Indicators
- **Startup time**: How long to initialize
- **Memory stability**: Growth rate over time
- **CPU efficiency**: Usage during playback

## Interpreting Results

### Good Performance
```
Average startup time: 0.2s
Average initial memory: 45KB (43MB)
Average CPU: 15%
Memory growth: 2%
```

### Needs Investigation
```
Average startup time: 1.5s          ← Slow startup
Average initial memory: 150KB (146MB) ← High initial memory
Average CPU: 55%                    ← High CPU
Memory growth: 15%                  ← Growing steadily
```

### Critical Issues
```
Average CPU: 85%                    ← Very high CPU!
Memory growth: 35%                  ← Memory leak!
Peak memory: 500MB                  ← Excessive memory!
```

## Common Issues and Fixes

### Issue: High Memory Usage (~65% CPU mentioned)

**Investigate:**
1. Audio buffer management
2. FFT buffer reuse
3. TUI rendering frequency
4. Unclosed resources

**Profile:**
```bash
make bench-memory   # Check for leaks
make bench-cpu      # Find hot paths
make bench-watch    # Watch in real-time
```

### Issue: Memory Growing Over Time

**Profile:**
```bash
make bench-memory
# Look at the CSV file for growth pattern
cat bench/results/memory_profile_*.csv
```

**Common causes:**
- Audio buffers not being freed
- FFT data accumulating
- Circular references
- Resource handles not closed

### Issue: CPU Too High

**Profile:**
```bash
make bench-cpu
open bench/results/cpu_profile_*.sample
```

**Look for:**
- Frequent FFT computations
- TUI re-rendering too often
- Inefficient audio processing
- Busy-wait loops

## Tips

1. **Always use release builds** for accurate measurements
   ```bash
   make bench-setup  # Builds release automatically
   ```

2. **Run multiple times** for consistency
   - Benchmarks can vary
   - Average 3-5 runs for reliable data

3. **Close other apps** during benchmarking
   - Reduces noise in measurements
   - More accurate CPU/memory stats

4. **Save baselines** before making changes
   ```bash
   make bench-analyze > baseline.txt
   ```

5. **Focus on one issue at a time**
   - Fix memory leaks first (biggest impact)
   - Then optimize CPU usage
   - Then improve startup time

## Documentation

- **Quick Start**: [QUICKSTART.md](QUICKSTART.md)
- **Full Docs**: [README.md](README.md)
- **Results**: `bench/results/`

## Ready to Go!

Start with:
```bash
make bench-all
make bench-analyze
```

This will give you a comprehensive view of looper's performance and identify the memory hogs to attack one by one.

Good luck optimizing! 🚀
