use chrono::{DateTime, Utc};
use thiserror::Error;

pub mod analyze;
pub mod config;
pub mod csv;
pub mod gpx;
pub mod power;
pub mod smoothing;
pub mod stats;
pub mod tui;

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

    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Always returns `false`: `Track::new` rejects empty point lists.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
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
        if polynomial_degree >= window_size.get() {
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
    pub interval_type: String,
    pub interval_index: usize,
    pub start_timestamp: DateTime<Utc>,
    pub end_timestamp: DateTime<Utc>,
    pub duration_seconds: f64,
    pub distance_km: f64,
    pub average_speed_kmh: f64,
    pub average_power_watts: Option<f64>,
}

pub fn fmt_hhmmss(total_secs: f64) -> String {
    let secs = total_secs as u64;
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

    #[error("Invalid GPX format: {0}")]
    InvalidGpx(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("CSV error: {0}")]
    Csv(String),

    #[error("Polynomial degree must be less than window size")]
    PolynomialDegreeTooLarge,

    #[error("Parsing error: {0}")]
    ParseError(String),

    #[error("Config error: {0}")]
    ConfigError(String),

    #[error("Bike '{0}' not found in config")]
    BikeNotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_savitzky_golay_config_degree_must_be_less_than_window() {
        assert!(SavitzkyGolayConfig::new(3, 3).is_err());
        assert!(SavitzkyGolayConfig::new(3, 4).is_err());
        assert!(SavitzkyGolayConfig::new(5, 5).is_err());
        assert!(SavitzkyGolayConfig::new(5, 2).is_ok());
        assert!(SavitzkyGolayConfig::new(3, 0).is_ok());
        assert!(SavitzkyGolayConfig::new(3, 2).is_ok());
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
}
