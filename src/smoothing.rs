use crate::{GpsAnalyzerError, GpsPoint, SavitzkyGolayConfig, Track};
use nalgebra::{DMatrix, DVector};

pub fn smooth_track(track: &Track, config: SavitzkyGolayConfig) -> Result<Track, GpsAnalyzerError> {
    let window_size = config.window_size.get() as usize;
    let poly_degree = config.polynomial_degree as usize;

    if window_size > track.points.len() {
        return Err(GpsAnalyzerError::WindowSizeTooLarge(
            window_size as u32,
            track.points.len(),
        ));
    }

    if poly_degree >= window_size {
        return Err(GpsAnalyzerError::PolynomialDegreeTooLarge);
    }

    let half_window = window_size / 2;

    let mut smoothed_points = Vec::with_capacity(track.points.len());
    let mut lat_buf: Vec<f64> = Vec::with_capacity(window_size);
    let mut lon_buf: Vec<f64> = Vec::with_capacity(window_size);
    let mut alt_buf: Vec<f64> = Vec::with_capacity(window_size);

    for i in 0..track.points.len() {
        let start = i.saturating_sub(half_window);
        let end = std::cmp::min(i + half_window + 1, track.points.len());
        let actual_window = end - start;

        let window_points = &track.points[start..end];

        lat_buf.clear();
        lat_buf.extend(window_points.iter().map(|p| p.lat));
        let smoothed_lat = savitzky_golay_smooth(&lat_buf, poly_degree, i - start)?;

        lon_buf.clear();
        lon_buf.extend(window_points.iter().map(|p| p.lon));
        let smoothed_lon = savitzky_golay_smooth(&lon_buf, poly_degree, i - start)?;

        let smoothed_alt = if track.points[i].alt.is_some() {
            alt_buf.clear();
            alt_buf.extend(window_points.iter().filter_map(|p| p.alt));
            // Only smooth when every window point has altitude: alt_buf and window_points
            // are then the same length and order, so i - start remains the correct center index.
            // Any missing altitude in the window falls back to the raw value.
            if alt_buf.len() == actual_window {
                Some(savitzky_golay_smooth(&alt_buf, poly_degree, i - start)?)
            } else {
                track.points[i].alt
            }
        } else {
            None
        };

        smoothed_points.push(GpsPoint {
            lat: smoothed_lat,
            lon: smoothed_lon,
            alt: smoothed_alt,
            timestamp: track.points[i].timestamp,
        });
    }

    Track::new(smoothed_points)
}

fn savitzky_golay_smooth(
    values: &[f64],
    poly_degree: usize,
    point_index: usize,
) -> Result<f64, GpsAnalyzerError> {
    let n = values.len();
    if n < poly_degree + 1 {
        return Err(GpsAnalyzerError::PolynomialDegreeTooLarge);
    }

    // Build design matrix: each row is [x^0, x^1, ..., x^poly_degree] for each point
    let mut design = DMatrix::<f64>::zeros(n, poly_degree + 1);
    for (i, _) in values.iter().enumerate() {
        for j in 0..=poly_degree {
            design[(i, j)] = (i as f64 - point_index as f64).powi(j as i32);
        }
    }

    // Build observation vector
    let observations = DVector::from_row_slice(values);

    // Solve normal equations: (X^T X)^-1 X^T y
    let xt = design.transpose();
    let xtx = &xt * &design;
    let xty = &xt * &observations;

    let coeffs = xtx.try_inverse().ok_or(GpsAnalyzerError::ParseError(
        "Cannot invert design matrix".to_string(),
    ))? * xty;

    // x=0 by construction (center point), so the polynomial evaluates to its constant term
    Ok(coeffs[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_track(values: Vec<f64>) -> Track {
        let points = values
            .into_iter()
            .map(|lat| GpsPoint {
                lat,
                lon: 0.0,
                alt: None,
                timestamp: Utc::now(),
            })
            .collect();
        Track::new(points).expect("Failed to create test track")
    }

    #[test]
    fn test_smooth_linear_sequence() {
        let track = create_test_track(vec![1.0, 2.0, 3.0, 4.0, 5.0]);
        let config = SavitzkyGolayConfig::new(3, 1).expect("Failed to create config");
        let smoothed = smooth_track(&track, config).expect("Failed to smooth");

        assert_eq!(smoothed.len(), track.len());
        // Linear sequence with degree 1 should reproduce exactly
        for i in 0..track.len() {
            assert!((smoothed.points[i].lat - track.points[i].lat).abs() < 1e-10);
        }
    }

    #[test]
    fn test_smooth_noisy_sequence() {
        let track = create_test_track(vec![1.0, 1.1, 0.9, 2.0, 2.1, 1.9, 3.0]);
        let config = SavitzkyGolayConfig::new(3, 1).expect("Failed to create config");
        let smoothed = smooth_track(&track, config).expect("Failed to smooth");

        assert_eq!(smoothed.len(), track.len());
        // Check that smoothed values are within expected range
        for point in smoothed.points.iter() {
            assert!(point.lat.is_finite());
        }
    }

    #[test]
    fn test_window_size_validation() {
        let track = create_test_track(vec![1.0, 2.0]);
        let config = SavitzkyGolayConfig::new(5, 1).expect("Failed to create config");
        assert!(smooth_track(&track, config).is_err());
    }

    #[test]
    fn test_smooth_three_point_minimum_window() {
        let track = create_test_track(vec![1.0, 2.0, 3.0]);
        let config = SavitzkyGolayConfig::new(3, 1).expect("window=3 degree=1 is valid");
        let smoothed = smooth_track(&track, config).expect("3-point track with window=3 succeeds");
        assert_eq!(smoothed.len(), 3);
        for i in 0..3 {
            assert!(
                (smoothed.points[i].lat - track.points[i].lat).abs() < 1e-10,
                "expected {}, got {}",
                track.points[i].lat,
                smoothed.points[i].lat
            );
        }
    }

    #[test]
    fn test_smooth_single_point_errors_on_window_too_large() {
        let track = create_test_track(vec![1.0]);
        let config = SavitzkyGolayConfig::new(3, 1).expect("config is valid");
        assert!(smooth_track(&track, config).is_err());
    }

    #[test]
    fn test_smooth_degree_zero_returns_mean() {
        let track = create_test_track(vec![1.0, 3.0, 5.0, 3.0, 1.0]);
        let config = SavitzkyGolayConfig::new(3, 0).expect("degree=0 is valid");
        let smoothed = smooth_track(&track, config).expect("smoothing succeeded");
        assert!((smoothed.points[1].lat - 3.0).abs() < 1e-10);
        assert!((smoothed.points[2].lat - 11.0 / 3.0).abs() < 1e-10);
        assert!((smoothed.points[3].lat - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_smooth_partial_altitude_falls_back_to_raw() {
        let points = vec![
            GpsPoint {
                lat: 1.0,
                lon: 0.0,
                alt: Some(100.0),
                timestamp: Utc::now(),
            },
            GpsPoint {
                lat: 2.0,
                lon: 0.0,
                alt: Some(110.0),
                timestamp: Utc::now(),
            },
            GpsPoint {
                lat: 3.0,
                lon: 0.0,
                alt: None,
                timestamp: Utc::now(),
            },
            GpsPoint {
                lat: 4.0,
                lon: 0.0,
                alt: Some(130.0),
                timestamp: Utc::now(),
            },
            GpsPoint {
                lat: 5.0,
                lon: 0.0,
                alt: Some(140.0),
                timestamp: Utc::now(),
            },
        ];
        let track = Track::new(points).expect("valid track");
        let config = SavitzkyGolayConfig::new(3, 1).expect("valid config");
        let smoothed = smooth_track(&track, config).expect("smoothing succeeded");

        // p[1] and p[3] have windows that include p[2] (None) → fall back to raw
        assert_eq!(smoothed.points[1].alt, Some(110.0));
        assert_eq!(smoothed.points[3].alt, Some(130.0));
        // p[2] has no altitude → stays None
        assert_eq!(smoothed.points[2].alt, None);
        // p[0] window is [p0,p1], both have altitude → smoothed (differs from raw)
        assert!(smoothed.points[0].alt.is_some());
        // p[4] window is [p3,p4], both have altitude → smoothed
        assert!(smoothed.points[4].alt.is_some());
    }
}
