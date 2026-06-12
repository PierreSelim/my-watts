use chrono::{DateTime, Utc};
use std::fmt;
use thiserror::Error;

pub mod analyze;
pub mod config;
pub mod csv;
pub mod gpx;
pub mod index;
pub mod list_tui;
pub mod power;
pub mod smoothing;
pub mod stats;
pub mod tui;

use std::path::Path;

/// Load power configuration from an optional explicit config path, applying CLI overrides for
/// rider weight and bike name on top of whatever the config file (or built-in defaults) provide.
pub fn load_power_config(
    config_path: Option<&Path>,
    rider_weight: Option<f64>,
    bike_weight: f64,
    bike_name: Option<&str>,
) -> Result<power::PowerConfig, GpsAnalyzerError> {
    let app_config = config::AppConfig::load_or_default(config_path)?;
    let rider_weight = rider_weight.unwrap_or(app_config.default_rider_weight_kg);
    let bike_name = bike_name.unwrap_or(&app_config.default_bike);
    let bike = app_config
        .find_bike(bike_name)
        .ok_or_else(|| GpsAnalyzerError::BikeNotFound(bike_name.to_string()))?
        .clone();
    Ok(power::PowerConfig {
        rider_weight_kg: rider_weight,
        bike_weight_kg: bike_weight,
        bike,
    })
}

// ── Pipeline functions ────────────────────────────────────────────────────────
//
// Each pipeline encapsulates one subcommand's full I/O workflow so that
// `main.rs` is reduced to argument extraction, calling the pipeline, and
// formatting the result for the user.

/// Result of the smooth pipeline.
#[derive(Debug, Clone)]
pub struct SmoothSummary {
    /// Number of GPS points in the output (equals input).
    pub point_count: usize,
}

/// Load a GPX file, apply Savitzky-Golay smoothing, and write the result to a CSV.
pub fn smooth_pipeline(
    input: &Path,
    output: &Path,
    sg_config: SavitzkyGolayConfig,
) -> Result<SmoothSummary, GpsAnalyzerError> {
    let track = gpx::load_gpx(input)?;
    let smoothed = smoothing::smooth_track(&track, sg_config)?;
    csv::write_csv(&smoothed, output)?;
    Ok(SmoothSummary {
        point_count: smoothed.len(),
    })
}

/// Load a GPX file, estimate power for each segment, and write the result to a CSV.
/// Returns the number of power points written (= input points − 1).
pub fn power_pipeline(
    input: &Path,
    output: &Path,
    power_config: &power::PowerConfig,
) -> Result<usize, GpsAnalyzerError> {
    let track = gpx::load_gpx(input)?;
    let points = power::compute_power(&track, power_config)?;
    csv::write_power_csv(&points, output)?;
    Ok(points.len())
}

/// All metrics produced by the analyze pipeline, ready to format and display.
#[derive(Debug)]
pub struct AnalyzeSummary {
    pub point_count: usize,
    pub interval_count: usize,
    /// Timestamp of the first GPS point — the ride's start time.
    pub start_timestamp: DateTime<Utc>,
    pub total_distance_km: f64,
    pub elapsed_secs: f64,
    pub moving_secs: f64,
    /// Total distance divided by elapsed time — includes stops.
    pub elapsed_avg_speed_kmh: f64,
    /// Moving average speed (distance / moving time).
    pub moving_avg_speed_kmh: f64,
    pub training_speed_kmh: f64,
    /// Simple mean of per-segment power for segments above the moving-speed threshold.
    pub avg_power_watts: f64,
    pub total_calories_kcal: f64,
    pub total_elevation_gain_m: f64,
    pub moving_speed_quartiles: Option<stats::Quartiles>,
    /// Pre-built data for the interactive TUI chart.
    pub plot_data: tui::PlotData,
}

/// Full analyze pipeline: load raw GPX, smooth, compute power, analyze the track,
/// then write both the per-point CSV and the interval summary CSV.
///
/// Parent directories of the output paths are created if they do not yet exist.
pub fn analyze_pipeline(
    input: &Path,
    analyze_out: &Path,
    intervals_out: &Path,
    sg_config: SavitzkyGolayConfig,
    power_config: &power::PowerConfig,
    smooth_window: usize,
    stop_buffer_secs: f64,
) -> Result<AnalyzeSummary, GpsAnalyzerError> {
    let raw_track = gpx::load_gpx(input)?;
    let smoothed_track = smoothing::smooth_track(&raw_track, sg_config)?;

    let moving_speed_threshold_kmh = power_config.bike.moving_speed_threshold_kmh;
    let power_points = power::compute_power(&smoothed_track, power_config)?;
    let (analyze_points, intervals) = analyze::analyze_track(
        &raw_track,
        &smoothed_track,
        Some(&power_points),
        moving_speed_threshold_kmh,
        smooth_window,
    );

    for path in [analyze_out, intervals_out] {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(GpsAnalyzerError::Io)?;
        }
    }
    csv::write_analyze_csv(&analyze_points, analyze_out)?;
    csv::write_intervals_csv(&intervals, intervals_out)?;

    let first = analyze_points.first().ok_or(GpsAnalyzerError::EmptyTrack)?;
    let start_timestamp = first.timestamp;
    let last = analyze_points.last().ok_or(GpsAnalyzerError::EmptyTrack)?;
    let elapsed_secs = last.seconds_from_start;
    let total_distance_km = last.distance_km;
    let elapsed_avg_speed_kmh = if elapsed_secs > 0.0 {
        total_distance_km / (elapsed_secs / 3600.0)
    } else {
        0.0
    };
    let training_speed_kmh = analyze::compute_training_speed_kmh(
        &analyze_points,
        moving_speed_threshold_kmh,
        stop_buffer_secs,
    );
    let moving_power: Vec<f64> = power_points
        .iter()
        .filter(|p| p.speed_ms * 3.6 >= moving_speed_threshold_kmh)
        .map(|p| p.power_watts)
        .collect();
    let avg_power_watts = if moving_power.is_empty() {
        0.0
    } else {
        moving_power.iter().sum::<f64>() / moving_power.len() as f64
    };
    let moving_speed_quartiles =
        tui::moving_speed_quartiles(&analyze_points, moving_speed_threshold_kmh);
    let plot_data = tui::build_plot_data(
        &analyze_points,
        moving_speed_threshold_kmh,
        training_speed_kmh,
    );

    Ok(AnalyzeSummary {
        point_count: analyze_points.len(),
        interval_count: intervals.len(),
        start_timestamp,
        total_distance_km,
        elapsed_secs,
        moving_secs: last.moving_seconds_from_start,
        elapsed_avg_speed_kmh,
        moving_avg_speed_kmh: last.average_speed_kmh,
        training_speed_kmh,
        avg_power_watts,
        total_calories_kcal: last.cumulative_energy_kj.map(kj_to_kcal).unwrap_or(0.0),
        total_elevation_gain_m: analyze::compute_elevation_gain_m(&analyze_points).unwrap_or(0.0),
        moving_speed_quartiles,
        plot_data,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntervalType {
    Min1,
    Min5,
    Min10,
    Min30,
    Km1,
    Km5,
    Km10,
}

impl fmt::Display for IntervalType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            IntervalType::Min1 => "1min",
            IntervalType::Min5 => "5min",
            IntervalType::Min10 => "10min",
            IntervalType::Min30 => "30min",
            IntervalType::Km1 => "1km",
            IntervalType::Km5 => "5km",
            IntervalType::Km10 => "10km",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GpsPoint {
    pub lat: f64,
    pub lon: f64,
    pub alt: Option<f64>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Track {
    pub points: Vec<GpsPoint>,
}

impl Track {
    pub fn new(points: Vec<GpsPoint>) -> Result<Self, GpsAnalyzerError> {
        if points.is_empty() {
            return Err(GpsAnalyzerError::EmptyTrack);
        }
        Ok(Track { points })
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.points.len()
    }
}

const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// Haversine great-circle distance between two GPS points, in metres.
pub fn haversine_distance(p1: &GpsPoint, p2: &GpsPoint) -> f64 {
    let lat1 = p1.lat.to_radians();
    let lat2 = p2.lat.to_radians();
    let dlat = (p2.lat - p1.lat).to_radians();
    let dlon = (p2.lon - p1.lon).to_radians();
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    EARTH_RADIUS_M * 2.0 * a.sqrt().asin()
}

/// Convert mechanical energy in kJ to metabolic energy in kcal (25 % mechanical efficiency).
pub fn kj_to_kcal(kj: f64) -> f64 {
    kj / (0.25 * 4.184)
}

#[derive(Debug, Clone, Copy)]
pub struct WindowSize(u32);

impl WindowSize {
    pub fn new(size: u32) -> Result<Self, GpsAnalyzerError> {
        if size < 3 {
            return Err(GpsAnalyzerError::WindowSizeTooSmall);
        }
        if size.is_multiple_of(2) {
            return Err(GpsAnalyzerError::WindowSizeNotOdd);
        }
        Ok(WindowSize(size))
    }

    pub fn get(&self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SavitzkyGolayConfig {
    pub window_size: WindowSize,
    pub polynomial_degree: u32,
}

impl SavitzkyGolayConfig {
    pub fn new(window_size: u32, polynomial_degree: u32) -> Result<Self, GpsAnalyzerError> {
        let window_size = WindowSize::new(window_size)?;
        if polynomial_degree > window_size.get() / 2 {
            return Err(GpsAnalyzerError::PolynomialDegreeTooLarge);
        }
        Ok(SavitzkyGolayConfig {
            window_size,
            polynomial_degree,
        })
    }
}

#[derive(Debug, Clone)]
pub struct AnalyzePoint {
    pub timestamp: DateTime<Utc>,
    pub seconds_from_start: f64,
    pub moving_seconds_from_start: f64,
    pub raw_lat: f64,
    pub raw_lon: f64,
    pub smoothed_lat: f64,
    pub smoothed_lon: f64,
    pub instant_speed_kmh: f64,
    pub average_speed_kmh: f64,
    pub distance_km: f64,
    pub smoothed_alt: Option<f64>,
    pub power_smooth_watts: Option<f64>,
    pub cumulative_energy_kj: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct IntervalSummary {
    pub interval_type: IntervalType,
    pub interval_index: usize,
    pub start_timestamp: DateTime<Utc>,
    pub end_timestamp: DateTime<Utc>,
    pub duration_seconds: f64,
    pub distance_km: f64,
    pub average_speed_kmh: f64,
    pub average_power_watts: Option<f64>,
}

pub fn fmt_hhmmss(total_secs: f64) -> String {
    let secs = total_secs.max(0.0) as u64;
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

#[derive(Error, Debug)]
pub enum GpsAnalyzerError {
    #[error("Track is empty")]
    EmptyTrack,

    #[error("Window size must be at least 3")]
    WindowSizeTooSmall,

    #[error("Window size must be odd")]
    WindowSizeNotOdd,

    #[error("Window size {0} is larger than track length {1}")]
    WindowSizeTooLarge(u32, usize),

    #[error("Track must have at least {0} points")]
    TrackTooShort(usize),

    #[error("Invalid GPX format: {0}")]
    InvalidGpx(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("CSV error: {0}")]
    Csv(String),

    #[error("Polynomial degree must be at most half the window size")]
    PolynomialDegreeTooLarge,

    #[error("Parsing error: {0}")]
    ParseError(String),

    #[error("Non-positive time delta ({0:.3}s) between consecutive GPS points")]
    NonPositiveTimeDelta(f64),

    #[error("Numerical error: {0}")]
    NumericalError(String),

    #[error("Config error: {0}")]
    ConfigError(String),

    #[error("Bike '{0}' not found in config")]
    BikeNotFound(String),

    #[error("Index error: {0}")]
    IndexError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_window_size_valid() {
        assert!(WindowSize::new(5).is_ok());
        assert!(WindowSize::new(3).is_ok());
        assert!(WindowSize::new(7).is_ok());
    }

    #[test]
    fn test_window_size_invalid() {
        assert!(WindowSize::new(1).is_err());
        assert!(WindowSize::new(2).is_err());
        assert!(WindowSize::new(4).is_err());
        assert!(WindowSize::new(6).is_err());
    }

    #[test]
    fn test_track_creation() {
        let points = vec![GpsPoint {
            lat: 0.0,
            lon: 0.0,
            alt: None,
            timestamp: Utc::now(),
        }];
        assert!(Track::new(points).is_ok());
    }

    #[test]
    fn test_track_empty() {
        assert!(Track::new(vec![]).is_err());
    }

    #[test]
    fn test_savitzky_golay_config_degree_at_most_half_window() {
        // max degree = window_size / 2 (integer division)
        assert!(SavitzkyGolayConfig::new(3, 0).is_ok()); // 0 <= 1
        assert!(SavitzkyGolayConfig::new(3, 1).is_ok()); // 1 <= 1
        assert!(SavitzkyGolayConfig::new(3, 2).is_err()); // 2 > 1
        assert!(SavitzkyGolayConfig::new(3, 3).is_err());
        assert!(SavitzkyGolayConfig::new(5, 2).is_ok()); // 2 <= 2
        assert!(SavitzkyGolayConfig::new(5, 3).is_err()); // 3 > 2
        assert!(SavitzkyGolayConfig::new(5, 5).is_err());
        assert!(SavitzkyGolayConfig::new(7, 3).is_ok()); // 3 <= 3
        assert!(SavitzkyGolayConfig::new(7, 4).is_err()); // 4 > 3
    }

    #[test]
    fn test_kj_to_kcal() {
        let kcal = kj_to_kcal(4.184);
        assert!((kcal - 4.0).abs() < 1e-9);
    }

    #[test]
    fn test_haversine_paris_london() {
        let paris = GpsPoint {
            lat: 48.8566,
            lon: 2.3522,
            alt: None,
            timestamp: Utc::now(),
        };
        let london = GpsPoint {
            lat: 51.5074,
            lon: -0.1278,
            alt: None,
            timestamp: Utc::now(),
        };
        let d = haversine_distance(&paris, &london);
        assert!(
            (d - 343_556.0).abs() < 1.0,
            "expected ~343556m, got {d:.2}m"
        );
    }

    #[test]
    fn test_load_power_config_uses_builtin_defaults_when_no_file() {
        let cfg = load_power_config(None, None, 10.0, None).unwrap();
        assert_eq!(cfg.rider_weight_kg, 75.0);
        assert_eq!(cfg.bike_weight_kg, 10.0);
        assert_eq!(cfg.bike.name, "road");
    }

    #[test]
    fn test_load_power_config_overrides_rider_weight() {
        let cfg = load_power_config(None, Some(90.0), 8.0, Some("road")).unwrap();
        assert_eq!(cfg.rider_weight_kg, 90.0);
    }

    #[test]
    fn test_load_power_config_selects_named_bike() {
        let cfg = load_power_config(None, None, 10.0, Some("gravel")).unwrap();
        assert_eq!(cfg.bike.name, "gravel");
    }

    #[test]
    fn test_load_power_config_unknown_bike_errors() {
        let err = load_power_config(None, None, 10.0, Some("unicycle")).unwrap_err();
        assert!(err.to_string().contains("unicycle"));
    }

    #[test]
    fn test_load_power_config_from_explicit_toml() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(
            temp,
            r#"
default_rider_weight_kg = 65.0
[[bikes]]
name = "gravel"
crr = 0.006
cda = 0.40
"#
        )
        .unwrap();
        let cfg = load_power_config(Some(temp.path()), None, 10.0, Some("gravel")).unwrap();
        assert_eq!(cfg.rider_weight_kg, 65.0);
        assert_eq!(cfg.bike.name, "gravel");
    }

    #[test]
    fn test_load_power_config_missing_explicit_file_errors() {
        let result = load_power_config(
            Some(std::path::Path::new("/nonexistent/config.toml")),
            None,
            10.0,
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_fmt_hhmmss_zero() {
        assert_eq!(fmt_hhmmss(0.0), "00:00:00");
    }

    #[test]
    fn test_fmt_hhmmss_one_hour() {
        assert_eq!(fmt_hhmmss(3600.0), "01:00:00");
    }

    #[test]
    fn test_fmt_hhmmss_negative_clamps_to_zero() {
        assert_eq!(fmt_hhmmss(-5.0), "00:00:00");
    }

    // ── Pipeline tests ────────────────────────────────────────────────────────

    const FIVE_POINT_GPX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1">
  <trk><trkseg>
    <trkpt lat="48.8566" lon="2.3522"><ele>50.0</ele><time>2024-01-01T10:00:00Z</time></trkpt>
    <trkpt lat="48.8575" lon="2.3530"><ele>51.0</ele><time>2024-01-01T10:00:10Z</time></trkpt>
    <trkpt lat="48.8584" lon="2.3538"><ele>52.0</ele><time>2024-01-01T10:00:20Z</time></trkpt>
    <trkpt lat="48.8593" lon="2.3546"><ele>53.0</ele><time>2024-01-01T10:00:30Z</time></trkpt>
    <trkpt lat="48.8602" lon="2.3554"><ele>54.0</ele><time>2024-01-01T10:00:40Z</time></trkpt>
  </trkseg></trk>
</gpx>"#;

    fn write_gpx_temp() -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "{}", FIVE_POINT_GPX).unwrap();
        f
    }

    #[test]
    fn test_smooth_pipeline_preserves_point_count() {
        let gpx = write_gpx_temp();
        let out = tempfile::NamedTempFile::new().unwrap();
        let sg = SavitzkyGolayConfig::new(3, 1).unwrap();
        let summary = smooth_pipeline(gpx.path(), out.path(), sg).unwrap();
        assert_eq!(summary.point_count, 5);
        // Output CSV should have header + 5 rows
        let content = std::fs::read_to_string(out.path()).unwrap();
        assert_eq!(content.lines().count(), 6);
    }

    #[test]
    fn test_smooth_pipeline_missing_input_errors() {
        let out = tempfile::NamedTempFile::new().unwrap();
        let sg = SavitzkyGolayConfig::new(3, 1).unwrap();
        assert!(smooth_pipeline(std::path::Path::new("/no/such.gpx"), out.path(), sg).is_err());
    }

    #[test]
    fn test_power_pipeline_returns_n_minus_one_points() {
        let gpx = write_gpx_temp();
        let out = tempfile::NamedTempFile::new().unwrap();
        let power_config = load_power_config(None, Some(75.0), 10.0, Some("road")).unwrap();
        let count = power_pipeline(gpx.path(), out.path(), &power_config).unwrap();
        // 5 GPS points → 4 power segments
        assert_eq!(count, 4);
        let content = std::fs::read_to_string(out.path()).unwrap();
        assert_eq!(content.lines().count(), 5); // header + 4 rows
    }

    #[test]
    fn test_power_pipeline_missing_input_errors() {
        let out = tempfile::NamedTempFile::new().unwrap();
        let power_config = load_power_config(None, Some(75.0), 10.0, Some("road")).unwrap();
        assert!(power_pipeline(
            std::path::Path::new("/no/such.gpx"),
            out.path(),
            &power_config
        )
        .is_err());
    }

    #[test]
    fn test_analyze_pipeline_output_structure() {
        let gpx = write_gpx_temp();
        let analyze_out = tempfile::NamedTempFile::new().unwrap();
        let intervals_out = tempfile::NamedTempFile::new().unwrap();
        let sg = SavitzkyGolayConfig::new(3, 1).unwrap();
        let power_config = load_power_config(None, Some(75.0), 10.0, Some("road")).unwrap();

        let summary = analyze_pipeline(
            gpx.path(),
            analyze_out.path(),
            intervals_out.path(),
            sg,
            &power_config,
            1,
            10.0,
        )
        .unwrap();

        assert_eq!(summary.point_count, 5);
        assert!(summary.interval_count > 0);
        assert!(summary.total_distance_km > 0.0);
        assert!(summary.elapsed_secs > 0.0);
        assert!(summary.total_elevation_gain_m > 0.0);
        // start_timestamp is the first GPS point's time.
        assert_eq!(
            summary.start_timestamp,
            "2024-01-01T10:00:00Z".parse::<DateTime<Utc>>().unwrap()
        );

        // per-point CSV: header + 5 rows
        let analyze_content = std::fs::read_to_string(analyze_out.path()).unwrap();
        assert_eq!(analyze_content.lines().count(), 6);
    }

    #[test]
    fn test_analyze_pipeline_speeds_are_consistent() {
        let gpx = write_gpx_temp();
        let analyze_out = tempfile::NamedTempFile::new().unwrap();
        let intervals_out = tempfile::NamedTempFile::new().unwrap();
        let sg = SavitzkyGolayConfig::new(3, 1).unwrap();
        let power_config = load_power_config(None, Some(75.0), 10.0, Some("road")).unwrap();

        let summary = analyze_pipeline(
            gpx.path(),
            analyze_out.path(),
            intervals_out.path(),
            sg,
            &power_config,
            1,
            10.0,
        )
        .unwrap();

        // elapsed_avg_speed = distance / elapsed * 3600
        let expected_elapsed_avg = summary.total_distance_km / (summary.elapsed_secs / 3600.0);
        assert!((summary.elapsed_avg_speed_kmh - expected_elapsed_avg).abs() < 1e-9);

        // Moving speed ≥ elapsed speed (moving time ≤ elapsed time)
        assert!(summary.moving_avg_speed_kmh >= summary.elapsed_avg_speed_kmh - 1e-9);
    }

    #[test]
    fn test_analyze_pipeline_missing_input_errors() {
        let analyze_out = tempfile::NamedTempFile::new().unwrap();
        let intervals_out = tempfile::NamedTempFile::new().unwrap();
        let sg = SavitzkyGolayConfig::new(3, 1).unwrap();
        let power_config = load_power_config(None, Some(75.0), 10.0, Some("road")).unwrap();
        assert!(analyze_pipeline(
            std::path::Path::new("/no/such.gpx"),
            analyze_out.path(),
            intervals_out.path(),
            sg,
            &power_config,
            1,
            10.0,
        )
        .is_err());
    }
}
