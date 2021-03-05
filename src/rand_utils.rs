//
// utility functions to implement
// RANDOMNESS
//

use rand::Rng;

/* Take a probability distribution and
 * change it to a cumulative distribution
 * where the last element is always 1
 */
pub fn make_cdf(pdf: Vec<f64>) -> Vec<f64> {
    let error_margin = f64::EPSILON;
    let sum: f64 = pdf.iter().sum();
    if (sum - 1.0).abs() > error_margin {
        panic!(
            "Probability distribution does not sum to 1! Instead sums to {}",
            sum as f32
        );
    }
    let mut cdf: Vec<f64> = Vec::with_capacity(pdf.len());
    cdf.push(pdf[0]);
    for idx in 1..pdf.len() {
        cdf.push(cdf[idx - 1] + pdf[idx]);
    }
    cdf
}

/* Given a probability distribution containing n elements
 * randomly roll a n-sided die weighted to the probabilities
 * given. Return the index of the side that comes up.
 */
pub fn roll_die(pdf: Vec<f64>) -> usize {
    let mut rng = rand::thread_rng();
    let x = rng.gen::<f64>();
    let cdf: Vec<f64> = make_cdf(pdf);
    let index = cdf.iter().position(|&r| x < r).unwrap();
    index
}
