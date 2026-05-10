use my_watts::{analyze, config::BikeConfig, csv, gpx, power, smoothing, SavitzkyGolayConfig};
use std::fs;
use tempfile::NamedTempFile;

#[test]
fn test_full_pipeline_gpx_to_csv() {
    let gpx_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="test">
  <trk>
    <trkseg>
      <trkpt lat="40.0" lon="-120.0">
        <ele>100.0</ele>
        <time>2026-04-27T10:00:00Z</time>
      </trkpt>
      <trkpt lat="40.001" lon="-120.001">
        <ele>105.0</ele>
        <time>2026-04-27T10:01:00Z</time>
      </trkpt>
      <trkpt lat="40.002" lon="-120.002">
        <ele>110.0</ele>
        <time>2026-04-27T10:02:00Z</time>
      </trkpt>
      <trkpt lat="40.003" lon="-120.003">
        <ele>115.0</ele>
        <time>2026-04-27T10:03:00Z</time>
      </trkpt>
      <trkpt lat="40.004" lon="-120.004">
        <ele>120.0</ele>
        <time>2026-04-27T10:04:00Z</time>
      </trkpt>
    </trkseg>
  </trk>
</gpx>"#;

    // Parse GPX
    let track = gpx::parse_gpx(gpx_content).expect("Failed to parse GPX");
    assert_eq!(track.len(), 5);

    // Smooth
    let config = SavitzkyGolayConfig::new(3, 1).expect("Failed to create config");
    let smoothed = smoothing::smooth_track(&track, config).expect("Failed to smooth");
    assert_eq!(smoothed.len(), track.len());

    // Write CSV
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let csv_path = temp_file.path().to_path_buf();
    csv::write_csv(&smoothed, &csv_path).expect("Failed to write CSV");

    // Verify CSV file exists and has content
    let content = fs::read_to_string(&csv_path).expect("Failed to read CSV");
    assert!(content.contains("latitude,longitude,altitude,timestamp"));
    assert!(content.contains("40"));
    assert!(content.contains("-120"));
    assert!(content.lines().count() >= 6); // header + 5 points
}

const FIVE_POINT_GPX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1">
  <trk>
    <trkseg>
      <trkpt lat="48.8566" lon="2.3522"><ele>50.0</ele><time>2024-01-01T10:00:00Z</time></trkpt>
      <trkpt lat="48.8575" lon="2.3530"><ele>51.0</ele><time>2024-01-01T10:00:10Z</time></trkpt>
      <trkpt lat="48.8584" lon="2.3538"><ele>52.0</ele><time>2024-01-01T10:00:20Z</time></trkpt>
      <trkpt lat="48.8593" lon="2.3546"><ele>53.0</ele><time>2024-01-01T10:00:30Z</time></trkpt>
      <trkpt lat="48.8602" lon="2.3554"><ele>54.0</ele><time>2024-01-01T10:00:40Z</time></trkpt>
    </trkseg>
  </trk>
</gpx>"#;

fn road_bike() -> BikeConfig {
    BikeConfig { name: "road".to_string(), crr: 0.004, cda: 0.32, moving_speed_threshold_kmh: 3.0 }
}

#[test]
fn test_analyze_pipeline_invariants() {
    let track = gpx::parse_gpx(FIVE_POINT_GPX).expect("parse GPX");
    let sg = SavitzkyGolayConfig::new(3, 1).unwrap();
    let smoothed = smoothing::smooth_track(&track, sg).unwrap();
    let pc = power::PowerConfig { rider_weight_kg: 75.0, bike_weight_kg: 10.0, bike: road_bike() };
    let power_pts = power::compute_power(&smoothed, &pc).unwrap();
    assert_eq!(power_pts.len(), 4);

    let (pts, intervals) = analyze::analyze_track(&track, &smoothed, Some(&power_pts), 3.0, 1);
    assert_eq!(pts.len(), 5);
    assert!(!intervals.is_empty());

    // First-point zeroes
    assert_eq!(pts[0].seconds_from_start, 0.0);
    assert_eq!(pts[0].distance_km, 0.0);
    assert_eq!(pts[0].average_speed_kmh, 0.0);
    assert_eq!(pts[0].cumulative_energy_kj, Some(0.0));

    // Monotonic distance and non-decreasing energy
    for w in pts.windows(2) {
        assert!(w[1].distance_km >= w[0].distance_km);
        assert!(w[1].cumulative_energy_kj >= w[0].cumulative_energy_kj);
    }

    // Power window produces at least one non-None value
    assert!(pts.iter().any(|p| p.power_smooth_watts.is_some()));
}

#[test]
fn test_analyze_csv_roundtrip() {
    let track = gpx::parse_gpx(FIVE_POINT_GPX).expect("parse GPX");
    let sg = SavitzkyGolayConfig::new(3, 1).unwrap();
    let smoothed = smoothing::smooth_track(&track, sg).unwrap();
    let pc = power::PowerConfig { rider_weight_kg: 75.0, bike_weight_kg: 10.0, bike: road_bike() };
    let power_pts = power::compute_power(&smoothed, &pc).unwrap();
    let (pts, intervals) = analyze::analyze_track(&track, &smoothed, Some(&power_pts), 3.0, 1);

    let analyze_file = NamedTempFile::new().unwrap();
    csv::write_analyze_csv(&pts, analyze_file.path()).expect("write analyze csv");
    let content = fs::read_to_string(analyze_file.path()).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 6, "header + 5 data rows");
    assert_eq!(lines[0].split(',').count(), 11, "11 columns");

    let intervals_file = NamedTempFile::new().unwrap();
    csv::write_intervals_csv(&intervals, intervals_file.path()).expect("write intervals csv");
    let int_content = fs::read_to_string(intervals_file.path()).unwrap();
    // At least header + one row per interval type
    assert!(int_content.lines().count() > 7);
}
