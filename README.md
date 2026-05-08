# my-watts: GPS Analyzer for Bike Rides

A Rust tool to analyze and correct GPS drift in GPX files from bike tracking applications.

## Features

- **GPS Drift Correction**: Uses Savitzky-Golay polynomial smoothing to reduce GPS noise while preserving route features
- **Power Estimation**: Estimates cycling power output from physics (gravity, rolling resistance, air drag)
- **Ride Analysis**: Produces enriched per-point metrics and interval summaries at multiple time/distance windows; reports total elevation gain
- **Interactive Plot**: Terminal chart showing speed, altitude profile, and power over time
- **Flexible Configuration**: Tunable smoothing, bike presets, and per-user defaults in `config.toml`

## Building

```bash
cargo build --release
```

## Running

### Smooth GPS drift in a GPX file

```bash
cargo run --release -- smooth <INPUT.gpx> [OPTIONS]
```

Options:

- `-o, --output <FILE>`: Output CSV file (default: `input.smoothed.csv`)
- `--window-size <N>`: Savitzky-Golay window size, must be odd (default: 5)
- `--degree <N>`: Polynomial degree for smoothing (default: 2)
- `-v, --verbose`: Enable verbose output

Example:

```bash
cargo run --release -- smooth my_ride.gpx --window-size 5 --degree 2 -v
```

### Estimate power output from a GPX file

```bash
cargo run --release -- power <INPUT.gpx> --rider-weight <KG> --bike <NAME> [OPTIONS]
```

Options:

- `--rider-weight <KG>`: Rider weight in kg (required)
- `--bike <NAME>`: Bike preset — `road`, `gravel`, `mountain`, `hybrid` (required)
- `--bike-weight <KG>`: Bike weight in kg (default: 10.0)
- `--config <FILE>`: Path to `config.toml`
- `-o, --output <FILE>`: Output CSV file (default: `input.power.csv`)
- `-v, --verbose`: Enable verbose output

Example:

```bash
cargo run --release -- power my_ride.gpx --rider-weight 72 --bike gravel -v
```

### Analyze a GPX file (enriched CSV + interval summaries)

```bash
cargo run --release -- analyze <INPUT.gpx> [OPTIONS]
```

Produces two output files:

- `input.analyze.csv` — one row per GPS point with smoothed coordinates, speed, and cumulative distance
- `input.intervals.csv` — aggregated stats at 7 interval sizes (1/5/10/30 min and 1/5/10 km)

Options:

- `--rider-weight <KG>`: Rider weight in kg (default: 75.0 or value from `config.toml`)
- `--bike <NAME>`: Bike preset (default: `road` or value from `config.toml`)
- `--bike-weight <KG>`: Bike weight in kg (default: 10.0)
- `--window-size <N>`: Savitzky-Golay window size, must be odd (default: 5)
- `--degree <N>`: Polynomial degree for smoothing (default: 2)
- `--smooth-window <N>`: Half-window for instant speed and power smoothing — both are computed over `[i-N, i+N]` seconds (default: 5); use 0 for consecutive-point speed
- `--config <FILE>`: Path to `config.toml`
- `-o, --output <FILE>`: Override the per-point CSV path
- `-v, --verbose`: Print ride summary (distance, avg speed, avg power)

Example:

```bash
cargo run --release -- analyze my_ride.gpx --rider-weight 72 --bike gravel -v
```

### Interactive terminal plot

```bash
cargo run --release -- plot <INPUT.gpx> [OPTIONS]
```

Runs the full analysis pipeline and renders a live ratatui chart in the terminal. Press `q` or `Esc` to quit. No output files are written.

The top panel combines speed and altitude:
- **cyan** — instant speed (km/h)
- **green** — cumulative average speed (km/h)
- **light blue** — altitude profile, normalized to the speed y-range; actual altitude range shown in the panel title

The bottom panel shows power:
- **yellow** — smoothed instant power (`--smooth-window` rolling average)
- **green** — cumulative average power over moving time

The status bar shows distance, elapsed time, moving time, average speed, average power, and total elevation gain.

Options:

- `--rider-weight <KG>`: Rider weight in kg (default: 75.0 or value from `config.toml`)
- `--bike <NAME>`: Bike preset (default: `road` or value from `config.toml`)
- `--bike-weight <KG>`: Bike weight in kg (default: 10.0)
- `--window-size <N>`: Savitzky-Golay window size, must be odd (default: 5)
- `--degree <N>`: Polynomial degree for smoothing (default: 2)
- `--smooth-window <N>`: Half-window for instant speed and power smoothing (default: 5)
- `--config <FILE>`: Path to `config.toml`
- `-v, --verbose`: Print loading details

Example:

```bash
cargo run --release -- plot my_ride.gpx --rider-weight 72 --bike gravel
```

## Output Formats

### `smooth` — `input.smoothed.csv`

| Column | Description |
|--------|-------------|
| `latitude` | Smoothed latitude (decimal degrees) |
| `longitude` | Smoothed longitude (decimal degrees) |
| `altitude` | Smoothed altitude in metres (empty if absent) |
| `timestamp` | Original timestamp (ISO 8601, not smoothed) |

### `power` — `input.power.csv`

| Column | Description |
|--------|-------------|
| `timestamp` | ISO 8601 timestamp |
| `power_watts` | Estimated power (W, rounded to 0.1) |
| `speed_kmh` | Speed (km/h, rounded to 0.1) |
| `gradient_pct` | Gradient percentage (rounded to 0.1) |

### `analyze` — `input.analyze.csv`

| Column | Description |
|--------|-------------|
| `timestamp` | ISO 8601 timestamp |
| `seconds_from_start` | Elapsed seconds from first point |
| `raw_lat`, `raw_lon` | Original GPS coordinates |
| `smoothed_lat`, `smoothed_lon` | Savitzky-Golay smoothed coordinates |
| `instant_speed_kmh` | Speed over `[i−N, i+N]` smoothed points where N = `--smooth-window` (km/h) |
| `average_speed_kmh` | Cumulative average speed since start (km/h) |
| `distance_km` | Cumulative distance using smoothed coords (km) |

### `analyze` — `input.intervals.csv`

| Column | Description |
|--------|-------------|
| `interval_type` | `1min`, `5min`, `10min`, `30min`, `1km`, `5km`, `10km` |
| `interval_index` | 0-based bucket index within this interval type |
| `start_timestamp` | ISO 8601 start of interval |
| `end_timestamp` | ISO 8601 end of interval |
| `duration_seconds` | Duration of interval |
| `distance_km` | Distance covered in interval |
| `average_speed_kmh` | Average speed in interval |
| `average_power_watts` | Average power in interval (empty if no power config) |

## Testing

Run all tests:

```bash
cargo test
```

Run specific test:

```bash
cargo test <test_name>
```

View test output:

```bash
cargo test -- --nocapture
```

## Code Quality

- Format code: `cargo fmt`
- Lint code: `cargo clippy`
- Check test coverage: `cargo tarpaulin --out Html`

## Algorithm: Savitzky-Golay Filter

The Savitzky-Golay filter works by fitting a polynomial through a sliding window of GPS points. This approach:

- **Preserves features**: Better than simple moving average for sharp turns and elevation changes
- **Independent measurements**: No dependency on derived velocity (which would be circular with GPS position)
- **Tunable**: Window size and degree control smoothing aggressiveness
- **Offline-friendly**: Processes entire track at once

### Why Not Kalman Filter?

While Kalman filters are excellent for real-time smoothing, they require velocity as an independent measurement. In GPS tracking, instantaneous velocity is derived from position changes, creating a circular dependency. Savitzky-Golay avoids this by smoothing positions directly without modeling velocity.

## Documentation

- See [ARCHITECTURE.md](ARCHITECTURE.md) for design decisions and module structure.
- See [SPEC.md](SPEC.md) for feature specifications.
