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

## Feature 3: Segment-Based Power Analysis

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
