use my_watts::{csv, gpx, smoothing, SavitzkyGolayConfig};
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
