//! L2 normalize + cosine helpers.

/// In-place L2 normalize. Zero vector left unchanged (all zeros).
pub fn l2_normalize(v: &mut [f32]) {
    let mut sum = 0.0f64;
    for x in v.iter() {
        let f = f64::from(*x);
        sum += f * f;
    }
    if sum <= f64::EPSILON {
        return;
    }
    let inv = 1.0 / sum.sqrt();
    for x in v.iter_mut() {
        *x = (*x as f64 * inv) as f32;
    }
}

/// Return an L2-normalized copy.
pub fn l2_normalize_owned(mut v: Vec<f32>) -> Vec<f32> {
    l2_normalize(&mut v);
    v
}

/// Cosine similarity of two **already L2-normalized** vectors (dot product).
///
/// Returns 0.0 if lengths differ or either is empty.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let mut acc = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        acc += f64::from(*x) * f64::from(*y);
    }
    acc as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l2_unit_and_cosine() {
        let mut a = vec![3.0f32, 4.0];
        l2_normalize(&mut a);
        assert!((a[0] - 0.6).abs() < 1e-5);
        assert!((a[1] - 0.8).abs() < 1e-5);
        let b = a.clone();
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-5);
    }
}
