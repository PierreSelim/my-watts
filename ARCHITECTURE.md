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
├── config.rs        # TOML config loading, bike presets, output dir resolution
├── gpx.rs           # GPX file parsing
├── smoothing.rs     # Savitzky-Golay implementation
├── analyze.rs       # Per-point metrics and interval bucketing
├── power.rs         # Physics-based power estimation
├── stats.rs         # Speed quartiles and other descriptive statistics
├── csv.rs           # CSV output writers
├── index.rs         # Persistent ride index (JSON) and upsert logic
├── storage.rs       # GPX store: copy analyzed files into ~/.my-watts/gpx, enumerate them
├── tui.rs           # Interactive ratatui terminal plot
└── list_tui.rs      # Interactive ratatui ride-list table
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
Newtype wrapper around `u32` whose constructor enforces "odd and ≥ 3":
```rust
struct WindowSize(u32);

impl WindowSize {
    pub fn new(size: u32) -> Result<Self, GpsAnalyzerError> { /* … */ }
}
```
Validation happens in `::new`, so once you hold a `WindowSize` the invariant is guaranteed even though the inner `u32` is unconstrained at the type level.

### `SavitzkyGolayConfig`
```rust
struct SavitzkyGolayConfig {
    window_size: WindowSize,
    polynomial_degree: u32,
}
```

### `RideIndex` / `RideEntry`
Defined in `index.rs` and serialized as JSON at `config::index_path()` (`~/.my-watts/index.json`).
`RideEntry` holds the metrics shown in the `list` table plus the paths and `ReplayParams` needed to
re-run `analyze` on the ride. `RideIndex::upsert` keys on `analyze_csv_path` (replace, not append)
and keeps entries sorted by `start_timestamp`, most recent first. Load/save tolerate a missing file
(empty index); callers in `analyze` treat any index error as a warning, not a failure.

`build_ride_entry` (in `lib.rs`) assembles a `RideEntry` from a finished `AnalyzeSummary` plus the
source/output paths and `ReplayParams`; it is shared by `analyze` (single ride) and `reindex` (bulk).

### GPX store (`storage.rs`)
`analyze` copies each source GPX into `config::gpx_dir()` (`~/.my-watts/gpx`) via `store_gpx`, keyed
by file stem, and indexes the managed copy so the index is reproducible from the store alone. The
`*_in` variants take an explicit directory and carry the logic (testable without touching the real
home dir); the public `store_gpx` / `stored_gpx_files` wrap them with `config::gpx_dir()`.

## Data Flow

**smooth / power / analyze commands:**
```
load_gpx() → Track
  → smooth_track()  → smoothed Track
    → compute_power() → Vec<PowerPoint>
      → analyze_track() → (Vec<AnalyzePoint>, Vec<IntervalSummary>)
        → write_analyze_csv() / write_intervals_csv()
          → RideIndex::upsert() + save()   (analyze only; warn-on-failure)
```

**plot command** (no CSV output):
```
load_gpx() → smooth → compute_power → analyze_track
  → build_plot_data() → PlotData
    → run_tui()   (ratatui event loop, q/Esc to exit)
```

**list command:**
```
RideIndex::load() → [RideEntry]
  → run_list_tui()   (ratatui table; ↑↓ navigate, q/Esc quit)
    → on Enter: replay the selected entry through analyze_pipeline → run_tui(),
      then return to the list (replay errors surface as an inline status line)
```

**reindex command:**
```
storage::stored_gpx_files()  →  [gpx paths in ~/.my-watts/gpx]
  → reindex_pipeline(): for each → analyze_pipeline() → build_ride_entry()
      (failures collected as `skipped`, not fatal)
    → fresh RideIndex → save_default()   (replaces the previous index.json)
```

The `analyze` command additionally copies its source GPX into the store (`storage::store_gpx`,
non-fatal) before upserting, so `source_gpx_path` in the index points at the managed copy.

### Altitude through the pipeline
`GpsPoint.alt` is smoothed by Savitzky-Golay alongside lat/lon. The smoothed value is stored as `AnalyzePoint.smoothed_alt`. Elevation gain (sum of positive consecutive deltas) is derived from this field both in the CLI summary and in `PlotData.summary`. In the TUI, altitude is normalized to the speed y-range for overlay on the speed chart; the actual min–max is shown in the panel title.

## Error Handling
- All fallible operations return `Result<T, E>`
- Custom error type `GpsAnalyzerError` with variant per error kind
- CLI catches top-level errors and exits with status 1

## Future Extensibility
- `compare` subcommand: visualize original vs smoothed tracks
- Alternative algorithms: Kalman filter under `--algorithm=kalman`
- Map matching: snap to road networks if available

## Testing Strategy
- Unit tests in each module (in `#[cfg(test)]` blocks)
- Integration tests in `tests/` directory
- Golden tests: known inputs with expected outputs
- Property tests: verify smoothing doesn't increase track length beyond expected bounds
