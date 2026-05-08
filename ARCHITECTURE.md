# Architecture: my-watts

## Design Philosophy
- **Strong typing**: Uses Rust's type system to make invalid states unrepresentable (e.g., `WindowSize` guarantees odd integers)
- **Functional composition**: Pure functions for transformations (parse → smooth → write)
- **Comprehensive testing**: All public APIs and complex logic covered

## Module Structure

```
src/
├── lib.rs           # Public API: GpsPoint, Track, AnalyzePoint, error types
├── main.rs          # CLI entry point, command dispatch
├── cli.rs           # Argument parsing with clap
├── gpx.rs           # GPX file parsing
├── smoothing.rs     # Savitzky-Golay implementation
├── analyze.rs       # Per-point metrics and interval bucketing
├── power.rs         # Physics-based power estimation
├── csv.rs           # CSV output writers
└── tui.rs           # Interactive ratatui terminal plot
```

## Key Types

### `GpsPoint`
```rust
struct GpsPoint {
    lat: f64,              // decimal degrees
    lon: f64,              // decimal degrees
    alt: Option<f64>,      // meters, may be missing
    timestamp: DateTime,   // ISO 8601
}
```

### `Track`
Collection of ordered `GpsPoint`s representing a continuous ride.

### `AnalyzePoint`
One enriched record per GPS point, produced by `analyze::analyze_track`:
```rust
struct AnalyzePoint {
    timestamp: DateTime,
    seconds_from_start: f64,
    moving_seconds_from_start: f64,
    raw_lat: f64, raw_lon: f64,
    smoothed_lat: f64, smoothed_lon: f64,
    smoothed_alt: Option<f64>,      // from Savitzky-Golay smoothed track
    instant_speed_kmh: f64,
    average_speed_kmh: f64,
    distance_km: f64,
    power_smooth_watts: Option<f64>,
    cumulative_energy_kj: Option<f64>,
}
```

### `WindowSize`
Newtype wrapper ensuring odd integers >= 3:
```rust
struct WindowSize(NonZeroU32);
```
This makes it impossible to construct invalid window sizes at the type level.

### `SavitzkyGolayConfig`
```rust
struct SavitzkyGolayConfig {
    window_size: WindowSize,
    polynomial_degree: u32,
}
```

## Data Flow

**smooth / power / analyze commands:**
```
load_gpx() → Track
  → smooth_track()  → smoothed Track
    → compute_power() → Vec<PowerPoint>
      → analyze_track() → (Vec<AnalyzePoint>, Vec<IntervalSummary>)
        → write_analyze_csv() / write_intervals_csv()
```

**plot command** (no CSV output):
```
load_gpx() → smooth → compute_power → analyze_track
  → build_plot_data() → PlotData
    → run_tui()   (ratatui event loop, q/Esc to exit)
```

### Altitude through the pipeline
`GpsPoint.alt` is smoothed by Savitzky-Golay alongside lat/lon. The smoothed value is stored as `AnalyzePoint.smoothed_alt`. Elevation gain (sum of positive consecutive deltas) is derived from this field both in the CLI summary and in `PlotData.summary`. In the TUI, altitude is normalized to the speed y-range for overlay on the speed chart; the actual min–max is shown in the panel title.

## Error Handling
- All fallible operations return `Result<T, E>`
- Custom error type `GpsAnalyzerError` with variant per error kind
- CLI catches top-level errors and exits with status 1

## Why Savitzky-Golay?
1. **No velocity dependency**: Speed is derived from position, creating circular dependency with Kalman filter
2. **Feature preservation**: Better than simple moving average for preserving route features
3. **Offline-friendly**: Processes full track at once, no streaming needed
4. **Tunable**: Window size and degree control smoothing aggressiveness
5. **Proven**: Well-established in signal processing

## Future Extensibility
- `compare` subcommand: visualize original vs smoothed tracks
- Alternative algorithms: Kalman filter under `--algorithm=kalman`
- Map matching: snap to road networks if available

## Testing Strategy
- Unit tests in each module (in `#[cfg(test)]` blocks)
- Integration tests in `tests/` directory
- Golden tests: known inputs with expected outputs
- Property tests: verify smoothing doesn't increase track length beyond expected bounds
