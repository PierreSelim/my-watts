# my-watts: GPS Analyzer for Bike Rides

A Rust tool to analyze and correct GPS drift in GPX files from bike tracking applications.

## Features

- **GPS Drift Correction**: Uses Savitzky-Golay polynomial smoothing to reduce GPS noise while preserving route features
- **Flexible Configuration**: Tunable window size and polynomial degree for smoothing
- **CSV Output**: Exports smoothed coordinates for analysis

## Building

```bash
cargo build --release
```

## Running

### Smooth GPS drift in a GPX file

```bash
cargo run --release -- smooth <INPUT.gpx> [OPTIONS]
```

#### Options

- `-o, --output <FILE>`: Output CSV file (default: `input.smoothed.csv`)
- `--window-size <N>`: Savitzky-Golay window size, must be odd (default: 5)
- `--degree <N>`: Polynomial degree for smoothing (default: 2)
- `-v, --verbose`: Enable verbose output

#### Example

```bash
cargo run --release -- smooth my_ride.gpx -o smoothed.csv --window-size 5 --degree 2 -v
```

## Output Format

The tool outputs a CSV file with the following columns:

- `latitude`: Smoothed latitude in decimal degrees
- `longitude`: Smoothed longitude in decimal degrees
- `altitude`: Smoothed altitude in meters (empty if not in source)
- `timestamp`: Original timestamp (ISO 8601 format, not smoothed)

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

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for design decisions and module structure.

See [SPEC.md](SPEC.md) for feature specifications.
