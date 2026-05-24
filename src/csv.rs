use crate::power::PowerPoint;
use crate::{kj_to_kcal, AnalyzePoint, GpsAnalyzerError, IntervalSummary, Track};
use csv::Writer;
use serde::Serialize;
use std::fs::File;
use std::path::Path;

fn round_to(value: f64, places: u32) -> f64 {
    let factor = 10_f64.powi(places as i32);
    (value * factor).round() / factor
}

fn flush_writer(writer: &mut Writer<File>) -> Result<(), GpsAnalyzerError> {
    writer
        .flush()
        .map_err(|e| GpsAnalyzerError::Csv(format!("Failed to flush CSV writer: {e}")))
}

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

    flush_writer(&mut writer)?;

    Ok(())
}

#[derive(Serialize)]
struct PowerCsvRecord {
    timestamp: String,
    power_watts: f64,
    speed_kmh: f64,
    gradient_pct: f64,
}

pub fn write_power_csv<P: AsRef<Path>>(
    points: &[PowerPoint],
    path: P,
) -> Result<(), GpsAnalyzerError> {
    let file = File::create(path)?;
    let mut writer = Writer::from_writer(file);

    for point in points {
        let record = PowerCsvRecord {
            timestamp: point.timestamp.to_rfc3339(),
            power_watts: round_to(point.power_watts, 1),
            speed_kmh: round_to(point.speed_ms * 3.6, 1),
            gradient_pct: round_to(point.gradient * 100.0, 1),
        };
        writer
            .serialize(record)
            .map_err(|e| GpsAnalyzerError::Csv(format!("Failed to write CSV record: {}", e)))?;
    }

    flush_writer(&mut writer)?;

    Ok(())
}

#[derive(Serialize)]
struct AnalyzeCsvRecord {
    timestamp: String,
    seconds_from_start: f64,
    raw_lat: f64,
    raw_lon: f64,
    smoothed_lat: f64,
    smoothed_lon: f64,
    instant_speed_kmh: f64,
    average_speed_kmh: f64,
    distance_km: f64,
    power_smooth_watts: String,
    calories_kcal: String,
}

pub fn write_analyze_csv<P: AsRef<Path>>(
    points: &[AnalyzePoint],
    path: P,
) -> Result<(), GpsAnalyzerError> {
    let file = File::create(path)?;
    let mut writer = Writer::from_writer(file);

    for point in points {
        let record = AnalyzeCsvRecord {
            timestamp: point.timestamp.to_rfc3339(),
            seconds_from_start: round_to(point.seconds_from_start, 2),
            raw_lat: point.raw_lat,
            raw_lon: point.raw_lon,
            smoothed_lat: point.smoothed_lat,
            smoothed_lon: point.smoothed_lon,
            instant_speed_kmh: round_to(point.instant_speed_kmh, 1),
            average_speed_kmh: round_to(point.average_speed_kmh, 1),
            distance_km: round_to(point.distance_km, 3),
            power_smooth_watts: point
                .power_smooth_watts
                .map(|w| format!("{:.1}", w))
                .unwrap_or_default(),
            calories_kcal: point
                .cumulative_energy_kj
                .map(|e| format!("{:.0}", kj_to_kcal(e)))
                .unwrap_or_default(),
        };
        writer
            .serialize(record)
            .map_err(|e| GpsAnalyzerError::Csv(format!("Failed to write CSV record: {}", e)))?;
    }

    flush_writer(&mut writer)?;

    Ok(())
}

#[derive(Serialize)]
struct IntervalCsvRecord {
    interval_type: String,
    interval_index: usize,
    start_timestamp: String,
    end_timestamp: String,
    duration_seconds: f64,
    distance_km: f64,
    average_speed_kmh: f64,
    average_power_watts: String,
}

pub fn write_intervals_csv<P: AsRef<Path>>(
    intervals: &[IntervalSummary],
    path: P,
) -> Result<(), GpsAnalyzerError> {
    let file = File::create(path)?;
    let mut writer = Writer::from_writer(file);

    for interval in intervals {
        let record = IntervalCsvRecord {
            interval_type: interval.interval_type.to_string(),
            interval_index: interval.interval_index,
            start_timestamp: interval.start_timestamp.to_rfc3339(),
            end_timestamp: interval.end_timestamp.to_rfc3339(),
            duration_seconds: round_to(interval.duration_seconds, 1),
            distance_km: round_to(interval.distance_km, 3),
            average_speed_kmh: round_to(interval.average_speed_kmh, 1),
            average_power_watts: interval
                .average_power_watts
                .map(|w| format!("{:.1}", w))
                .unwrap_or_default(),
        };
        writer
            .serialize(record)
            .map_err(|e| GpsAnalyzerError::Csv(format!("Failed to write CSV record: {}", e)))?;
    }

    flush_writer(&mut writer)?;

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

    fn make_analyze_point(seconds: f64, distance_km: f64) -> AnalyzePoint {
        AnalyzePoint {
            timestamp: Utc::now(),
            seconds_from_start: seconds,
            moving_seconds_from_start: seconds,
            raw_lat: 48.0,
            raw_lon: 2.0,
            smoothed_lat: 48.0001,
            smoothed_lon: 2.0001,
            smoothed_alt: None,
            instant_speed_kmh: 30.0,
            average_speed_kmh: 28.0,
            distance_km,
            power_smooth_watts: None,
            cumulative_energy_kj: None,
        }
    }

    #[test]
    fn test_write_power_csv_row_and_column_count() {
        use crate::power::PowerPoint;
        use chrono::DateTime;

        let temp_file = NamedTempFile::new().unwrap();
        let points = vec![
            PowerPoint {
                timestamp: DateTime::from_timestamp(1_700_000_010, 0).unwrap(),
                power_watts: 250.345,
                speed_ms: 8.333,
                gradient: 0.025,
            },
            PowerPoint {
                timestamp: DateTime::from_timestamp(1_700_000_020, 0).unwrap(),
                power_watts: 0.0,
                speed_ms: 0.0,
                gradient: -0.1,
            },
        ];
        write_power_csv(&points, temp_file.path()).unwrap();

        let content = fs::read_to_string(temp_file.path()).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3, "header + 2 data rows");
        assert_eq!(lines[0].split(',').count(), 4, "4 columns");
        // round_to(250.345, 1) == 250.3
        assert!(
            lines[1].contains("250.3"),
            "power rounded to 1dp: {}",
            lines[1]
        );
        // round_to(8.333 * 3.6, 1) == round_to(29.999, 1) == 30.0
        assert!(lines[1].contains("30.0"), "speed in km/h: {}", lines[1]);
        // round_to(0.025 * 100, 1) == 2.5
        assert!(lines[1].contains("2.5"), "gradient in pct: {}", lines[1]);
    }

    #[test]
    fn test_write_power_csv_negative_gradient_is_preserved() {
        use crate::power::PowerPoint;
        use chrono::DateTime;

        let temp_file = NamedTempFile::new().unwrap();
        let points = vec![PowerPoint {
            timestamp: DateTime::from_timestamp(1_700_000_010, 0).unwrap(),
            power_watts: 0.0,
            speed_ms: 5.0,
            gradient: -0.05,
        }];
        write_power_csv(&points, temp_file.path()).unwrap();

        let content = fs::read_to_string(temp_file.path()).unwrap();
        // round_to(-0.05 * 100, 1) == -5.0
        assert!(content.contains("-5.0"), "negative gradient: {content}");
    }

    #[test]
    fn test_write_analyze_csv_row_and_column_count() {
        let temp_file = NamedTempFile::new().unwrap();
        let points = vec![
            make_analyze_point(0.0, 0.0),
            make_analyze_point(10.0, 0.083),
        ];
        write_analyze_csv(&points, temp_file.path()).unwrap();

        let content = fs::read_to_string(temp_file.path()).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3, "header + 2 data rows");
        assert_eq!(lines[0].split(',').count(), 11, "11 columns in header");
    }

    #[test]
    fn test_write_analyze_csv_values_present() {
        let temp_file = NamedTempFile::new().unwrap();
        let points = vec![make_analyze_point(0.0, 0.0)];
        write_analyze_csv(&points, temp_file.path()).unwrap();
        let content = fs::read_to_string(temp_file.path()).unwrap();
        assert!(content.contains("48"));
        assert!(content.contains("0.0"));
    }

    #[test]
    fn test_write_analyze_csv_calories_empty_when_no_power() {
        let temp_file = NamedTempFile::new().unwrap();
        let points = vec![make_analyze_point(0.0, 0.0)];
        write_analyze_csv(&points, temp_file.path()).unwrap();
        let content = fs::read_to_string(temp_file.path()).unwrap();
        let mut rdr = csv::Reader::from_reader(content.as_bytes());
        let records: Vec<_> = rdr.records().collect();
        assert_eq!(records[0].as_ref().unwrap().get(10).unwrap(), "");
    }

    #[test]
    fn test_write_analyze_csv_calories_present_when_power_given() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut point = make_analyze_point(10.0, 0.083);
        // 4.184 kJ / (0.25 × 4.184) = exactly 4 kcal
        point.cumulative_energy_kj = Some(4.184);
        write_analyze_csv(&[point], temp_file.path()).unwrap();
        let content = fs::read_to_string(temp_file.path()).unwrap();
        let mut rdr = csv::Reader::from_reader(content.as_bytes());
        let records: Vec<_> = rdr.records().collect();
        assert_eq!(records[0].as_ref().unwrap().get(10).unwrap(), "4");
    }

    #[test]
    fn test_write_analyze_csv_power_4s_empty_when_none() {
        let temp_file = NamedTempFile::new().unwrap();
        let points = vec![make_analyze_point(0.0, 0.0)];
        write_analyze_csv(&points, temp_file.path()).unwrap();
        let content = fs::read_to_string(temp_file.path()).unwrap();
        let mut rdr = csv::Reader::from_reader(content.as_bytes());
        let records: Vec<_> = rdr.records().collect();
        assert_eq!(records[0].as_ref().unwrap().get(9).unwrap(), "");
    }

    #[test]
    fn test_write_analyze_csv_power_4s_present_when_some() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut point = make_analyze_point(5.0, 0.04);
        point.power_smooth_watts = Some(185.5);
        write_analyze_csv(&[point], temp_file.path()).unwrap();
        let content = fs::read_to_string(temp_file.path()).unwrap();
        let mut rdr = csv::Reader::from_reader(content.as_bytes());
        let records: Vec<_> = rdr.records().collect();
        assert_eq!(records[0].as_ref().unwrap().get(9).unwrap(), "185.5");
    }

    #[test]
    fn test_write_intervals_csv_column_count() {
        let temp_file = NamedTempFile::new().unwrap();
        let intervals = vec![IntervalSummary {
            interval_type: crate::IntervalType::Min1,
            interval_index: 0,
            start_timestamp: Utc::now(),
            end_timestamp: Utc::now(),
            duration_seconds: 60.0,
            distance_km: 0.5,
            average_speed_kmh: 30.0,
            average_power_watts: Some(200.0),
        }];
        write_intervals_csv(&intervals, temp_file.path()).unwrap();
        let content = fs::read_to_string(temp_file.path()).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "header + 1 data row");
        assert_eq!(lines[0].split(',').count(), 8, "8 columns in header");
    }

    #[test]
    fn test_write_intervals_csv_power_some() {
        let temp_file = NamedTempFile::new().unwrap();
        let intervals = vec![IntervalSummary {
            interval_type: crate::IntervalType::Km5,
            interval_index: 2,
            start_timestamp: Utc::now(),
            end_timestamp: Utc::now(),
            duration_seconds: 600.0,
            distance_km: 5.0,
            average_speed_kmh: 30.0,
            average_power_watts: Some(250.0),
        }];
        write_intervals_csv(&intervals, temp_file.path()).unwrap();
        let content = fs::read_to_string(temp_file.path()).unwrap();
        let mut rdr = csv::Reader::from_reader(content.as_bytes());
        let records: Vec<_> = rdr.records().collect();
        assert_eq!(records.len(), 1);
        let record = records[0].as_ref().unwrap();
        assert_eq!(record.get(0).unwrap(), "5km");
        assert_eq!(record.get(1).unwrap(), "2");
        assert_eq!(record.get(7).unwrap(), "250.0");
    }

    #[test]
    fn test_write_intervals_csv_power_none_is_empty_field() {
        let temp_file = NamedTempFile::new().unwrap();
        let intervals = vec![IntervalSummary {
            interval_type: crate::IntervalType::Km1,
            interval_index: 0,
            start_timestamp: Utc::now(),
            end_timestamp: Utc::now(),
            duration_seconds: 120.0,
            distance_km: 1.0,
            average_speed_kmh: 30.0,
            average_power_watts: None,
        }];
        write_intervals_csv(&intervals, temp_file.path()).unwrap();
        let content = fs::read_to_string(temp_file.path()).unwrap();
        let mut rdr = csv::Reader::from_reader(content.as_bytes());
        let records: Vec<_> = rdr.records().collect();
        let record = records[0].as_ref().unwrap();
        assert_eq!(
            record.get(7).unwrap(),
            "",
            "power column should be empty when None"
        );
    }

    #[test]
    fn test_write_intervals_csv_all_seven_types() {
        use crate::IntervalType;
        let temp_file = NamedTempFile::new().unwrap();
        let all_types = [
            IntervalType::Min1,
            IntervalType::Min5,
            IntervalType::Min10,
            IntervalType::Min30,
            IntervalType::Km1,
            IntervalType::Km5,
            IntervalType::Km10,
        ];
        let intervals: Vec<IntervalSummary> = all_types
            .iter()
            .enumerate()
            .map(|(i, t)| IntervalSummary {
                interval_type: *t,
                interval_index: i,
                start_timestamp: Utc::now(),
                end_timestamp: Utc::now(),
                duration_seconds: 60.0,
                distance_km: 1.0,
                average_speed_kmh: 60.0,
                average_power_watts: None,
            })
            .collect();
        write_intervals_csv(&intervals, temp_file.path()).unwrap();
        let content = fs::read_to_string(temp_file.path()).unwrap();
        for t in &all_types {
            let label = t.to_string();
            assert!(content.contains(&label), "missing interval type {label}");
        }
    }
}
