# Performance Benchmarking Quick Start

## Overview

This benchmarking suite helps you identify and fix memory/CPU issues in looper.

## Run Benchmarks

```bash
cd /Users/kevin/.kevin/personal-code/looper

# Run all benchmarks (takes ~5 minutes)
make bench-all

# Analyze results and get recommendations
make bench-analyze
```

## What Gets Measured

### 1. Startup (10 runs)
- Startup time
- Initial memory allocation

### 2. Playback (30s)
- Memory usage over time
- CPU percentage
- Peak values

### 3. Memory Profile (2 minutes)
- Memory growth patterns
- Leak detection
- Stability analysis

### 4. CPU Profile (30s)
- CPU usage patterns
- Thread analysis
- Hot code paths (with Instruments)

## Interpret Results

After running `make bench-analyze`, you'll see:

### ✅ Green - Good
- Memory growth < 10%
- CPU usage reasonable
- Stable operation

### 🟡 Yellow - Investigate
- Memory growth 10-20%
- Moderate CPU usage
- Worth profiling

### 🔴 Red - Critical
- Memory growth > 20% (leak!)
- High CPU usage
- Needs immediate attention

## Common Issues to Look For

### High Initial Memory
```
Average initial memory: 150MB
```
**Investigate**: Check what's loaded at startup. Should be < 50MB.

### Memory Growth
```
Growth: 50MB (45%)
```
**Investigate**: 
- Unclosed audio buffers
- FFT buffer accumulation
- Circular references

### High CPU
```
Average CPU: 65%
```
**Investigate**:
- Audio processing efficiency
- TUI render frequency
- FFT computation rate

## Workflow

### 1. Establish Baseline
```bash
make bench-all
make bench-analyze > baseline.txt
```

### 2. Identify Issues
Look at the analysis output:
- Which metric is worst?
- Is there a memory leak?
- Is CPU too high?

### 3. Profile in Detail
```bash
# Watch real-time
make bench-watch

# Get detailed CPU profile
make bench-cpu
open bench/results/*.sample
```

### 4. Fix and Verify
```bash
# After making changes
make build-release
make bench-all
make bench-analyze > after-fix.txt

# Compare
diff baseline.txt after-fix.txt
```

### 5. Iterate
Focus on one issue at a time:
1. Memory leaks (biggest impact)
2. High CPU (affects battery/fans)
3. Startup time (user experience)

## Real-Time Monitoring

While developing, keep this running in a separate terminal:

```bash
make bench-watch
```

Then play/pause/interact with looper and watch the memory/CPU stats live.

## Tips

- Always use release builds: `make build-release`
- Run benchmarks multiple times for consistency
- Close other apps during benchmarking
- Save baseline before any changes
- Focus on one optimization at a time

## Example Session

```bash
# 1. Build release
make build-release

# 2. Run benchmarks
make bench-all

# 3. Analyze
make bench-analyze

# Output shows:
# 🔴 CRITICAL: Memory leak detected (35%)
# → Check for leaked buffers

# 4. Watch real-time
make bench-watch
# (observe memory growing)

# 5. Profile details
make bench-cpu
open bench/results/*.sample
# (Instruments shows FFT buffers accumulating)

# 6. Fix the issue
# ... edit code ...

# 7. Verify fix
make build-release
make bench-memory
make bench-analyze

# Output shows:
# ✅ Memory appears stable (2% growth)
```

## Next Steps

Read [bench/README.md](README.md) for detailed documentation on:
- Individual benchmark commands
- Result file formats
- Advanced profiling techniques
- Integration with Instruments
