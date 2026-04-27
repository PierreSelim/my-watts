use crate::{GpsAnalyzerError, Track};
use csv::Writer;
use serde::Serialize;
use std::fs::File;
use std::path::Path;

#[derive(Serialize)]
struct CsvRecord {
    latitude: f64,
    longitude: f64,
    altitude: String,
    timestamp: String,
}

pub fn write_csv<P: AsRef<Path>>(track: &Track, path: P) -> Result<(), GpsAnalyzerError> {
    let file = File::create(path)?;
    let mut writer = Writer::from_writer(file);

    for point in &track.points {
        let altitude = point.alt.map(|a| a.to_string()).unwrap_or_default();

        let record = CsvRecord {
            latitude: point.lat,
            longitude: point.lon,
            altitude,
            timestamp: point.timestamp.to_rfc3339(),
        };

        writer
            .serialize(record)
            .map_err(|e| GpsAnalyzerError::Csv(format!("Failed to write CSV record: {}", e)))?;
    }

    writer
        .flush()
        .map_err(|e| GpsAnalyzerError::Csv(format!("Failed to flush CSV writer: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::fs;
    use tempfile::NamedTempFile;

    #[test]
    fn test_write_csv() {
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let path = temp_file.path().to_path_buf();

        let points = vec![
            crate::GpsPoint {
                lat: 40.0,
                lon: -120.0,
                alt: Some(100.0),
                timestamp: Utc::now(),
            },
            crate::GpsPoint {
                lat: 40.1,
                lon: -120.1,
                alt: None,
                timestamp: Utc::now(),
            },
        ];

        let track = Track::new(points).expect("Failed to create track");
        write_csv(&track, &path).expect("Failed to write CSV");

        let content = fs::read_to_string(&path).expect("Failed to read CSV");
        assert!(content.contains("40"));
        assert!(content.contains("-120"));
    }
}
