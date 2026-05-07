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
    smooth_window_half: usize,
) -> (Vec<AnalyzePoint>, Vec<IntervalSummary>) {
    debug_assert_eq!(raw.len(), smoothed.len());
    let analyze_points = compute_analyze_points(
        raw,
        smoothed,
        power_points,
        moving_speed_threshold_kmh,
        smooth_window_half,
    );
    let intervals = compute_intervals(&analyze_points, power_points, moving_speed_threshold_kmh);
    (analyze_points, intervals)
}

fn compute_analyze_points(
    raw: &Track,
    smoothed: &Track,
    power_points: Option<&[PowerPoint]>,
    moving_speed_threshold_kmh: f64,
    smooth_window_half: usize,
) -> Vec<AnalyzePoint> {
    if smoothed.is_empty() {
        return vec![];
    }
    let first_ts = smoothed.points[0].timestamp;
    let mut cumulative_distance_km = 0.0;
    let mut moving_seconds = 0.0;
    let mut cumulative_energy_kj = 0.0;
    let mut power_idx = 0;
    let mut centered_window_start = 0usize;
    let mut centered_window_end = 0usize;
    let mut result = Vec::with_capacity(raw.len());

    for (i, (raw_pt, smooth_pt)) in raw.points.iter().zip(smoothed.points.iter()).enumerate() {
        let seconds_from_start =
            (smooth_pt.timestamp - first_ts).num_milliseconds() as f64 / 1000.0;

        let (distance_delta_km, dt) = if i == 0 {
            (0.0, 0.0)
        } else {
            let prev_smooth = &smoothed.points[i - 1];
            let dist_m = haversine_distance(prev_smooth, smooth_pt);
            let dt =
                (smooth_pt.timestamp - prev_smooth.timestamp).num_milliseconds() as f64 / 1000.0;
            (dist_m / 1000.0, dt)
        };

        let instant_speed_kmh = {
            let (back_idx, fwd_idx) = if smooth_window_half == 0 || i == 0 {
                (i.saturating_sub(1), i)
            } else {
                (
                    i.saturating_sub(smooth_window_half),
                    (i + smooth_window_half).min(smoothed.len() - 1),
                )
            };
            if back_idx == fwd_idx {
                0.0
            } else {
                let p_back = &smoothed.points[back_idx];
                let p_fwd = &smoothed.points[fwd_idx];
                let dist_m = haversine_distance(p_back, p_fwd);
                let total_dt =
                    (p_fwd.timestamp - p_back.timestamp).num_milliseconds() as f64 / 1000.0;
                if total_dt > 0.0 {
                    dist_m / total_dt * 3.6
                } else {
                    0.0
                }
            }
        };

        cumulative_distance_km += distance_delta_km;

        if instant_speed_kmh >= moving_speed_threshold_kmh {
            moving_seconds += dt;
        }

        // Consume power points up to this timestamp and accumulate energy (W × s / 1000 = kJ).
        // power_points[j].timestamp == smoothed.points[j+1].timestamp, so the first power point
        // is reached when i == 1, with seg_dt = time between point 0 and point 1.
        if let Some(pp) = power_points {
            while power_idx < pp.len() && pp[power_idx].timestamp <= smooth_pt.timestamp {
                let seg_dt = if power_idx == 0 {
                    (pp[0].timestamp - first_ts).num_milliseconds() as f64 / 1000.0
                } else {
                    (pp[power_idx].timestamp - pp[power_idx - 1].timestamp).num_milliseconds()
                        as f64
                        / 1000.0
                };
                cumulative_energy_kj += pp[power_idx].power_watts * seg_dt / 1000.0;
                power_idx += 1;
            }
        }

        let average_speed_kmh = if moving_seconds > 0.0 {
            cumulative_distance_km / (moving_seconds / 3600.0)
        } else {
            0.0
        };

        let power_smooth_watts: Option<f64> = if let Some(pp) = power_points {
            let half_window_ms = smooth_window_half as i64 * 1_000;
            while centered_window_end < pp.len()
                && (pp[centered_window_end].timestamp - smooth_pt.timestamp).num_milliseconds()
                    <= half_window_ms
            {
                centered_window_end += 1;
            }
            while centered_window_start < centered_window_end
                && (smooth_pt.timestamp - pp[centered_window_start].timestamp).num_milliseconds()
                    > half_window_ms
            {
                centered_window_start += 1;
            }
            if centered_window_start >= centered_window_end {
                None
            } else {
                let slice = &pp[centered_window_start..centered_window_end];
                let sum: f64 = slice.iter().map(|p| p.power_watts).sum();
                Some(sum / slice.len() as f64)
            }
        } else {
            None
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
            power_smooth_watts,
            cumulative_energy_kj: power_points.map(|_| cumulative_energy_kj),
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
            power_smooth_watts: None,
            cumulative_energy_kj: None,
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
        let (points, _) = analyze_track(&track, &track, None, 3.0, 1);
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
        let (points, _) = analyze_track(&raw, &smoothed, None, 3.0, 1);
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
        let (points, _) = analyze_track(&track, &track, None, 3.0, 1);
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
        let (points, _) = analyze_track(&track, &track, None, 3.0, 1);
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
        let (analyze_pts, _) = analyze_track(&track, &track, None, 3.0, 1);
        assert_eq!(analyze_pts[1].instant_speed_kmh, 0.0);
        assert_eq!(analyze_pts[1].distance_km, 0.0);
    }

    #[test]
    fn test_average_speed_consistent_with_distance_over_time() {
        let track = make_moving_track(20, 10);
        let (points, _) = analyze_track(&track, &track, None, 3.0, 1);
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
        let (points, _) = analyze_track(&track, &track, None, 3.0, 1);
        assert_eq!(points.len(), 17);
    }

    // --- Power propagation tests ---

    #[test]
    fn test_no_power_gives_none_in_all_intervals() {
        let track = make_moving_track(5, 10);
        let (_, intervals) = analyze_track(&track, &track, None, 3.0, 1);
        assert!(intervals.iter().all(|s| s.average_power_watts.is_none()));
    }

    #[test]
    fn test_no_power_gives_none_energy_in_all_points() {
        let track = make_moving_track(5, 10);
        let (points, _) = analyze_track(&track, &track, None, 3.0, 1);
        assert!(points.iter().all(|p| p.cumulative_energy_kj.is_none()));
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
        let (_, intervals) = analyze_track(&track, &track, Some(&power_points), 3.0, 1);
        assert!(intervals.iter().all(|s| s.average_power_watts.is_some()));
    }

    #[test]
    fn test_energy_is_zero_at_first_point() {
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
        let (points, _) = analyze_track(&track, &track, Some(&power_points), 3.0, 1);
        assert_eq!(points[0].cumulative_energy_kj, Some(0.0));
    }

    #[test]
    fn test_energy_is_monotonically_non_decreasing() {
        use crate::config::BikeConfig;
        use crate::power::{compute_power, PowerConfig};
        let track = make_moving_track(10, 10);
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
        let (points, _) = analyze_track(&track, &track, Some(&power_points), 3.0, 1);
        for w in points.windows(2) {
            assert!(
                w[1].cumulative_energy_kj >= w[0].cumulative_energy_kj,
                "energy must not decrease: {:?} < {:?}",
                w[1].cumulative_energy_kj,
                w[0].cumulative_energy_kj
            );
        }
    }

    #[test]
    fn test_energy_matches_power_times_time() {
        // Constant-power track: total energy must equal P × t / 1000
        use crate::power::PowerPoint;
        let base_ts = 1_700_000_000i64;
        let n = 5usize;
        let dt_secs = 10i64;
        let constant_power = 200.0f64;

        // Build analyze points manually (just need timestamps and distance)
        let analyze_pts: Vec<AnalyzePoint> = (0..n)
            .map(|i| make_analyze_point(i as f64 * dt_secs as f64, i as f64 * 0.05))
            .collect();

        // Build power points: N-1 points, each at t = base + (i+1)*dt, all 200 W
        let power_pts: Vec<PowerPoint> = (0..(n - 1))
            .map(|i| PowerPoint {
                timestamp: DateTime::from_timestamp(base_ts + (i as i64 + 1) * dt_secs, 0).unwrap(),
                power_watts: constant_power,
                speed_ms: 5.0,
                gradient: 0.0,
            })
            .collect();

        let intervals = compute_intervals(&analyze_pts, Some(&power_pts), 3.0);
        // Use analyze_track-style integration to verify via direct call
        // Instead: construct a minimal track and use analyze_track end-to-end
        let _ = intervals; // just silence unused warning

        // Direct test: sum energy manually and compare with expected
        // 4 segments × 200 W × 10 s / 1000 = 8 kJ total
        let expected_total_kj = (n - 1) as f64 * constant_power * dt_secs as f64 / 1000.0;

        // Re-create using the real track to exercise compute_analyze_points
        let track_pts: Vec<GpsPoint> = (0..n)
            .map(|i| make_gps_point(48.8566 + i as f64 * LAT_STEP, 2.3522, i as i64 * dt_secs))
            .collect();
        let track = Track::new(track_pts).unwrap();

        // Build matching power points aligned to the smoothed track timestamps
        let first_ts = track.points[0].timestamp;
        let aligned_power: Vec<PowerPoint> = (0..(n - 1))
            .map(|i| PowerPoint {
                timestamp: track.points[i + 1].timestamp,
                power_watts: constant_power,
                speed_ms: 5.0,
                gradient: 0.0,
            })
            .collect();

        let (points, _) = analyze_track(&track, &track, Some(&aligned_power), 3.0, 1);
        let _ = first_ts;
        let total_energy = points.last().unwrap().cumulative_energy_kj.unwrap();
        assert!(
            (total_energy - expected_total_kj).abs() < 1e-9,
            "expected {expected_total_kj} kJ, got {total_energy} kJ"
        );
    }

    // --- 10-second power window tests ---

    #[test]
    fn test_power_4s_is_none_at_first_point() {
        use crate::config::BikeConfig;
        use crate::power::{compute_power, PowerConfig};
        // 10-second intervals: the first power point is 10s ahead, outside ±2s window.
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
        let (points, _) = analyze_track(&track, &track, Some(&power_points), 3.0, 1);
        assert_eq!(
            points[0].power_smooth_watts, None,
            "first point has no power within ±2s"
        );
    }

    #[test]
    fn test_power_4s_constant_power_averages_to_that_power() {
        use crate::power::PowerPoint;
        let base_ts = 1_700_000_000i64;
        let n = 15usize;
        let track_pts: Vec<GpsPoint> = (0..n)
            .map(|i| make_gps_point(48.8566 + i as f64 * LAT_STEP, 2.3522, i as i64))
            .collect();
        let track = Track::new(track_pts).unwrap();
        let power_pts: Vec<PowerPoint> = (0..(n - 1))
            .map(|i| PowerPoint {
                timestamp: DateTime::from_timestamp(base_ts + i as i64 + 1, 0).unwrap(),
                power_watts: 200.0,
                speed_ms: 5.0,
                gradient: 0.0,
            })
            .collect();
        let (points, _) = analyze_track(&track, &track, Some(&power_pts), 3.0, 1);
        // Points 0 and 1 have power points within ±2s; all return 200 W.
        for pt in &points[..] {
            if let Some(w) = pt.power_smooth_watts {
                assert!((w - 200.0).abs() < 1e-9, "expected 200 W, got {w}");
            }
        }
    }

    #[test]
    fn test_power_smooth_centered_window_drops_spike_outside_window() {
        use crate::power::PowerPoint;
        // smooth_window_half=2 → ±2 s window.
        // Spike at pp[0] (timestamp base_ts+1, 1000 W), rest 100 W.
        // Spike is in the window of analyze points i=0..=3
        // (|base_ts+1 - (base_ts+i)| = |1-i| ≤ 2 ⟺ i ≤ 3).
        // At i=3 (t=base_ts+3): window [base_ts+1, base_ts+5] → pp[0..5]
        //   = 1000 + 4×100 = 1400 / 5 = 280 W.
        // At i=4 (t=base_ts+4): window [base_ts+2, base_ts+6] → pp[1..6]
        //   = 5×100 = 100 W (spike dropped).
        let base_ts = 1_700_000_000i64;
        let n = 15usize;
        let track_pts: Vec<GpsPoint> = (0..n)
            .map(|i| make_gps_point(48.8566 + i as f64 * LAT_STEP, 2.3522, i as i64))
            .collect();
        let track = Track::new(track_pts).unwrap();
        let power_pts: Vec<PowerPoint> = (0..(n - 1))
            .map(|i| PowerPoint {
                timestamp: DateTime::from_timestamp(base_ts + i as i64 + 1, 0).unwrap(),
                power_watts: if i == 0 { 1000.0 } else { 100.0 },
                speed_ms: 5.0,
                gradient: 0.0,
            })
            .collect();
        let (points, _) = analyze_track(&track, &track, Some(&power_pts), 3.0, 2);

        let w3 = points[3].power_smooth_watts.unwrap();
        let expected3 = (1000.0 + 4.0 * 100.0) / 5.0;
        assert!(
            (w3 - expected3).abs() < 1e-9,
            "expected {expected3:.4} W at i=3, got {w3}"
        );

        let w4 = points[4].power_smooth_watts.unwrap();
        assert!(
            (w4 - 100.0).abs() < 1e-9,
            "spike dropped at i=4, expected 100 W, got {w4}"
        );
    }

    #[test]
    fn test_power_4s_is_none_without_power_points() {
        let track = make_moving_track(5, 1);
        let (points, _) = analyze_track(&track, &track, None, 3.0, 1);
        assert!(points.iter().all(|p| p.power_smooth_watts.is_none()));
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

    #[test]
    fn test_smooth_window_reduces_spike() {
        // 11 points (i=0..=10) with 1-second spacing, moving ~36 km/h.
        // Point 5 is displaced 50 m north (GPS artifact).
        //
        // The centered window at point i uses [i-n, i+n], so the artifact at pts[5] does NOT
        // appear at i=5 itself (which uses pts[4] and pts[6]). It appears at neighboring points:
        //   i=4: window=1 uses haversine(pts[3], pts[5_displaced]) → large spike
        //         window=5 uses haversine(pts[0], pts[9])           → normal speed
        let base_lat = 48.8566_f64;
        let base_lon = 2.3522_f64;
        let artifact_deg = 50.0_f64 / 111_000.0;
        let mut pts: Vec<GpsPoint> = (0..11)
            .map(|i| GpsPoint {
                lat: base_lat + i as f64 * LAT_STEP,
                lon: base_lon,
                alt: Some(100.0),
                timestamp: DateTime::from_timestamp(1_700_000_000 + i as i64, 0).unwrap(),
            })
            .collect();
        pts[5].lat += artifact_deg;

        let track = Track::new(pts).unwrap();

        let (points_narrow, _) = analyze_track(&track, &track, None, 3.0, 1);
        let (points_wide, _) = analyze_track(&track, &track, None, 3.0, 5);

        let spike_narrow = points_narrow[4].instant_speed_kmh;
        let spike_wide = points_wide[4].instant_speed_kmh;

        assert!(
            spike_narrow > spike_wide,
            "window=1 spike ({spike_narrow:.1}) should exceed window=5 spike ({spike_wide:.1})"
        );
    }
}
