/// Quartiles of a non-empty, NaN-free distribution. Held together so callers
/// cannot accidentally represent "p25 known, median unknown" — by construction
/// all three are computed from the same sorted samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quartiles {
    pub p25: f64,
    pub p50: f64,
    pub p75: f64,
}

/// Returns the `q`-th percentile (`q` in `[0.0, 1.0]`) of `values` using
/// linear interpolation between adjacent sorted samples (numpy default).
///
/// Returns `None` if the slice is empty, `q` is outside `[0, 1]`, or any
/// value is NaN.
pub fn percentile(values: &[f64], q: f64) -> Option<f64> {
    if values.is_empty() || !(0.0..=1.0).contains(&q) {
        return None;
    }
    if values.iter().any(|v| v.is_nan()) {
        return None;
    }
    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("NaN ruled out above"));
    Some(quantile_of_sorted(&sorted, q))
}

/// Returns P25, median, and P75 of `values`, sorting once.
///
/// Returns `None` under the same conditions as [`percentile`] (empty input or
/// any NaN).
pub fn quartiles(values: &[f64]) -> Option<Quartiles> {
    if values.is_empty() || values.iter().any(|v| v.is_nan()) {
        return None;
    }
    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("NaN ruled out above"));
    Some(Quartiles {
        p25: quantile_of_sorted(&sorted, 0.25),
        p50: quantile_of_sorted(&sorted, 0.50),
        p75: quantile_of_sorted(&sorted, 0.75),
    })
}

/// `sorted` must be non-empty and NaN-free; `q` must be in `[0.0, 1.0]`.
fn quantile_of_sorted(sorted: &[f64], q: f64) -> f64 {
    let n = sorted.len();
    if n == 1 {
        return sorted[0];
    }
    let rank = q * (n - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let frac = rank - lo as f64;
        sorted[lo] + frac * (sorted[hi] - sorted[lo])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "expected {b}, got {a}");
    }

    #[test]
    fn empty_slice_returns_none() {
        assert_eq!(percentile(&[], 0.5), None);
    }

    #[test]
    fn q_below_zero_returns_none() {
        assert_eq!(percentile(&[1.0, 2.0, 3.0], -0.1), None);
    }

    #[test]
    fn q_above_one_returns_none() {
        assert_eq!(percentile(&[1.0, 2.0, 3.0], 1.1), None);
    }

    #[test]
    fn nan_value_returns_none() {
        assert_eq!(percentile(&[1.0, f64::NAN, 3.0], 0.5), None);
    }

    #[test]
    fn single_value_returns_that_value_for_any_q() {
        approx_eq(percentile(&[42.0], 0.0).unwrap(), 42.0);
        approx_eq(percentile(&[42.0], 0.5).unwrap(), 42.0);
        approx_eq(percentile(&[42.0], 1.0).unwrap(), 42.0);
    }

    #[test]
    fn boundaries_return_min_and_max() {
        let v = [1.0, 2.0, 3.0, 4.0];
        approx_eq(percentile(&v, 0.0).unwrap(), 1.0);
        approx_eq(percentile(&v, 1.0).unwrap(), 4.0);
    }

    #[test]
    fn quartiles_of_one_two_three_four() {
        let v = [1.0, 2.0, 3.0, 4.0];
        // rank = q*(n-1) = q*3 → linear interp on sorted samples
        approx_eq(percentile(&v, 0.25).unwrap(), 1.75);
        approx_eq(percentile(&v, 0.50).unwrap(), 2.5);
        approx_eq(percentile(&v, 0.75).unwrap(), 3.25);
    }

    #[test]
    fn unsorted_input_is_sorted_internally() {
        let v = [4.0, 1.0, 3.0, 2.0];
        approx_eq(percentile(&v, 0.25).unwrap(), 1.75);
        approx_eq(percentile(&v, 0.50).unwrap(), 2.5);
        approx_eq(percentile(&v, 0.75).unwrap(), 3.25);
    }

    #[test]
    fn two_values_median_is_midpoint() {
        approx_eq(percentile(&[10.0, 20.0], 0.5).unwrap(), 15.0);
    }

    #[test]
    fn percentiles_are_monotonic_in_q() {
        let v = [5.0, 1.0, 9.0, 3.0, 7.0, 11.0, 8.0, 2.0];
        let p25 = percentile(&v, 0.25).unwrap();
        let p50 = percentile(&v, 0.50).unwrap();
        let p75 = percentile(&v, 0.75).unwrap();
        assert!(p25 <= p50 && p50 <= p75, "p25={p25}, p50={p50}, p75={p75}");
    }

    #[test]
    fn quartiles_match_individual_percentiles() {
        let v = [4.0, 1.0, 3.0, 2.0];
        let q = quartiles(&v).unwrap();
        approx_eq(q.p25, percentile(&v, 0.25).unwrap());
        approx_eq(q.p50, percentile(&v, 0.50).unwrap());
        approx_eq(q.p75, percentile(&v, 0.75).unwrap());
    }

    #[test]
    fn quartiles_empty_returns_none() {
        assert_eq!(quartiles(&[]), None);
    }

    #[test]
    fn quartiles_nan_returns_none() {
        assert_eq!(quartiles(&[1.0, f64::NAN, 3.0]), None);
    }

    #[test]
    fn quartiles_single_value_collapses_to_that_value() {
        let q = quartiles(&[42.0]).unwrap();
        approx_eq(q.p25, 42.0);
        approx_eq(q.p50, 42.0);
        approx_eq(q.p75, 42.0);
    }
}
