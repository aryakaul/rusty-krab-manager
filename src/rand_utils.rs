//! Weighted random sampling. `make_cdf` turns a probability distribution into a
//! cumulative one; `roll_die` samples an index from it. Both tolerate the minor
//! float drift the pdf builders in `selection` produce, and never panic.

use rand::RngExt;

// Loose tolerance for "sums to 1.0" checks (here and in config validation):
// float accumulation in the pdf builders (normalization, weight
// redistribution) drifts by several ULPs, far more than f64::EPSILON.
pub const PROB_SUM_TOLERANCE: f64 = 1e-9;

// Take a probability distribution and
// change it to a cumulative distribution
// where the last element is always 1
pub fn make_cdf(pdf: &[f64]) -> Vec<f64> {
    let sum: f64 = pdf.iter().sum();
    assert!(
        (sum - 1.0).abs() <= PROB_SUM_TOLERANCE,
        "Probability distribution does not sum to 1! Instead sums to {}",
        sum as f32
    );
    pdf.iter()
        .scan(0.0, |acc, &p| {
            *acc += p;
            Some(*acc)
        })
        .collect()
}

// Given a probability distribution containing n elements
// randomly roll a n-sided die weighted to the probabilities
// given. Return the index of the side that comes up.
pub fn roll_die(pdf: &[f64]) -> usize {
    let mut rng = rand::rng();
    let x = rng.random::<f64>();
    let cdf = make_cdf(pdf);
    // The final CDF entry can land just under 1.0 from float accumulation, so a
    // high roll may match nothing; fall back to the last index rather than panic.
    cdf.iter().position(|&r| x < r).unwrap_or(cdf.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_cdf_tolerates_minor_float_drift() {
        // Sums to 1 - 1e-12: would fail the old f64::EPSILON assert, passes now.
        let cdf = make_cdf(&[0.5, 0.5 - 1e-12]);
        assert_eq!(cdf.len(), 2);
    }

    #[test]
    fn roll_die_always_returns_a_valid_index() {
        for _ in 0..1000 {
            assert!(roll_die(&[0.25, 0.25, 0.25, 0.25]) < 4);
        }
    }

    #[test]
    fn roll_die_handles_single_outcome() {
        assert_eq!(roll_die(&[1.0]), 0);
    }
}
