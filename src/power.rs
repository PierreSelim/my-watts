use crate::config::BikeConfig;
use crate::{GpsAnalyzerError, GpsPoint, Track};
use chrono::{DateTime, Utc};

const G: f64 = 9.80665;
const RHO_AIR: f64 = 1.225;
const EARTH_RADIUS_M: f64 = 6_371_000.0;

#[derive(Debug, Clone)]
pub struct PowerConfig {
    pub rider_weight_kg: f64,
    pub bike_weight_kg: f64,
    pub bike: BikeConfig,
}

#[derive(Debug, Clone)]
pub struct PowerPoint {
    pub timestamp: DateTime<Utc>,
    pub power_watts: f64,
    pub speed_ms: f64,
    /// Dimensionless rise-over-run ratio; multiply by 100 to get percent.
    pub gradient: f64,
}

pub fn compute_power(
    track: &Track,
    config: &PowerConfig,
) -> Result<Vec<PowerPoint>, GpsAnalyzerError> {
    if track.len() < 2 {
        return Err(GpsAnalyzerError::ParseError(
            "Track must have at least 2 points to compute power".to_string(),
        ));
    }

    let total_mass = config.rider_weight_kg + config.bike_weight_kg;
    track
        .points
        .windows(2)
        .map(|w| segment_power(&w[0], &w[1], total_mass, &config.bike))
        .collect()
}

fn segment_power(
    p1: &GpsPoint,
    p2: &GpsPoint,
    total_mass_kg: f64,
    bike: &BikeConfig,
) -> Result<PowerPoint, GpsAnalyzerError> {
    let dt = (p2.timestamp - p1.timestamp).num_milliseconds() as f64 / 1000.0;
    if dt <= 0.0 {
        return Err(GpsAnalyzerError::ParseError(format!(
            "Non-positive time delta ({dt:.3}s) between consecutive GPS points"
        )));
    }

    let distance = haversine_distance(p1, p2);
    let speed = distance / dt;

    let alt_diff = match (p1.alt, p2.alt) {
        (Some(a1), Some(a2)) => a2 - a1,
        _ => 0.0,
    };
    let gradient = if distance > 0.0 {
        alt_diff / distance
    } else {
        0.0
    };
    let angle = gradient.atan();

    let f_gravity = total_mass_kg * G * angle.sin();
    let f_rolling = bike.crr * total_mass_kg * G * angle.cos();
    let f_drag = 0.5 * bike.cda * RHO_AIR * speed * speed;
    let power = ((f_gravity + f_rolling + f_drag) * speed).max(0.0);

    Ok(PowerPoint {
        timestamp: p2.timestamp,
        power_watts: power,
        speed_ms: speed,
        gradient,
    })
}

pub fn haversine_distance(p1: &GpsPoint, p2: &GpsPoint) -> f64 {
    let lat1 = p1.lat.to_radians();
    let lat2 = p2.lat.to_radians();
    let dlat = (p2.lat - p1.lat).to_radians();
    let dlon = (p2.lon - p1.lon).to_radians();

    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    EARTH_RADIUS_M * 2.0 * a.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BikeConfig;

    fn make_point(lat: f64, lon: f64, alt: f64, secs_offset: i64) -> GpsPoint {
        GpsPoint {
            lat,
            lon,
            alt: Some(alt),
            timestamp: DateTime::from_timestamp(1_700_000_000 + secs_offset, 0).unwrap(),
        }
    }

    fn road_bike() -> BikeConfig {
        BikeConfig {
            name: "road".to_string(),
            crr: 0.004,
            cda: 0.32,
            moving_speed_threshold_kmh: 3.0,
        }
    }

    fn gravel_bike() -> BikeConfig {
        BikeConfig {
            name: "gravel".to_string(),
            crr: 0.006,
            cda: 0.40,
            moving_speed_threshold_kmh: 3.0,
        }
    }

    #[test]
    fn test_flat_ground_positive_power() {
        let track = Track::new(vec![
            make_point(48.0, 2.0, 100.0, 0),
            make_point(48.001, 2.0, 100.0, 10),
        ])
        .unwrap();

        let config = PowerConfig {
            rider_weight_kg: 70.0,
            bike_weight_kg: 8.0,
            bike: road_bike(),
        };
        let points = compute_power(&track, &config).unwrap();
        assert_eq!(points.len(), 1);
        assert!(
            (points[0].power_watts - 303.5).abs() < 1.0,
            "expected ~303.5W, got {:.1}W",
            points[0].power_watts
        );
    }

    #[test]
    fn test_steep_downhill_clamps_to_zero() {
        let track = Track::new(vec![
            make_point(48.0, 2.0, 500.0, 0),
            make_point(48.001, 2.0, 200.0, 10),
        ])
        .unwrap();

        let config = PowerConfig {
            rider_weight_kg: 70.0,
            bike_weight_kg: 8.0,
            bike: road_bike(),
        };
        let points = compute_power(&track, &config).unwrap();
        assert_eq!(points[0].power_watts, 0.0);
    }

    #[test]
    fn test_heavier_rider_needs_more_power_on_climb() {
        let points = vec![
            make_point(48.0, 2.0, 100.0, 0),
            make_point(48.001, 2.0, 110.0, 10),
        ];
        let track_a = Track::new(points.clone()).unwrap();
        let track_b = Track::new(points).unwrap();

        let light = PowerConfig {
            rider_weight_kg: 60.0,
            bike_weight_kg: 8.0,
            bike: road_bike(),
        };
        let heavy = PowerConfig {
            rider_weight_kg: 90.0,
            bike_weight_kg: 8.0,
            bike: road_bike(),
        };

        let p_light = compute_power(&track_a, &light).unwrap()[0].power_watts;
        let p_heavy = compute_power(&track_b, &heavy).unwrap()[0].power_watts;
        assert!(p_heavy > p_light);
        assert!(
            (p_light - 963.2).abs() < 1.0,
            "light rider expected ~963.2W, got {p_light:.1}W"
        );
        assert!(
            (p_heavy - 1269.2).abs() < 1.0,
            "heavy rider expected ~1269.2W, got {p_heavy:.1}W"
        );
    }

    #[test]
    fn test_gravel_needs_more_power_than_road_on_flat() {
        let points = vec![
            make_point(48.0, 2.0, 100.0, 0),
            make_point(48.001, 2.0, 100.0, 10),
        ];
        let track_road = Track::new(points.clone()).unwrap();
        let track_gravel = Track::new(points).unwrap();

        let road_cfg = PowerConfig {
            rider_weight_kg: 70.0,
            bike_weight_kg: 8.0,
            bike: road_bike(),
        };
        let gravel_cfg = PowerConfig {
            rider_weight_kg: 70.0,
            bike_weight_kg: 8.0,
            bike: gravel_bike(),
        };

        let p_road = compute_power(&track_road, &road_cfg).unwrap()[0].power_watts;
        let p_gravel = compute_power(&track_gravel, &gravel_cfg).unwrap()[0].power_watts;
        assert!(
            p_gravel > p_road,
            "gravel {p_gravel:.1}W should exceed road {p_road:.1}W"
        );
    }

    #[test]
    fn test_single_point_track_errors() {
        let track = Track::new(vec![make_point(48.0, 2.0, 100.0, 0)]).unwrap();
        let config = PowerConfig {
            rider_weight_kg: 70.0,
            bike_weight_kg: 8.0,
            bike: road_bike(),
        };
        assert!(compute_power(&track, &config).is_err());
    }

    #[test]
    fn test_haversine_paris_london() {
        let paris = make_point(48.8566, 2.3522, 0.0, 0);
        let london = make_point(51.5074, -0.1278, 0.0, 0);
        let d = haversine_distance(&paris, &london);
        assert!(
            (d - 343_556.0).abs() < 1.0,
            "expected ~343556m, got {:.2}m",
            d
        );
    }
}
