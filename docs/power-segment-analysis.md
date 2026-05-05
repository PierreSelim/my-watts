# Power Segment Analysis — Implementation Plan

## Context

The project already computes per-GPS-point wattage estimates (`src/power.rs`). The goal is to aggregate those estimates over ride segments — every 1 km, every 10 minutes, every 30 minutes, entire ride — to get meaningful summaries rather than thousands of point-level values.

The formula also has two minor gaps vs. the [omnicalculator cycling wattage reference](https://www.omnicalculator.com/sports/cycling-wattage):
- Air density is fixed at sea level (should vary with altitude)
- No drivetrain-loss factor or wind term

---

## Physics Reference

### Full Power Formula

```
P = (Fg + Fr + Fa) × v / (1 - loss)
```

| Symbol | Meaning | Unit |
|--------|---------|------|
| P | Power output | W |
| Fg | Gravitational resistance | N |
| Fr | Rolling resistance | N |
| Fa | Aerodynamic drag | N |
| v | Speed | m/s |
| loss | Drivetrain loss fraction | 0.0–0.05 |

### Force Components

```
Fg = g × sin(arctan(slope)) × (M + m)
Fr = g × cos(arctan(slope)) × (M + m) × Crr
Fa = 0.5 × Cd×A × ρ × (v + w)²
```

### Air Density by Elevation

```
ρ = 1.225 × exp(-0.00011856 × h)
```

where `h` = elevation in metres above sea level.

### Approximated Factors

| Factor | Approximation |
|--------|--------------|
| Wind (w) | Constant, user-supplied; defaults to 0 m/s |
| Drivetrain loss | Constant fraction; defaults to 0 (i.e., ignored) |
| Air density | Either constant 1.225 or altitude-derived (opt-in flag) |
| CdA | From bike config (see `config.toml`) |
| Crr | From bike config |

---

## Scope of Changes

### 1. Formula Improvements — `src/power.rs`

Extend `PowerConfig`:

```rust
pub struct PowerConfig {
    pub rider_weight_kg: f64,
    pub bike_weight_kg: f64,
    pub bike: BikeConfig,
    // New fields (backward-compatible via Default)
    pub wind_speed_ms: f64,          // headwind > 0, tailwind < 0; default 0.0
    pub drivetrain_loss: f64,        // fraction 0.0–1.0; default 0.0
    pub use_elevation_density: bool, // default false
}
```

Extend `PowerPoint`:

```rust
pub struct PowerPoint {
    pub timestamp: DateTime<Utc>,
    pub power_watts: f64,
    pub speed_ms: f64,
    pub gradient: f64,
    pub distance_m: f64,  // distance from previous point — needed for segmentation
}
```

Update `segment_power` physics:
- `rho = if use_elevation_density { 1.225 * (-0.00011856 * avg_alt).exp() } else { 1.225 }`  
  `avg_alt = (p1.alt + p2.alt) / 2`, falling back to 0.0 if altitude absent
- `f_drag = 0.5 * cda * rho * (speed + wind_speed_ms).powi(2)`
- `raw_power = (f_gravity + f_rolling + f_drag) * speed`
- `power_output = (raw_power / (1.0 - drivetrain_loss)).max(0.0)`

**New tests**: elevation density reduces drag at altitude; headwind increases power; tailwind reduces power; drivetrain loss scales power; `distance_m` matches manual haversine.

> Existing tests use struct literal construction — add `#[derive(Default)]` to `PowerConfig` and update tests with `..Default::default()`.

---

### 2. New Module — `src/segments.rs`

#### Types

```rust
#[derive(Debug, Clone, Copy)]
pub enum SegmentKind {
    ByDistance { interval_m: f64 },
    ByTime { interval_secs: u64 },
    EntireRide,
}

impl SegmentKind {
    pub fn label(&self) -> String {
        // "1km", "500m", "10min", "30min", "600s", "entire"
    }
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub kind_label: String,
    pub index: usize,
    pub start_timestamp: DateTime<Utc>,
    pub end_timestamp: DateTime<Utc>,
    pub distance_km: f64,
    pub duration_secs: f64,
    pub avg_power_watts: f64,
    pub max_power_watts: f64,
    pub avg_speed_kmh: f64,
    pub avg_gradient_pct: f64,
    pub normalized_power_watts: Option<f64>,  // None if segment < 30s
}
```

#### Core Function

```rust
pub fn compute_segments(points: &[PowerPoint], kind: SegmentKind) -> Vec<Segment>
```

Algorithm (`fold`-based, no escaping mutation):
1. Accumulate `(Vec<Segment>, current_bucket: Vec<&PowerPoint>, bucket_cum_dist_m, bucket_start_time)`
2. For each point: push to bucket; check threshold (distance or time)
3. On threshold exceeded: `aggregate(bucket)` → push segment, reset bucket
4. After fold: flush remaining non-empty bucket as final partial segment
5. `EntireRide`: skip fold, call `aggregate` on all points directly

#### Aggregation

```rust
fn aggregate(bucket: &[&PowerPoint], index: usize, label: String) -> Segment
```
- `distance_km` = sum of `distance_m` / 1000
- `avg_power_watts` = arithmetic mean of `power_watts`
- `max_power_watts` = fold with `f64::max`
- `avg_speed_kmh` = mean of `speed_ms × 3.6`
- `avg_gradient_pct` = mean of `gradient × 100`
- `normalized_power_watts` = `normalized_power(bucket)`

#### Normalized Power (NP)

```rust
fn normalized_power(bucket: &[&PowerPoint]) -> Option<f64>
```
- Return `None` if segment duration < 30 s
- For each point, compute 30-second rolling average of power using a two-pointer sliding window over timestamps
- `NP = (mean of rolling_avg^4)^0.25`

#### Unit Tests

| Test | Verifies |
|------|---------|
| `test_by_distance_correct_segment_count` | 3 km at 1 km → 3 segments |
| `test_by_distance_partial_last_segment` | 2.5 km at 1 km → 3 segments (last ~0.5 km) |
| `test_by_time_correct_segment_count` | 30 min at 10 min → 3 segments |
| `test_by_time_partial_last_segment` | 25 min at 10 min → 3 segments |
| `test_entire_ride_single_segment` | N points → 1 segment |
| `test_avg_power_is_mean` | known powers → correct mean |
| `test_max_power_is_max` | known powers → correct max |
| `test_normalized_power_none_under_30s` | < 30s bucket → `None` |
| `test_normalized_power_constant_is_self` | constant 200 W over 60 s → NP = 200 W |
| `test_empty_points_returns_empty` | `[]` → `vec![]`, no panic |

---

### 3. CSV Output — `src/csv.rs`

```rust
#[derive(Serialize)]
struct SegmentCsvRecord {
    segment_type: String,
    index: usize,
    start_time: String,         // RFC 3339
    end_time: String,
    distance_km: f64,
    duration_secs: f64,
    avg_power_watts: f64,
    max_power_watts: f64,
    avg_speed_kmh: f64,
    avg_gradient_pct: f64,
    normalized_power_watts: String,  // number or "" when None
}

pub fn write_segments_csv<P: AsRef<Path>>(
    segments: &[Segment],
    path: P,
) -> Result<(), GpsAnalyzerError>
```

All floats rounded to 1 decimal place (consistent with `write_power_csv`).

**New tests**: headers present; `None` NP renders as empty string.

---

### 4. CLI Extensions — `src/cli.rs`

Add to `PowerCommand`:

```rust
// Segment flags
/// Segment by distance in metres (e.g. 1000 for 1 km)
#[arg(long)]
pub segment_distance_m: Option<f64>,

/// Segment by time in seconds (e.g. 600 for 10 min)
#[arg(long)]
pub segment_time_s: Option<u64>,

/// Include an entire-ride summary segment
#[arg(long)]
pub segment_entire_ride: bool,

/// Output CSV for segments (default: <stem>.segments.csv)
#[arg(long)]
pub segments_output: Option<PathBuf>,

// Physics flags
/// Constant wind speed in km/h (positive = headwind, default 0)
#[arg(long, default_value = "0.0")]
pub wind_speed_kmh: f64,

/// Drivetrain loss fraction (e.g. 0.03 = 3%, default 0.0)
#[arg(long, default_value = "0.0")]
pub drivetrain_loss: f64,

/// Use elevation-dependent air density
#[arg(long)]
pub elevation_density: bool,
```

Add helpers to `impl PowerCommand`:
- `segments_output_path(&self) -> PathBuf`
- `requested_segment_kinds(&self) -> Vec<SegmentKind>`

---

### 5. Wire-Up — `src/main.rs`

In `run_power`, after `compute_power`:

1. Build `PowerConfig` with physics fields from CLI (wind, loss, elevation density)
2. Collect `requested_segment_kinds()`
3. For each kind: `compute_segments(&points, kind)` → extend a flat `Vec<Segment>`
4. Sort by `kind_label` then `index`
5. `write_segments_csv(&segments, &cmd.segments_output_path())`
6. Print summary count

---

### 6. Register Module — `src/lib.rs`

- Add `pub mod segments;`
- Add error variant: `#[error("Invalid segment interval: {0}")] InvalidSegmentInterval(String)`

---

## File Changelist

| File | Change |
|------|--------|
| `src/lib.rs` | Add `pub mod segments`, add `InvalidSegmentInterval` error variant |
| `src/power.rs` | Extend `PowerConfig`/`PowerPoint`, update physics, add tests |
| `src/segments.rs` | **New** — segment types, `compute_segments`, `aggregate`, `normalized_power`, tests |
| `src/csv.rs` | Add `write_segments_csv`, `SegmentCsvRecord`, tests |
| `src/cli.rs` | Add segment + physics flags to `PowerCommand` |
| `src/main.rs` | Wire segments pipeline in `run_power` |

---

## Verification

```bash
cargo build
cargo test
cargo fmt && cargo clippy

# End-to-end with a real GPX file
cargo run -- power test_ride.gpx \
  --rider-weight 75 --bike road \
  --segment-distance-m 1000 \
  --segment-time-s 600 \
  --segment-entire-ride \
  --wind-speed-kmh 5 \
  --drivetrain-loss 0.03 \
  --elevation-density \
  --verbose
# Inspect: test_ride.segments.csv
```
