# Specification: my-watts GPS Analyzer

## Feature 1: GPS Drift Correction

### Problem
GPS/GNSS receivers in bike tracking applications suffer from instantaneous position drift, causing noisy coordinates that don't accurately reflect the actual route. This results in:
- Spurious distance calculations
- Inaccurate elevation profiles
- Difficult-to-read route visualizations

### Solution
Apply Savitzky-Golay smoothing to correct position drift while preserving route features (sharp turns, elevation changes).

### Algorithm Choice
**Primary**: Savitzky-Golay filter (polynomial smoothing)
**Fallback**: Kalman filter (if Savitzky-Golay proves insufficient)

The Savitzky-Golay filter works by fitting a polynomial through a sliding window of points. This approach:
- Preserves sharp features (turns, climbs) better than moving average
- Doesn't require velocity as an independent measurement
- Is well-suited for offline GPX analysis
- Works independently on latitude, longitude, and altitude

### CLI Interface

```
my-watts smooth <INPUT> [OPTIONS]

Arguments:
  INPUT                  GPX file to smooth

Options:
  -o, --output <FILE>    Output CSV file (default: input.smoothed.csv)
  --window-size <N>      Savitzky-Golay window size, must be odd (default: 5)
  --degree <N>           Polynomial degree for smoothing (default: 2)
  -v, --verbose          Enable verbose output
  -h, --help             Print help
```

### Input/Output Formats
- **Input**: GPX 1.1 format (standard bike tracking format)
- **Output**: CSV with columns: `latitude,longitude,altitude,timestamp`
- Timestamps preserved exactly (not smoothed)
- All coordinates in decimal degrees, altitude in meters

### Behavior
1. Parse GPX file, extract all trackpoints
2. Apply Savitzky-Golay smoothing to lat/lon/alt independently
3. Write smoothed coordinates to CSV
4. Report summary: number of points, window size used

### Error Handling
- Invalid GPX format → error with helpful message
- File not found → error
- Window size too large → error (must be < track length)
- Window size even or < 3 → error (must be odd and >= 3)
- Empty track → error

### Testing
- Unit tests for smoothing algorithm (verify polynomial fitting)
- Integration tests: read GPX → smooth → write CSV
- Edge cases: minimal points (1-3), large window size, exact boundaries
- Golden test: known input GPX produces expected smoothed output

---

## Feature 2: Power Estimation

### Problem

Raw GPX files contain position and time but no power data. Estimating wattage from first principles (physics of cycling) enables training analysis without a power meter.

### Solution

Compute power at each GPS segment using the standard cycling power model, driven by speed, gradient, rider/bike mass, and configurable aerodynamic and rolling-resistance parameters.

### Physics Model

Power is decomposed into three resistive forces:

```
P = (Fg + Fr + Fa) × v
```

| Component | Formula | Description |
|-----------|---------|-------------|
| Gravitational | `Fg = (M+m) × g × sin(arctan(slope))` | Climbing resistance |
| Rolling | `Fr = (M+m) × g × cos(arctan(slope)) × Crr` | Tyre/surface friction |
| Aerodynamic | `Fa = 0.5 × CdA × ρ × v²` | Air drag |

Constants: `g = 9.80665 m/s²`, `ρ = 1.225 kg/m³` (sea-level air density).  
Power is clamped to `≥ 0` (coasting/descending contributes zero).

### Configuration

Bike parameters are stored in `config.toml` (TOML format).  
Default location: `%APPDATA%\my-watts\config.toml` (Windows) or `~/.config/my-watts/config.toml` (Unix).  
Falls back to built-in defaults if no file is found.

```toml
[[bikes]]
name = "road"
crr  = 0.004   # rolling resistance coefficient
cda  = 0.32    # drag area Cd×A in m²

[[bikes]]
name = "gravel"
crr  = 0.006
cda  = 0.40
```

Built-in bike presets:

| Name | Crr | CdA (m²) |
|------|-----|---------|
| road | 0.004 | 0.32 |
| gravel | 0.006 | 0.40 |
| mountain | 0.012 | 0.57 |
| hybrid | 0.008 | 0.46 |

### CLI Interface

```
my-watts power <INPUT> [OPTIONS]

Arguments:
  INPUT                       GPX file to analyse

Options:
  --rider-weight <KG>         Rider weight in kg (required)
  --bike-weight <KG>          Bike weight in kg (default: 10.0)
  --bike <NAME>               Bike preset name, e.g. road, gravel (required)
  --config <FILE>             Path to config.toml (default: platform config dir)
  -o, --output <FILE>         Output CSV file (default: input.power.csv)
  -v, --verbose               Enable verbose output
  -h, --help                  Print help
```

### Output Format

CSV file with one row per GPS segment (consecutive point pair):

```
timestamp,power_watts,speed_kmh,gradient_pct
```

- `power_watts` — rounded to 0.1 W
- `speed_kmh` — rounded to 0.1 km/h
- `gradient_pct` — rounded to 0.1 %

### Error Handling

- Track with fewer than 2 points → error
- Non-positive time delta between consecutive points → error
- Bike name not found in config → error with bike name
- Invalid config file → error

### Testing

- Unit tests for each force component and the combined formula
- Property tests: heavier rider needs more power on climbs; gravel bike needs more power than road on flats
- Unit test for Haversine distance accuracy (Paris–London ±5 km)
- Integration test: parse GPX → compute power → write CSV

---

## Feature 3: Ride Analysis

### Problem

The `smooth` and `power` commands produce separate outputs, and neither provides a combined view of a ride's key metrics at a glance. Riders want a single enriched dataset that includes both raw and smoothed coordinates, instant and average speed, cumulative distance, aggregated summaries across standard time and distance windows, and an immediate visual overview without leaving the terminal.

### Solution

The `analyze` command processes a GPX file in one pass: it smooths the track, computes per-point metrics, estimates power, groups results into interval buckets at 7 predefined window sizes (1/5/10/30 min and 1/5/10 km), writes two CSV files, prints a one-line ride summary, and opens an interactive terminal plot. Pass `--no-plot` to skip the plot and exit after writing the CSVs.

### CLI Interface

```
my-watts analyze <INPUT> [OPTIONS]

Arguments:
  INPUT                       GPX file to analyse

Options:
  -o, --output <FILE>         Output CSV for per-point data (default: input.analyze.csv)
  --window-size <N>           Savitzky-Golay window size, must be odd (default: 5)
  --degree <N>                Polynomial degree for smoothing (default: 2)
  --smooth-window <N>         Half-window for instant speed and power smoothing (default: 5)
  --rider-weight <KG>         Rider weight in kg (default: config or 75.0)
  --bike-weight <KG>          Bike weight in kg (default: 10.0)
  --bike <NAME>               Bike preset name (default: config or "road")
  --config <FILE>             Path to config.toml (default: platform config dir)
  --no-plot                   Write CSVs and print summary without opening the terminal plot
  -v, --verbose               Enable verbose output
  -h, --help                  Print help
```

The intervals output path is always `{INPUT_STEM}.intervals.csv` and cannot be overridden separately.

### Configuration Defaults

`config.toml` may contain two new optional fields that set the defaults for `--rider-weight` and `--bike` when those flags are omitted:

```toml
default_rider_weight_kg = 75.0
default_bike = "road"
```

These fields are optional; built-in fallbacks are 75.0 kg and `"road"` respectively.

### Output: `input.analyze.csv`

One row per GPS point, in track order:

| Column | Description |
|--------|-------------|
| `timestamp` | ISO 8601 timestamp |
| `seconds_from_start` | Elapsed seconds from first point (rounded to 0.01) |
| `raw_lat`, `raw_lon` | Original GPS coordinates |
| `smoothed_lat`, `smoothed_lon` | Savitzky-Golay smoothed coordinates |
| `instant_speed_kmh` | Speed computed over `[i−n, i+n]` smoothed points where `n = --smooth-window`, clamped at track boundaries. 0.0 for the first point. Rounded to 0.1 km/h. |
| `average_speed_kmh` | Cumulative average moving speed since start in km/h: `distance_km / (moving_seconds / 3600)`. 0.0 for the first point and while stationary. Rounded to 0.1. Matches what Strava and bike computers report. |
| `distance_km` | Cumulative distance using smoothed coords in km (rounded to 0.001) |
| `power_smooth_watts` | Centered rolling average power over `[i−n, i+n]` seconds where `n = --smooth-window` (rounded to 0.1); empty string if no power points fall within the window |
| `calories_kcal` | Cumulative metabolic energy in kcal, computed as `cumulative_mechanical_kJ / (0.25 × 4.184)` (25% mechanical efficiency, rounded to 0); empty string if no power data |

### Output: `input.intervals.csv`

All 7 interval types stacked in one file, ordered by type then bucket index:

| Column | Description |
|--------|-------------|
| `interval_type` | `1min`, `5min`, `10min`, `30min`, `1km`, `5km`, `10km` |
| `interval_index` | 0-based bucket index (floor division of elapsed time or distance) |
| `start_timestamp` | ISO 8601 timestamp of first point in bucket |
| `end_timestamp` | ISO 8601 timestamp of last point in bucket |
| `duration_seconds` | `end - start` in seconds (rounded to 0.1) |
| `distance_km` | `last.distance_km - first.distance_km` in bucket (rounded to 0.001) |
| `average_speed_kmh` | `distance_km / (duration_seconds / 3600)`, 0.0 if single-point bucket (rounded to 0.1) |
| `average_power_watts` | Mean of power_watts for all power points within `[start_timestamp, end_timestamp]`; empty string if not computed (rounded to 0.1) |

Bucket assignment uses integer arithmetic to avoid floating-point drift at boundaries:
- Time-based: `bucket = floor(round(seconds_from_start × 1000) / window_ms)` (millisecond precision)
- Distance-based: `bucket = floor(round(distance_km × 1000) / window_m)` (metre precision)

### Stdout Summary

After writing both files, always printed to stderr:

```
Elapsed: HH:MM:SS | Moving: HH:MM:SS | Distance: X.XX km | Avg speed: X.X km/h | Avg power: X W | Calories: X kcal | Elevation: X m
N points → input.analyze.csv
M interval rows → input.intervals.csv
```

`Elevation` is the total elevation gain (sum of positive altitude deltas over consecutive smoothed points), in metres. It is always printed; it is 0 when no elevation data is present.

### Error Handling

- All errors from `smooth` and `power` apply (invalid GPX, window size constraints, bike not found)
- Track must have at least 2 points (required for power computation)

### Terminal Plot

After writing both CSVs (unless `--no-plot` is set), an interactive ratatui chart opens in the terminal. The user presses `q` or `Esc` to exit and return to the shell. The terminal is always restored to its original state on exit, including on I/O errors.

#### Display Layout

Two chart panels stacked vertically, with a one-line status bar below:

```
┌─────────────────────────────────────────────────────┐
│  Speed (km/h)  ·  Altitude: X–Y m                  │  ~50% of height
│    cyan       = instant speed                       │
│    green      = cumulative average speed            │
│    light blue = altitude profile (normalized)       │
├─────────────────────────────────────────────────────┤
│  Power (W)                                          │  ~47% of height
│    yellow = smoothed instant power                  │
│    green  = cumulative average power                │
├─────────────────────────────────────────────────────┤
│  Dist | Elapsed | Moving | Avg speed | Avg power | Elevation | [q] quit  │
└─────────────────────────────────────────────────────┘
```

- X axis: elapsed time in `HH:MM:SS`, three labels (start / midpoint / end)
- Y axis (speed): rounded up to the nearest 5 km/h
- Y axis (power): rounded up to the nearest 50 W
- Altitude is normalized to the speed y-range so it overlays on the same axis; the actual min–max in metres is shown in the panel title. If the GPX contains no elevation data, the altitude series is omitted.
- Power values where no 4-second window exists are rendered as 0 W
- Status bar shows total elevation gain (sum of positive altitude deltas); omitted if no elevation data

#### Interaction

| Key | Action |
|-----|--------|
| `q` | Quit |
| `Esc` | Quit |

### Testing

- Unit tests for `compute_analyze_points`: first-point zeroes, monotonic distance, raw/smoothed coord preservation, stationary points, average-speed formula consistency
- Unit tests for `compute_intervals`: correct bucket counts for all 7 specs, duration/distance sums, power averaging, sequential indices, empty input
- CSV round-trip tests: column count, power-Some vs power-None serialisation
- Config tests: serde defaults for new fields, TOML override
- Unit tests for `build_plot_data`: series length, x-values, `None` power → 0.0, `Some` power preserved, time bounds, average power computation (including all-`None` case), empty input
- Unit tests for `compute_y_bounds`: rounding up to next step, exact multiples, all-zero series, lower bound always 0.0

---

## Feature 5: Segment-Based Power Analysis

### Problem

Per-point power data has thousands of rows and is hard to interpret. Riders want aggregated summaries over meaningful windows: every kilometre, every 10 or 30 minutes, or the full ride.

### Solution

Aggregate per-point `PowerPoint` data into `Segment` records. Each segment reports average power, max power, average speed, average gradient, and Normalized Power (NP).

For the full implementation design — including the segmentation algorithm, Normalized Power calculation, formula extensions (elevation-dependent air density, wind, drivetrain loss), CLI flags, and CSV output format — see **[docs/power-segment-analysis.md](docs/power-segment-analysis.md)**.

### CLI Interface

```
my-watts power <INPUT> [OPTIONS]

Segment options (any combination):
  --segment-distance-m <M>    Segment by distance, interval in metres (e.g. 1000)
  --segment-time-s <S>        Segment by time, interval in seconds (e.g. 600)
  --segment-entire-ride       Include a single entire-ride summary segment
  --segments-output <FILE>    Output CSV for segments (default: input.segments.csv)

Physics refinement options:
  --wind-speed-kmh <KMH>      Constant wind speed; positive = headwind (default: 0)
  --drivetrain-loss <FRAC>    Drivetrain loss fraction, e.g. 0.03 (default: 0.0)
  --elevation-density         Use altitude-dependent air density
```

### Output Format

A single segments CSV combining all requested granularities:

```
segment_type,index,start_time,end_time,distance_km,duration_secs,
avg_power_watts,max_power_watts,avg_speed_kmh,avg_gradient_pct,
normalized_power_watts
```

`segment_type` values: `"1km"`, `"500m"`, `"10min"`, `"30min"`, `"entire"`, etc.  
`normalized_power_watts` is empty when the segment is shorter than 30 seconds.

### Error Handling

- No segment flags provided → no segments output produced (per-point CSV still written)
- Segment interval ≤ 0 → error

### Testing

- Unit tests for distance-based and time-based segmentation, including partial final segments
- Unit test for Normalized Power (constant power → NP equals that power)
- Integration test: GPX → compute power → compute segments (entire ride) → write segments CSV
