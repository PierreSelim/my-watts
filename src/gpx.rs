use crate::{GpsAnalyzerError, GpsPoint, Track};
use chrono::DateTime;
use quick_xml::de::from_str;
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Gpx {
    #[serde(default)]
    pub trk: Vec<GpxTrack>,
}

#[derive(Debug, Deserialize)]
pub struct GpxTrack {
    #[serde(default)]
    pub trkseg: Vec<GpxTrackSegment>,
}

#[derive(Debug, Deserialize)]
pub struct GpxTrackSegment {
    #[serde(default)]
    pub trkpt: Vec<GpxTrackPoint>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GpxTrackPoint {
    #[serde(rename = "@lat")]
    pub lat: f64,
    #[serde(rename = "@lon")]
    pub lon: f64,
    #[serde(default, rename = "ele")]
    pub ele: Option<f64>,
    #[serde(default, rename = "time")]
    pub time: Option<String>,
}

pub fn load_gpx<P: AsRef<Path>>(path: P) -> Result<Track, GpsAnalyzerError> {
    let content = fs::read_to_string(path).map_err(GpsAnalyzerError::Io)?;

    parse_gpx(&content)
}

pub fn parse_gpx(content: &str) -> Result<Track, GpsAnalyzerError> {
    let gpx: Gpx = from_str(content).map_err(|e| GpsAnalyzerError::InvalidGpx(e.to_string()))?;

    let mut points = Vec::new();

    for track in gpx.trk {
        for segment in track.trkseg {
            for trkpt in segment.trkpt {
                let timestamp = if let Some(time_str) = &trkpt.time {
                    DateTime::parse_from_rfc3339(time_str)
                        .map_err(|e| {
                            GpsAnalyzerError::ParseError(format!("Invalid timestamp: {}", e))
                        })?
                        .with_timezone(&chrono::Utc)
                } else {
                    chrono::Utc::now()
                };

                points.push(GpsPoint {
                    lat: trkpt.lat,
                    lon: trkpt.lon,
                    alt: trkpt.ele,
                    timestamp,
                });
            }
        }
    }

    Track::new(points)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gpx_minimal() {
        let gpx_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1">
    <trk>
        <trkseg>
            <trkpt lat="40.0" lon="-120.0">
                <ele>100.0</ele>
                <time>2026-04-27T10:00:00Z</time>
            </trkpt>
            <trkpt lat="40.1" lon="-120.1">
                <ele>105.0</ele>
                <time>2026-04-27T10:01:00Z</time>
            </trkpt>
        </trkseg>
    </trk>
</gpx>"#;

        let track = parse_gpx(gpx_content).expect("Failed to parse GPX");
        assert_eq!(track.len(), 2);
        assert_eq!(track.points[0].lat, 40.0);
        assert_eq!(track.points[0].lon, -120.0);
        assert_eq!(track.points[0].alt, Some(100.0));
    }

    #[test]
    fn test_parse_gpx_no_elevation() {
        let gpx_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1">
    <trk>
        <trkseg>
            <trkpt lat="40.0" lon="-120.0">
                <time>2026-04-27T10:00:00Z</time>
            </trkpt>
        </trkseg>
    </trk>
</gpx>"#;

        let track = parse_gpx(gpx_content).expect("Failed to parse GPX");
        assert_eq!(track.len(), 1);
        assert_eq!(track.points[0].alt, None);
    }

    #[test]
    fn test_parse_gpx_invalid() {
        let gpx_content = "not xml";
        assert!(parse_gpx(gpx_content).is_err());
    }
}
