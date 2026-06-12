use crate::{config, GpsAnalyzerError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Everything needed to re-run `analyze` on a ride and reproduce its interactive plot.
/// Stored alongside the metrics so the `list` TUI can re-open a ride exactly as it was analyzed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayParams {
    pub rider_weight_kg: f64,
    pub bike_weight_kg: f64,
    pub bike_name: String,
    pub config_path: Option<PathBuf>,
    pub window_size: u32,
    pub degree: u32,
    pub smooth_window: usize,
    pub stop_buffer_secs: f64,
}

/// One indexed ride: the headline metrics shown in the `list` table plus the paths and
/// parameters needed to re-open it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RideEntry {
    pub stem: String,
    pub source_gpx_path: PathBuf,
    pub analyze_csv_path: PathBuf,
    pub intervals_csv_path: PathBuf,
    pub start_timestamp: DateTime<Utc>,
    pub indexed_at: DateTime<Utc>,
    pub distance_km: f64,
    pub elapsed_secs: f64,
    pub moving_secs: f64,
    pub moving_avg_speed_kmh: f64,
    pub avg_power_watts: f64,
    pub total_calories_kcal: f64,
    pub total_elevation_gain_m: f64,
    pub replay: ReplayParams,
}

/// The persistent collection of indexed rides, serialized as JSON at `index_path()`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RideIndex {
    pub rides: Vec<RideEntry>,
}

impl RideIndex {
    /// Load the index from `path`, returning an empty index if the file does not exist.
    pub fn load(path: &Path) -> Result<Self, GpsAnalyzerError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        let index = serde_json::from_str(&content)
            .map_err(|e| GpsAnalyzerError::IndexError(e.to_string()))?;
        Ok(index)
    }

    pub fn load_default() -> Result<Self, GpsAnalyzerError> {
        Self::load(&config::index_path())
    }

    /// Write the index to `path`, creating parent directories as needed.
    pub fn save(&self, path: &Path) -> Result<(), GpsAnalyzerError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| GpsAnalyzerError::IndexError(e.to_string()))?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn save_default(&self) -> Result<(), GpsAnalyzerError> {
        self.save(&config::index_path())
    }

    /// Insert `entry`, replacing any existing entry that targets the same `analyze_csv_path`.
    /// The collection is kept sorted by `start_timestamp`, most recent first.
    pub fn upsert(&mut self, entry: RideEntry) {
        self.rides
            .retain(|e| e.analyze_csv_path != entry.analyze_csv_path);
        self.rides.push(entry);
        self.rides
            .sort_by_key(|e| std::cmp::Reverse(e.start_timestamp));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_entry(stem: &str, year: i32, analyze_csv: &str) -> RideEntry {
        RideEntry {
            stem: stem.to_string(),
            source_gpx_path: PathBuf::from(format!("{stem}.gpx")),
            analyze_csv_path: PathBuf::from(analyze_csv),
            intervals_csv_path: PathBuf::from(format!("{stem}.intervals.csv")),
            start_timestamp: Utc.with_ymd_and_hms(year, 1, 1, 8, 0, 0).unwrap(),
            indexed_at: Utc.with_ymd_and_hms(2026, 6, 12, 0, 0, 0).unwrap(),
            distance_km: 42.0,
            elapsed_secs: 7200.0,
            moving_secs: 7000.0,
            moving_avg_speed_kmh: 21.6,
            avg_power_watts: 180.0,
            total_calories_kcal: 1200.0,
            total_elevation_gain_m: 600.0,
            replay: ReplayParams {
                rider_weight_kg: 75.0,
                bike_weight_kg: 10.0,
                bike_name: "road".to_string(),
                config_path: None,
                window_size: 5,
                degree: 2,
                smooth_window: 5,
                stop_buffer_secs: 10.0,
            },
        }
    }

    #[test]
    fn test_load_nonexistent_returns_empty() {
        let index = RideIndex::load(Path::new("/no/such/index.json")).unwrap();
        assert!(index.rides.is_empty());
    }

    #[test]
    fn test_save_then_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.json");
        let mut index = RideIndex::default();
        index.upsert(sample_entry("ride-a", 2024, "ride-a.analyze.csv"));
        index.save(&path).unwrap();

        let loaded = RideIndex::load(&path).unwrap();
        assert_eq!(loaded.rides, index.rides);
    }

    #[test]
    fn test_upsert_replaces_same_analyze_path() {
        let mut index = RideIndex::default();
        index.upsert(sample_entry("ride", 2024, "ride.analyze.csv"));
        let mut updated = sample_entry("ride", 2024, "ride.analyze.csv");
        updated.distance_km = 99.0;
        index.upsert(updated);

        assert_eq!(index.rides.len(), 1);
        assert_eq!(index.rides[0].distance_km, 99.0);
    }

    #[test]
    fn test_upsert_keeps_distinct_paths() {
        let mut index = RideIndex::default();
        index.upsert(sample_entry("a", 2024, "a.analyze.csv"));
        index.upsert(sample_entry("b", 2024, "b.analyze.csv"));
        assert_eq!(index.rides.len(), 2);
    }

    #[test]
    fn test_upsert_sorts_most_recent_first() {
        let mut index = RideIndex::default();
        index.upsert(sample_entry("old", 2020, "old.analyze.csv"));
        index.upsert(sample_entry("new", 2025, "new.analyze.csv"));
        index.upsert(sample_entry("mid", 2023, "mid.analyze.csv"));

        let stems: Vec<&str> = index.rides.iter().map(|e| e.stem.as_str()).collect();
        assert_eq!(stems, vec!["new", "mid", "old"]);
    }

    #[test]
    fn test_load_corrupt_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.json");
        std::fs::write(&path, "{ not valid json").unwrap();
        assert!(RideIndex::load(&path).is_err());
    }
}
