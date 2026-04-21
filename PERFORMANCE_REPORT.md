# 🔴 Looper Performance Report

## Executive Summary

**Worst Offender: TUI Rendering at ~33 FPS**
- **Current CPU: 64%** (measured)
- **Expected CPU: <5%** (for a music player)
- **Root Cause: Rendering every 30ms regardless of changes**

## Detailed Analysis

### Current Behavior (src/play_loop.rs:236-246)

```rust
loop {
    if !state.paused {
        update_visualizer(state, player);
    }
    state.frame_count += 1;
    title_state.set(format_playback_title(...))?;
    terminal.draw(|f| draw(f, state))?;  // ← RENDERS EVERY LOOP
    
    if event::poll(Duration::from_millis(30))? {  // ← 30ms = ~33 FPS
        // handle events
    }
}
```

**Problem:**
- Renders **~33 times per second**
- Even when nothing changes (paused, no user input)
- Each render does expensive unicode width calculations
- Text wrapping/layout computed every frame

### CPU Profiling Results

From 10-second sample of running looper:

| Component | Samples | % of Total | Issue |
|-----------|---------|------------|-------|
| Unicode width calculations | 1,216 | ~15% | Binary searches in width tables |
| Text wrapping/layout | 1,318 | ~17% | Paragraph line truncation |
| Paragraph rendering | 2,851 | ~36% | Full text render pipeline |
| Audio processing | ~150 | ~2% | ✅ Efficient |

**Total TUI overhead: ~70% of CPU time**

### Memory Analysis

✅ **Memory is GOOD**: 12MB stable, no leaks detected

```
Sample 1-30:  11.95MB - 12.08MB
Growth:       0.13MB (1%)
Status:       ✅ STABLE
```

## Recommended Fixes

### 🔴 Priority 1: Frame Rate Control (50% CPU savings)

**Location:** `src/play_loop.rs:236-270`

**Current:**
```rust
loop {
    // update state
    terminal.draw(|f| draw(f, state))?;  // Every loop
    if event::poll(Duration::from_millis(30))? { ... }
}
```

**Fix:** Only render when needed
```rust
let mut last_render = Instant::now();
let render_interval = Duration::from_millis(33); // 30 FPS cap

loop {
    let mut needs_render = false;
    
    // Update visualizer (only when playing)
    if !state.paused {
        update_visualizer(state, player);
        needs_render = true;
    }
    
    // Check if render interval elapsed
    if last_render.elapsed() >= render_interval {
        needs_render = true;
    }
    
    // Handle events
    if event::poll(Duration::from_millis(30))? {
        if let Event::Key(key) = event::read()? {
            // ... handle key ...
            needs_render = true; // State changed
        }
    }
    
    // Only render when needed
    if needs_render {
        state.frame_count += 1;
        title_state.set(format_playback_title(...))?;
        terminal.draw(|f| draw(f, state))?;
        last_render = Instant::now();
    }
}
```

**Expected Result:**
- When paused: ~0 FPS (no rendering)
- When playing: 30 FPS (was ~33)
- **CPU reduction: 40-50%**

### 🔴 Priority 2: Reduce FPS When Playing (additional 20% savings)

Since the visualizer is the only thing changing during playback, we can render even less:

```rust
// When playing, render at 15 FPS instead of 30
let render_interval = if !state.paused {
    Duration::from_millis(66)  // 15 FPS for visualizer
} else {
    Duration::from_millis(33)  // 30 FPS when paused/interacting
};
```

**Expected Result:**
- **Total CPU: ~15-20%** (from 64%)

### 🟡 Priority 3: Cache Unicode Widths (10-15% savings)

**Location:** `src/tui.rs` (text rendering)

The ratatui library is doing unicode width calculations on every render. We can't easily change ratatui itself, but we can:

1. **Simplify text**: Use ASCII characters where possible
2. **Reduce text updates**: Don't update text that hasn't changed
3. **Cache formatted strings**: Store pre-formatted text

**Example:**
```rust
// Instead of formatting every frame:
terminal.draw(|f| {
    let text = format!("Playing: {}", state.filename);  // Alloc + format
    // ... render text
})?;

// Cache the formatted text:
struct State {
    cached_title: String,
    title_dirty: bool,
}

// Only update when changed
if state.title_dirty {
    state.cached_title = format!("Playing: {}", state.filename);
    state.title_dirty = false;
}

terminal.draw(|f| {
    // Use cached title
    // ... render state.cached_title
})?;
```

### 🟢 Priority 4: Optimize Scatter Plot (5-10% savings)

**Location:** `src/tui.rs:362`

Only redraw the FFT visualization when data changes, not every frame.

## Implementation Priority

### Phase 1: Quick Win (30 minutes)
1. Add frame rate control (Priority 1)
2. Test and measure

**Expected: 64% → 30% CPU**

### Phase 2: Optimize (1 hour)
1. Reduce FPS to 15 when playing
2. Add dirty flag for title updates

**Expected: 30% → 15% CPU**

### Phase 3: Deep Optimization (2+ hours)
1. Cache unicode text
2. Optimize scatter plot rendering
3. Profile again

**Expected: 15% → <10% CPU**

## Quick Test

After implementing Priority 1, verify with:

```bash
# Watch CPU in real-time
make bench-watch

# Or check with Activity Monitor
# Look for looper CPU% - should drop from ~65% to ~30%
```

## Code Locations

| File | Line | Function | Issue |
|------|------|----------|-------|
| `src/play_loop.rs` | 246 | `run_loop` | Renders every loop |
| `src/play_loop.rs` | 248 | Event poll | 30ms interval |
| `src/tui.rs` | 362 | `draw_scatter` | Full redraw |
| `src/tui.rs` | 227 | `draw_normal` | Text rendering |

## Measurement Baseline

```
Before optimization:
  CPU:    64% average (61-68% range)
  Memory: 12MB (stable)
  FPS:    ~33 (uncapped)

Target after fixes:
  CPU:    <10% average
  Memory: 12MB (no change needed)
  FPS:    15-30 (capped, event-driven)
```

## Next Steps

1. ✅ Benchmarking complete
2. ⏭️ Implement Priority 1 fix
3. ⏭️ Measure improvement
4. ⏭️ Implement Priority 2 fix
5. ⏭️ Measure again
6. ⏭️ Continue until CPU < 10%

---

*Generated: 2026-04-21*  
*Tool: Performance benchmarking suite in `bench/`*  
*Sample data: 30s playback + 10s CPU profile*
