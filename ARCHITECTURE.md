# Architecture: my-watts

## Design Philosophy
- **Strong typing**: Uses Rust's type system to make invalid states unrepresentable (e.g., `WindowSize` guarantees odd integers)
- **Functional composition**: Pure functions for transformations (parse → smooth → write)
- **Comprehensive testing**: All public APIs and complex logic covered

## Module Structure

```
src/
├── lib.rs           # Public API
├── main.rs          # CLI entry point
├── cli.rs           # Argument parsing with clap
├── gpx.rs           # GPX file parsing
├── smoothing.rs     # Savitzky-Golay implementation
└── csv.rs           # CSV output writer
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

```
main() 
  → parse_args() 
    → load_gpx() → Track
      → smooth_track(config) → SmoothedTrack
        → write_csv() → stdout/file
```

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
- `analyze` subcommand: calculate distance, elevation gain, speed statistics
- `compare` subcommand: visualize original vs smoothed tracks
- Alternative algorithms: Kalman filter under `--algorithm=kalman`
- Map matching: snap to road networks if available

## Testing Strategy
- Unit tests in each module (in `#[cfg(test)]` blocks)
- Integration tests in `tests/` directory
- Golden tests: known inputs with expected outputs
- Property tests: verify smoothing doesn't increase track length beyond expected bounds
