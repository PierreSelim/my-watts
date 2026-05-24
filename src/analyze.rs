use crate::power::PowerPoint;
use crate::{haversine_distance, AnalyzePoint, IntervalSummary, IntervalType, Track};
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

enum IntervalKind {
    Time { window_ms: i64 },
    Distance { window_m: i64 },
}

struct IntervalSpec {
    interval_type: IntervalType,
    kind: IntervalKind,
}

fn interval_specs() -> Vec<IntervalSpec> {
    vec![
        IntervalSpec {
            interval_type: IntervalType::Min1,
            kind: IntervalKind::Time { window_ms: 60_000 },
        },
        IntervalSpec {
            interval_type: IntervalType::Min5,
            kind: IntervalKind::Time { window_ms: 300_000 },
        },
        IntervalSpec {
            interval_type: IntervalType::Min10,
            kind: IntervalKind::Time { window_ms: 600_000 },
        },
        IntervalSpec {
            interval_type: IntervalType::Min30,
            kind: IntervalKind::Time {
                window_ms: 1_800_000,
            },
        },
        IntervalSpec {
            interval_type: IntervalType::Km1,
            kind: IntervalKind::Distance { window_m: 1_000 },
        },
        IntervalSpec {
            interval_type: IntervalType::Km5,
            kind: IntervalKind::Distance { window_m: 5_000 },
        },
        IntervalSpec {
            interval_type: IntervalType::Km10,
            kind: IntervalKind::Distance { window_m: 10_000 },
        },
    ]
}

fn power_window_average(
    pp: &[PowerPoint],
    smooth_ts: DateTime<Utc>,
    half_window_ms: i64,
    start: &mut usize,
    end: &mut usize,
) -> Option<f64> {
    while *end < pp.len() && (pp[*end].timestamp - smooth_ts).num_milliseconds() <= half_window_ms {
        *end += 1;
    }
    while *start < *end && (smooth_ts - pp[*start].timestamp).num_milliseconds() > half_window_ms {
        *start += 1;
    }
    if *start >= *end {
        None
    } else {
        let slice = &pp[*start..*end];
        Some(slice.iter().map(|p| p.power_watts).sum::<f64>() / slice.len() as f64)
    }
}

pub fn analyze_track(
    raw: &Track,
    smoothed: &Track,
    power_points: Option<&[PowerPoint]>,
    moving_speed_threshold_kmh: f64,
    smooth_window_half: usize,
) -> (Vec<AnalyzePoint>, Vec<IntervalSummary>) {
    assert_eq!(
        raw.len(),
        smoothed.len(),
        "raw and smoothed track lengths must match"
    );
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

        // power_points[j].timestamp == smoothed.points[j+1].timestamp; the first power
        // point is consumed at i == 1, seg_dt covering [point 0, point 1].
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

        let power_smooth_watts: Option<f64> = power_points.and_then(|pp| {
            let half_window_ms = smooth_window_half as i64 * 1_000;
            power_window_average(
                pp,
                smooth_pt.timestamp,
                half_window_ms,
                &mut centered_window_start,
                &mut centered_window_end,
            )
        });

        result.push(AnalyzePoint {
            timestamp: smooth_pt.timestamp,
            seconds_from_start,
            moving_seconds_from_start: moving_seconds,
            raw_lat: raw_pt.lat,
            raw_lon: raw_pt.lon,
            smoothed_lat: smooth_pt.lat,
            smoothed_lon: smooth_pt.lon,
            smoothed_alt: smooth_pt.alt,
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
                IntervalKind::Time { window_ms } => {
                    ((point.seconds_from_start * 1000.0).round() as i64 / window_ms) as usize
                }
                IntervalKind::Distance { window_m } => {
                    ((point.distance_km * 1000.0).round() as i64 / window_m) as usize
                }
            };
            buckets.entry(bucket_idx).or_default().push(point);
        }

        for (bucket_idx, bucket_points) in &buckets {
            let (Some(first), Some(last)) = (bucket_points.first(), bucket_points.last()) else {
                continue;
            };
            let (first, last) = (*first, *last);
            let duration_seconds = last.seconds_from_start - first.seconds_from_start;
            let distance_km = last.distance_km - first.distance_km;
            let average_speed_kmh = if duration_seconds > 0.0 {
                distance_km / (duration_seconds / 3600.0)
            } else {
                0.0
            };

            let average_power_watts = power_points.and_then(|pp| {
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
                    None
                } else {
                    Some(powers.iter().sum::<f64>() / powers.len() as f64)
                }
            });

            summaries.push(IntervalSummary {
                interval_type: spec.interval_type,
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

fn collect_stop_intervals(points: &[AnalyzePoint], threshold_kmh: f64) -> Vec<(f64, f64)> {
    let mut intervals: Vec<(f64, f64)> = Vec::new();
    let mut stop_start: Option<f64> = None;
    let mut last_stop_time = 0.0_f64;
    for pt in points {
        if pt.instant_speed_kmh < threshold_kmh {
            if stop_start.is_none() {
                stop_start = Some(pt.seconds_from_start);
            }
            last_stop_time = pt.seconds_from_start;
        } else if let Some(s) = stop_start.take() {
            intervals.push((s, last_stop_time));
        }
    }
    if let Some(s) = stop_start {
        intervals.push((s, last_stop_time));
    }
    intervals
}

fn buffer_intervals(intervals: &[(f64, f64)], buffer_secs: f64) -> Vec<(f64, f64)> {
    let mut buffered: Vec<(f64, f64)> = intervals
        .iter()
        .map(|(s, e)| (s - buffer_secs, e + buffer_secs))
        .collect();
    buffered.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    buffered
}

fn in_buffered_zone(buffered: &[(f64, f64)], t: f64) -> bool {
    let idx = buffered.partition_point(|(s, _)| *s <= t);
    idx > 0 && buffered[idx - 1].1 >= t
}

pub fn compute_training_speed_kmh(
    analyze_points: &[AnalyzePoint],
    moving_speed_threshold_kmh: f64,
    stop_buffer_secs: f64,
) -> f64 {
    let stop_intervals = collect_stop_intervals(analyze_points, moving_speed_threshold_kmh);
    let buffered = buffer_intervals(&stop_intervals, stop_buffer_secs);

    let (dist_km, time_secs) =
        analyze_points
            .windows(2)
            .fold((0.0_f64, 0.0_f64), |(acc_dist, acc_time), w| {
                let prev = &w[0];
                let curr = &w[1];
                if !in_buffered_zone(&buffered, prev.seconds_from_start)
                    && !in_buffered_zone(&buffered, curr.seconds_from_start)
                {
                    (
                        acc_dist + curr.distance_km - prev.distance_km,
                        acc_time + curr.seconds_from_start - prev.seconds_from_start,
                    )
                } else {
                    (acc_dist, acc_time)
                }
            });

    if time_secs > 0.0 {
        dist_km / (time_secs / 3600.0)
    } else {
        0.0
    }
}

pub fn compute_elevation_gain_m(points: &[AnalyzePoint]) -> Option<f64> {
    points
        .windows(2)
        .filter_map(|w| {
            let curr = w[1].smoothed_alt?;
            let prev = w[0].smoothed_alt?;
            Some((curr - prev).max(0.0))
        })
        .reduce(|a, b| a + b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GpsPoint, IntervalType, Track};
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
            smoothed_alt: None,
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
    fn test_average_speed_consistent_with_distance_over_moving_time() {
        let track = make_moving_track(20, 10);
        let (points, _) = analyze_track(&track, &track, None, 3.0, 1);
        for pt in &points {
            if pt.moving_seconds_from_start > 0.0 {
                let expected = pt.distance_km / (pt.moving_seconds_from_start / 3600.0);
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
    fn test_average_speed_uses_moving_not_elapsed_time() {
        // pt0→pt1: stationary (same location, 10s), pt1→pt2: moving (~10m, 10s)
        // elapsed = 20s, moving ≈ 10s → moving-based speed is ~2× the elapsed-based speed
        let pts = vec![
            make_gps_point(48.8566, 2.3522, 0),
            make_gps_point(48.8566, 2.3522, 10),
            make_gps_point(48.8566 + LAT_STEP, 2.3522, 20),
        ];
        let track = Track::new(pts).unwrap();
        let (points, _) = analyze_track(&track, &track, None, 3.0, 0);
        let last = points.last().unwrap();
        assert!(
            last.moving_seconds_from_start < last.seconds_from_start,
            "test requires a stationary period"
        );
        let expected = last.distance_km / (last.moving_seconds_from_start / 3600.0);
        assert!(
            (last.average_speed_kmh - expected).abs() < 1e-9,
            "expected moving-based avg speed {expected:.6}, got {:.6}",
            last.average_speed_kmh
        );
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
        let aligned_power: Vec<PowerPoint> = (0..(n - 1))
            .map(|i| PowerPoint {
                timestamp: track.points[i + 1].timestamp,
                power_watts: constant_power,
                speed_ms: 5.0,
                gradient: 0.0,
            })
            .collect();

        let (points, _) = analyze_track(&track, &track, Some(&aligned_power), 3.0, 1);
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
            .filter(|s| s.interval_type == IntervalType::Min1)
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
                .filter(|s| s.interval_type == IntervalType::Min1)
                .count(),
            30
        );
        assert_eq!(
            intervals
                .iter()
                .filter(|s| s.interval_type == IntervalType::Min5)
                .count(),
            6
        );
        assert_eq!(
            intervals
                .iter()
                .filter(|s| s.interval_type == IntervalType::Min10)
                .count(),
            3
        );
        assert_eq!(
            intervals
                .iter()
                .filter(|s| s.interval_type == IntervalType::Min30)
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
            .filter(|s| s.interval_type == IntervalType::Km1)
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
                .filter(|s| s.interval_type == IntervalType::Km1)
                .count(),
            13
        );
        assert_eq!(
            intervals
                .iter()
                .filter(|s| s.interval_type == IntervalType::Km5)
                .count(),
            3
        );
        assert_eq!(
            intervals
                .iter()
                .filter(|s| s.interval_type == IntervalType::Km10)
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
            .filter(|s| s.interval_type == IntervalType::Min1)
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
            .filter(|s| s.interval_type == IntervalType::Min1)
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
            .filter(|s| s.interval_type == IntervalType::Min1)
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
            .filter(|s| s.interval_type == IntervalType::Min1)
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

    // --- compute_training_speed_kmh tests ---

    fn make_analyze_point_with_speed(
        seconds: f64,
        distance_km: f64,
        speed_kmh: f64,
    ) -> AnalyzePoint {
        AnalyzePoint {
            instant_speed_kmh: speed_kmh,
            ..make_analyze_point(seconds, distance_km)
        }
    }

    #[test]
    fn test_training_speed_no_stops_positive() {
        // All points moving at 30 km/h with no stops → training speed ≈ 30 km/h.
        let step_km = 30.0_f64 / 360.0;
        let points: Vec<AnalyzePoint> = (0..10)
            .map(|i| make_analyze_point_with_speed(i as f64 * 10.0, i as f64 * step_km, 30.0))
            .collect();
        let training = compute_training_speed_kmh(&points, 3.0, 10.0);
        // With no stops the buffer has nothing to remove, so training ≈ 30 km/h
        assert!(
            (training - 30.0).abs() < 1.0,
            "training {training:.2} should be close to 30 km/h"
        );
    }

    #[test]
    fn test_training_speed_stop_excluded_raises_speed() {
        // 30 points: first 10 moving at 30 km/h, next 10 stopped (speed=0), last 10 moving at 30 km/h.
        // Each step is 10s. The moving segments cover 10 × (30/360) km ≈ 0.833 km each.
        let step_km = 30.0_f64 / 360.0; // distance per 10-second step at 30 km/h
        let mut points: Vec<AnalyzePoint> = Vec::new();
        for i in 0..10usize {
            points.push(make_analyze_point_with_speed(
                i as f64 * 10.0,
                i as f64 * step_km,
                30.0,
            ));
        }
        let stop_start_dist = points.last().unwrap().distance_km;
        for i in 0..10usize {
            let t = 100.0 + i as f64 * 10.0;
            points.push(make_analyze_point_with_speed(t, stop_start_dist, 0.0));
        }
        let resume_dist = stop_start_dist;
        for i in 0..10usize {
            let t = 200.0 + i as f64 * 10.0;
            points.push(make_analyze_point_with_speed(
                t,
                resume_dist + i as f64 * step_km,
                30.0,
            ));
        }

        // Compute expected average speed from test geometry (stop time pulls it down).
        let total_dist_km = points.last().unwrap().distance_km;
        let total_time_h = points.last().unwrap().seconds_from_start / 3600.0;
        let expected_avg_speed = total_dist_km / total_time_h;
        let training = compute_training_speed_kmh(&points, 3.0, 10.0);

        // Training speed strips stop + buffer, so it must exceed the raw avg speed.
        assert!(
            training > expected_avg_speed,
            "training speed {training:.2} should exceed avg speed {expected_avg_speed:.2}"
        );
        assert!(training > 0.0, "training speed must be positive");
    }

    #[test]
    fn test_training_speed_only_stops_returns_zero() {
        // All points are stopped → no training segments → returns 0.0.
        let points: Vec<AnalyzePoint> = (0..5)
            .map(|i| make_analyze_point_with_speed(i as f64 * 10.0, 0.0, 0.0))
            .collect();
        let training = compute_training_speed_kmh(&points, 3.0, 10.0);
        assert_eq!(training, 0.0);
    }

    #[test]
    fn test_training_speed_buffer_boundary_at_last_stopped_point() {
        // Stop interval must close at the last stopped point, not the first moving point.
        // Track: moving(0) → stop(10) → moving(20) → moving(30), buffer=0
        //   Stop interval = [10, 10]; buffered = [10, 10]
        //   Segment (0,10):  curr=10 IN  → excluded
        //   Segment (10,20): prev=10 IN  → excluded
        //   Segment (20,30): both outside → included → 30 km/h
        let step_km = 30.0_f64 / 360.0;
        let points = vec![
            make_analyze_point_with_speed(0.0, 0.0, 30.0),
            make_analyze_point_with_speed(10.0, step_km, 0.0),
            make_analyze_point_with_speed(20.0, step_km, 30.0),
            make_analyze_point_with_speed(30.0, step_km * 2.0, 30.0),
        ];
        let training = compute_training_speed_kmh(&points, 3.0, 0.0);
        assert!(
            (training - 30.0).abs() < 0.1,
            "only the post-gap moving segment should contribute; expected 30 km/h, got {training:.2}"
        );
    }

    #[test]
    fn test_training_speed_buffer_excludes_transition_zone() {
        // A buffer larger than the track duration excludes every segment → 0.0.
        // Track: moving(0) → stop(10) → moving(20) → moving(30), buffer=25
        //   Buffered = [10-25, 10+25] = [-15, 35] covers the entire 30-second track.
        let step_km = 30.0_f64 / 360.0;
        let points = vec![
            make_analyze_point_with_speed(0.0, 0.0, 30.0),
            make_analyze_point_with_speed(10.0, step_km, 0.0),
            make_analyze_point_with_speed(20.0, step_km, 30.0),
            make_analyze_point_with_speed(30.0, step_km * 2.0, 30.0),
        ];
        let training = compute_training_speed_kmh(&points, 3.0, 25.0);
        assert_eq!(
            training, 0.0,
            "buffer of 25s covers the entire 30s track; expected 0.0, got {training:.2}"
        );
    }

    #[test]
    fn test_training_speed_zero_buffer_moving_only() {
        let step_km = 30.0_f64 / 360.0;
        let points: Vec<AnalyzePoint> = (0..6)
            .map(|i| make_analyze_point_with_speed(i as f64 * 10.0, i as f64 * step_km, 30.0))
            .collect();
        let training = compute_training_speed_kmh(&points, 3.0, 0.0);
        assert!(training > 0.0);
        assert!(
            (training - 30.0).abs() < 1.0,
            "expected ~30 km/h, got {training:.2}"
        );
    }

    // --- compute_elevation_gain_m tests ---

    #[test]
    fn test_elevation_gain_no_altitude_returns_none() {
        let points: Vec<AnalyzePoint> = (0..5)
            .map(|i| make_analyze_point(i as f64 * 10.0, i as f64 * 0.1))
            .collect();
        assert_eq!(compute_elevation_gain_m(&points), None);
    }

    #[test]
    fn test_elevation_gain_flat_returns_zero() {
        let mut points: Vec<AnalyzePoint> = (0..5)
            .map(|i| make_analyze_point(i as f64 * 10.0, i as f64 * 0.1))
            .collect();
        for p in &mut points {
            p.smoothed_alt = Some(100.0);
        }
        assert_eq!(compute_elevation_gain_m(&points), Some(0.0));
    }

    #[test]
    fn test_elevation_gain_monotone_climb() {
        let mut points: Vec<AnalyzePoint> = (0..5)
            .map(|i| make_analyze_point(i as f64 * 10.0, i as f64 * 0.1))
            .collect();
        for (i, p) in points.iter_mut().enumerate() {
            p.smoothed_alt = Some(100.0 + i as f64 * 10.0);
        }
        let gain = compute_elevation_gain_m(&points).unwrap();
        assert!((gain - 40.0).abs() < 1e-9, "expected 40 m, got {gain}");
    }

    #[test]
    fn test_elevation_gain_downhill_only_returns_zero() {
        let mut points: Vec<AnalyzePoint> = (0..5)
            .map(|i| make_analyze_point(i as f64 * 10.0, i as f64 * 0.1))
            .collect();
        for (i, p) in points.iter_mut().enumerate() {
            p.smoothed_alt = Some(500.0 - i as f64 * 20.0);
        }
        assert_eq!(compute_elevation_gain_m(&points), Some(0.0));
    }

    #[test]
    fn test_elevation_gain_isolated_altitude_returns_none() {
        let mut points: Vec<AnalyzePoint> = (0..4)
            .map(|i| make_analyze_point(i as f64 * 10.0, i as f64 * 0.1))
            .collect();
        points[2].smoothed_alt = Some(100.0);
        assert_eq!(compute_elevation_gain_m(&points), None);
    }

    #[test]
    fn test_elevation_gain_skips_gaps_in_altitude() {
        let mut points: Vec<AnalyzePoint> = (0..5)
            .map(|i| make_analyze_point(i as f64 * 10.0, i as f64 * 0.1))
            .collect();
        points[0].smoothed_alt = Some(100.0);
        points[1].smoothed_alt = None;
        points[2].smoothed_alt = Some(200.0);
        points[3].smoothed_alt = Some(300.0);
        points[4].smoothed_alt = None;
        // Pairs (0,1): skipped (None). (1,2): skipped (None). (2,3): +100. (3,4): skipped (None).
        let gain = compute_elevation_gain_m(&points).unwrap();
        assert!((gain - 100.0).abs() < 1e-9, "expected 100 m, got {gain}");
    }
}
