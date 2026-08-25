pub struct Transition;

impl Transition {
    pub fn sample(length: f64, start_radius: f64, end_radius: f64, n: usize) -> Vec<[f64; 2]> {
        let n = n.max(2);
        let mut pts = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f64 / (n - 1) as f64;
            let x = length * t;
            let r = start_radius + (end_radius - start_radius) * t;
            pts.push([x, r]);
        }
        pts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transition_sample() {
        let pts = Transition::sample(200.0, 50.0, 70.0, 3);
        assert_eq!(pts.len(), 3);
        assert!((pts[0][1] - 50.0).abs() < 1e-10);
        assert!((pts[1][1] - 60.0).abs() < 1e-10);
        assert!((pts[2][1] - 70.0).abs() < 1e-10);
    }

    #[test]
    fn test_transition_monotonic() {
        let pts = Transition::sample(200.0, 30.0, 80.0, 20);
        for w in pts.windows(2) {
            assert!(w[1][1] >= w[0][1]);
        }
    }
}
