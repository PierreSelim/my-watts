use crate::power::{haversine_distance, PowerPoint};
use crate::{AnalyzePoint, IntervalSummary, Track};
use std::collections::BTreeMap;

enum IntervalKind {
    Time { window_secs: f64 },
    Distance { window_km: f64 },
}

struct IntervalSpec {
    label: &'static str,
    kind: IntervalKind,
}

fn interval_specs() -> Vec<IntervalSpec> {
    vec![
        IntervalSpec {
            label: "1min",
            kind: IntervalKind::Time { window_secs: 60.0 },
        },
        IntervalSpec {
            label: "5min",
            kind: IntervalKind::Time { window_secs: 300.0 },
        },
        IntervalSpec {
            label: "10min",
            kind: IntervalKind::Time { window_secs: 600.0 },
        },
        IntervalSpec {
            label: "30min",
            kind: IntervalKind::Time {
                window_secs: 1800.0,
            },
        },
        IntervalSpec {
            label: "1km",
            kind: IntervalKind::Distance { window_km: 1.0 },
        },
        IntervalSpec {
            label: "5km",
            kind: IntervalKind::Distance { window_km: 5.0 },
        },
        IntervalSpec {
            label: "10km",
            kind: IntervalKind::Distance { window_km: 10.0 },
        },
    ]
}

pub fn analyze_track(
    raw: &Track,
    smoothed: &Track,
    power_points: Option<&[PowerPoint]>,
    moving_speed_threshold_kmh: f64,
) -> (Vec<AnalyzePoint>, Vec<IntervalSummary>) {
    debug_assert_eq!(raw.len(), smoothed.len());
    let analyze_points = compute_analyze_points(raw, smoothed, moving_speed_threshold_kmh);
    let intervals = compute_intervals(&analyze_points, power_points, moving_speed_threshold_kmh);
    (analyze_points, intervals)
}

fn compute_analyze_points(
    raw: &Track,
    smoothed: &Track,
    moving_speed_threshold_kmh: f64,
) -> Vec<AnalyzePoint> {
    if smoothed.is_empty() {
        return vec![];
    }
    let first_ts = smoothed.points[0].timestamp;
    let mut cumulative_distance_km = 0.0;
    let mut moving_seconds = 0.0;
    let mut result = Vec::with_capacity(raw.len());

    for (i, (raw_pt, smooth_pt)) in raw.points.iter().zip(smoothed.points.iter()).enumerate() {
        let seconds_from_start =
            (smooth_pt.timestamp - first_ts).num_milliseconds() as f64 / 1000.0;

        let (instant_speed_kmh, distance_delta_km, dt) = if i == 0 {
            (0.0, 0.0, 0.0)
        } else {
            let prev_smooth = &smoothed.points[i - 1];
            let dist_m = haversine_distance(prev_smooth, smooth_pt);
            let dt =
                (smooth_pt.timestamp - prev_smooth.timestamp).num_milliseconds() as f64 / 1000.0;
            let speed_kmh = if dt > 0.0 { dist_m / dt * 3.6 } else { 0.0 };
            (speed_kmh, dist_m / 1000.0, dt)
        };

        cumulative_distance_km += distance_delta_km;

        if instant_speed_kmh >= moving_speed_threshold_kmh {
            moving_seconds += dt;
        }

        let average_speed_kmh = if moving_seconds > 0.0 {
            cumulative_distance_km / (moving_seconds / 3600.0)
        } else {
            0.0
        };

        result.push(AnalyzePoint {
            timestamp: smooth_pt.timestamp,
            seconds_from_start,
            moving_seconds_from_start: moving_seconds,
            raw_lat: raw_pt.lat,
            raw_lon: raw_pt.lon,
            smoothed_lat: smooth_pt.lat,
            smoothed_lon: smooth_pt.lon,
            instant_speed_kmh,
            average_speed_kmh,
            distance_km: cumulative_distance_km,
        });
    }

    result
}

fn compute_intervals(
    analyze_points: &[AnalyzePoint],
    power_points: Option<&[PowerPoint]>,
    moving_speed_threshold_kmh: f64,
) -> Vec<IntervalSummary> {
    if analyze_points.is_empty() {
        return vec![];
    }

    let specs = interval_specs();
    let mut summaries = Vec::new();

    for spec in &specs {
        let mut buckets: BTreeMap<usize, Vec<&AnalyzePoint>> = BTreeMap::new();
        for point in analyze_points {
            let bucket_idx = match &spec.kind {
                IntervalKind::Time { window_secs } => {
                    (point.seconds_from_start / window_secs).floor() as usize
                }
                IntervalKind::Distance { window_km } => {
                    (point.distance_km / window_km).floor() as usize
                }
            };
            buckets.entry(bucket_idx).or_default().push(point);
        }

        for (bucket_idx, bucket_points) in &buckets {
            let first = bucket_points[0];
            let last = *bucket_points.last().unwrap();
            let duration_seconds = last.seconds_from_start - first.seconds_from_start;
            let moving_duration_seconds =
                last.moving_seconds_from_start - first.moving_seconds_from_start;
            let distance_km = last.distance_km - first.distance_km;
            let average_speed_kmh = if moving_duration_seconds > 0.0 {
                distance_km / (moving_duration_seconds / 3600.0)
            } else {
                0.0
            };

            let average_power_watts = power_points.map(|pp| {
                let powers: Vec<f64> = pp
                    .iter()
                    .filter(|p| {
                        p.timestamp >= first.timestamp
                            && p.timestamp <= last.timestamp
                            && p.speed_ms * 3.6 >= moving_speed_threshold_kmh
                    })
                    .map(|p| p.power_watts)
                    .collect();
                if powers.is_empty() {
                    0.0
                } else {
                    powers.iter().sum::<f64>() / powers.len() as f64
                }
            });

            summaries.push(IntervalSummary {
                interval_type: spec.label.to_string(),
                interval_index: *bucket_idx,
                start_timestamp: first.timestamp,
                end_timestamp: last.timestamp,
                duration_seconds,
                distance_km,
                average_speed_kmh,
                average_power_watts,
            });
        }
    }

    summaries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GpsPoint, Track};
    use chrono::DateTime;

    fn make_gps_point(lat: f64, lon: f64, secs_offset: i64) -> GpsPoint {
        GpsPoint {
            lat,
            lon,
            alt: Some(100.0),
            timestamp: DateTime::from_timestamp(1_700_000_000 + secs_offset, 0).unwrap(),
        }
    }

    fn make_analyze_point(seconds: f64, distance_km: f64) -> AnalyzePoint {
        AnalyzePoint {
            timestamp: DateTime::from_timestamp(1_700_000_000 + seconds as i64, 0).unwrap(),
            seconds_from_start: seconds,
            moving_seconds_from_start: seconds,
            raw_lat: 0.0,
            raw_lon: 0.0,
            smoothed_lat: 0.0,
            smoothed_lon: 0.0,
            instant_speed_kmh: 30.0,
            average_speed_kmh: 0.0,
            distance_km,
        }
    }

    // ~10m north per step at Paris latitude
    const LAT_STEP: f64 = 0.000090;

    fn make_moving_track(n_points: usize, dt_secs: i64) -> Track {
        let points = (0..n_points)
            .map(|i| make_gps_point(48.8566 + i as f64 * LAT_STEP, 2.3522, i as i64 * dt_secs))
            .collect();
        Track::new(points).unwrap()
    }

    // --- compute_analyze_points tests ---

    #[test]
    fn test_first_point_all_zeroes() {
        let track = make_moving_track(5, 10);
        let (points, _) = analyze_track(&track, &track, None, 3.0);
        let first = &points[0];
        assert_eq!(first.seconds_from_start, 0.0);
        assert_eq!(first.instant_speed_kmh, 0.0);
        assert_eq!(first.distance_km, 0.0);
        assert_eq!(first.average_speed_kmh, 0.0);
    }

    #[test]
    fn test_raw_and_smoothed_coords_preserved() {
        let raw = make_moving_track(3, 10);
        let smoothed = make_moving_track(3, 10);
        let (points, _) = analyze_track(&raw, &smoothed, None, 3.0);
        for (i, pt) in points.iter().enumerate() {
            assert_eq!(pt.raw_lat, raw.points[i].lat);
            assert_eq!(pt.raw_lon, raw.points[i].lon);
            assert_eq!(pt.smoothed_lat, smoothed.points[i].lat);
            assert_eq!(pt.smoothed_lon, smoothed.points[i].lon);
        }
    }

    #[test]
    fn test_monotonic_distance() {
        let track = make_moving_track(10, 10);
        let (points, _) = analyze_track(&track, &track, None, 3.0);
        for w in points.windows(2) {
            assert!(
                w[1].distance_km >= w[0].distance_km,
                "distance must not decrease: {} < {}",
                w[1].distance_km,
                w[0].distance_km
            );
        }
    }

    #[test]
    fn test_two_point_average_speed_equals_instant_speed() {
        let track = make_moving_track(2, 1);
        let (points, _) = analyze_track(&track, &track, None, 3.0);
        assert_eq!(points.len(), 2);
        let second = &points[1];
        assert!((second.average_speed_kmh - second.instant_speed_kmh).abs() < 1e-9);
        assert!(second.instant_speed_kmh > 0.0);
    }

    #[test]
    fn test_stationary_points_have_zero_speed() {
        let pts = vec![
            make_gps_point(48.8566, 2.3522, 0),
            make_gps_point(48.8566, 2.3522, 10),
        ];
        let track = Track::new(pts).unwrap();
        let (analyze_pts, _) = analyze_track(&track, &track, None, 3.0);
        assert_eq!(analyze_pts[1].instant_speed_kmh, 0.0);
        assert_eq!(analyze_pts[1].distance_km, 0.0);
    }

    #[test]
    fn test_average_speed_consistent_with_distance_over_time() {
        let track = make_moving_track(20, 10);
        let (points, _) = analyze_track(&track, &track, None, 3.0);
        for pt in &points {
            if pt.seconds_from_start > 0.0 {
                let expected = pt.distance_km / (pt.seconds_from_start / 3600.0);
                assert!(
                    (pt.average_speed_kmh - expected).abs() < 1e-9,
                    "mismatch at {}s: got {}, expected {}",
                    pt.seconds_from_start,
                    pt.average_speed_kmh,
                    expected
                );
            }
        }
    }

    #[test]
    fn test_output_length_matches_input() {
        let track = make_moving_track(17, 5);
        let (points, _) = analyze_track(&track, &track, None, 3.0);
        assert_eq!(points.len(), 17);
    }

    // --- Power propagation tests ---

    #[test]
    fn test_no_power_gives_none_in_all_intervals() {
        let track = make_moving_track(5, 10);
        let (_, intervals) = analyze_track(&track, &track, None, 3.0);
        assert!(intervals.iter().all(|s| s.average_power_watts.is_none()));
    }

    #[test]
    fn test_with_power_gives_some_in_all_intervals() {
        use crate::config::BikeConfig;
        use crate::power::{compute_power, PowerConfig};
        let track = make_moving_track(5, 10);
        let power_cfg = PowerConfig {
            rider_weight_kg: 70.0,
            bike_weight_kg: 8.0,
            bike: BikeConfig {
                name: "road".to_string(),
                crr: 0.004,
                cda: 0.32,
                moving_speed_threshold_kmh: 3.0,
            },
        };
        let power_points = compute_power(&track, &power_cfg).unwrap();
        let (_, intervals) = analyze_track(&track, &track, Some(&power_points), 3.0);
        assert!(intervals.iter().all(|s| s.average_power_watts.is_some()));
    }

    // --- compute_intervals tests using direct AnalyzePoint construction ---

    #[test]
    fn test_time_intervals_1min_three_buckets() {
        // t=0..179s → floor(t/60) ∈ {0,1,2}
        let points: Vec<AnalyzePoint> = (0..180)
            .map(|i| make_analyze_point(i as f64, i as f64 * 0.005))
            .collect();
        let intervals = compute_intervals(&points, None, 3.0);
        let count = intervals
            .iter()
            .filter(|s| s.interval_type == "1min")
            .count();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_time_intervals_all_sizes_on_30min_track() {
        // t=0..1799s (30min - 1s)
        let points: Vec<AnalyzePoint> = (0..1800)
            .map(|i| make_analyze_point(i as f64, i as f64 * 0.005))
            .collect();
        let intervals = compute_intervals(&points, None, 3.0);
        assert_eq!(
            intervals
                .iter()
                .filter(|s| s.interval_type == "1min")
                .count(),
            30
        );
        assert_eq!(
            intervals
                .iter()
                .filter(|s| s.interval_type == "5min")
                .count(),
            6
        );
        assert_eq!(
            intervals
                .iter()
                .filter(|s| s.interval_type == "10min")
                .count(),
            3
        );
        assert_eq!(
            intervals
                .iter()
                .filter(|s| s.interval_type == "30min")
                .count(),
            1
        );
    }

    #[test]
    fn test_distance_intervals_1km_five_buckets() {
        // distance 0.0..4.99km → floor(d/1) ∈ {0,1,2,3,4}
        let points: Vec<AnalyzePoint> = (0..500)
            .map(|i| make_analyze_point(i as f64, i as f64 * 0.01))
            .collect();
        let intervals = compute_intervals(&points, None, 3.0);
        let count = intervals
            .iter()
            .filter(|s| s.interval_type == "1km")
            .count();
        assert_eq!(count, 5);
    }

    #[test]
    fn test_distance_intervals_all_sizes_on_12km_track() {
        // distance 0.0..12.0km
        let points: Vec<AnalyzePoint> = (0..1201)
            .map(|i| make_analyze_point(i as f64, i as f64 * 0.01))
            .collect();
        let intervals = compute_intervals(&points, None, 3.0);
        assert_eq!(
            intervals
                .iter()
                .filter(|s| s.interval_type == "1km")
                .count(),
            13
        );
        assert_eq!(
            intervals
                .iter()
                .filter(|s| s.interval_type == "5km")
                .count(),
            3
        );
        assert_eq!(
            intervals
                .iter()
                .filter(|s| s.interval_type == "10km")
                .count(),
            2
        );
    }

    #[test]
    fn test_interval_duration_sum_for_1min() {
        // 180 points t=0..179s → 3 buckets, each duration = last - first within bucket = 59s
        let points: Vec<AnalyzePoint> = (0..180)
            .map(|i| make_analyze_point(i as f64, i as f64 * 0.005))
            .collect();
        let intervals = compute_intervals(&points, None, 3.0);
        let total_dur: f64 = intervals
            .iter()
            .filter(|s| s.interval_type == "1min")
            .map(|s| s.duration_seconds)
            .sum();
        assert!(
            (total_dur - 177.0).abs() < 1e-6,
            "expected 177s, got {total_dur}"
        );
    }

    #[test]
    fn test_interval_distance_sum_for_1min() {
        // 3 buckets each covering 59 steps of 0.005km = 0.295km each → 0.885km total
        let points: Vec<AnalyzePoint> = (0..180)
            .map(|i| make_analyze_point(i as f64, i as f64 * 0.005))
            .collect();
        let intervals = compute_intervals(&points, None, 3.0);
        let total_dist: f64 = intervals
            .iter()
            .filter(|s| s.interval_type == "1min")
            .map(|s| s.distance_km)
            .sum();
        assert!(
            (total_dist - (3.0 * 59.0 * 0.005)).abs() < 1e-9,
            "got {total_dist}"
        );
    }

    #[test]
    fn test_short_ride_all_points_in_bucket_zero() {
        // 31 points t=0..30s < 1min, distance < 1km: all fall in bucket 0 for every spec
        let points: Vec<AnalyzePoint> = (0..31)
            .map(|i| make_analyze_point(i as f64, i as f64 * 0.001))
            .collect();
        let intervals = compute_intervals(&points, None, 3.0);
        assert_eq!(intervals.len(), 7); // one bucket per spec
        assert!(intervals.iter().all(|s| s.interval_index == 0));
    }

    #[test]
    fn test_empty_analyze_points_produces_no_intervals() {
        let intervals = compute_intervals(&[], None, 3.0);
        assert!(intervals.is_empty());
    }

    #[test]
    fn test_interval_power_averaging() {
        use crate::power::PowerPoint;

        let points = vec![
            make_analyze_point(0.0, 0.0),
            make_analyze_point(30.0, 0.15),
            make_analyze_point(59.0, 0.295),
        ];

        let power_pts = vec![
            PowerPoint {
                timestamp: DateTime::from_timestamp(1_700_000_030, 0).unwrap(),
                power_watts: 200.0,
                speed_ms: 5.0,
                gradient: 0.0,
            },
            PowerPoint {
                timestamp: DateTime::from_timestamp(1_700_000_059, 0).unwrap(),
                power_watts: 300.0,
                speed_ms: 5.0,
                gradient: 0.0,
            },
        ];

        let intervals = compute_intervals(&points, Some(&power_pts), 3.0);
        let one_min: Vec<_> = intervals
            .iter()
            .filter(|s| s.interval_type == "1min")
            .collect();
        assert_eq!(one_min.len(), 1);
        let avg = one_min[0].average_power_watts.unwrap();
        assert!((avg - 250.0).abs() < 1e-6, "expected 250W, got {avg}");
    }

    #[test]
    fn test_interval_indices_are_sequential_for_uniform_track() {
        // 5 points at t=0,60,120,180,240 → 5 distinct "1min" buckets
        let points: Vec<AnalyzePoint> = (0..5)
            .map(|i| make_analyze_point(i as f64 * 60.0, i as f64 * 0.5))
            .collect();
        let intervals = compute_intervals(&points, None, 3.0);
        let mut one_min: Vec<_> = intervals
            .iter()
            .filter(|s| s.interval_type == "1min")
            .collect();
        one_min.sort_by_key(|s| s.interval_index);
        let indices: Vec<usize> = one_min.iter().map(|s| s.interval_index).collect();
        assert_eq!(indices, vec![0, 1, 2, 3, 4]);
    }
}
