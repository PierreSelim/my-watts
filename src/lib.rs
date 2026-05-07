use chrono::{DateTime, Utc};
use std::num::NonZeroU32;
use thiserror::Error;

pub mod analyze;
pub mod config;
pub mod csv;
pub mod gpx;
pub mod power;
pub mod smoothing;
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

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WindowSize(NonZeroU32);

impl WindowSize {
    pub fn new(size: u32) -> Result<Self, GpsAnalyzerError> {
        if size < 3 {
            return Err(GpsAnalyzerError::WindowSizeTooSmall);
        }
        if size.is_multiple_of(2) {
            return Err(GpsAnalyzerError::WindowSizeNotOdd);
        }
        NonZeroU32::new(size)
            .ok_or(GpsAnalyzerError::WindowSizeTooSmall)
            .map(WindowSize)
    }

    pub fn get(&self) -> u32 {
        self.0.get()
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
}
