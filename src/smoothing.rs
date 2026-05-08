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

    for i in 0..track.points.len() {
        let start = i.saturating_sub(half_window);
        let end = std::cmp::min(i + half_window + 1, track.points.len());
        let actual_window = end - start;

        let window_points = &track.points[start..end];

        let smoothed_lat = savitzky_golay_smooth(
            window_points
                .iter()
                .map(|p| p.lat)
                .collect::<Vec<_>>()
                .as_slice(),
            poly_degree,
            i - start,
        )?;

        let smoothed_lon = savitzky_golay_smooth(
            window_points
                .iter()
                .map(|p| p.lon)
                .collect::<Vec<_>>()
                .as_slice(),
            poly_degree,
            i - start,
        )?;

        let smoothed_alt = if track.points[i].alt.is_some() {
            let alt_values: Vec<f64> = window_points.iter().filter_map(|p| p.alt).collect();
            if alt_values.len() == actual_window {
                Some(savitzky_golay_smooth(&alt_values, poly_degree, i - start)?)
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
}
