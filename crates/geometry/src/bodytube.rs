pub struct BodyTube;

impl BodyTube {
    pub fn sample(length: f64, radius: f64, n: usize) -> Vec<[f64; 2]> {
        let n = n.max(2);
        let mut pts = Vec::with_capacity(n);
        for i in 0..n {
            let x = length * i as f64 / (n - 1) as f64;
            pts.push([x, radius]);
        }
        pts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bodytube_sample() {
        let pts = BodyTube::sample(500.0, 50.0, 2);
        assert_eq!(pts.len(), 2);
        assert!((pts[0][0]).abs() < 1e-10);
        assert!((pts[0][1] - 50.0).abs() < 1e-10);
        assert!((pts[1][0] - 500.0).abs() < 1e-10);
        assert!((pts[1][1] - 50.0).abs() < 1e-10);
    }

    #[test]
    fn test_bodytube_constant_radius() {
        let pts = BodyTube::sample(500.0, 50.0, 10);
        for p in &pts {
            assert!((p[1] - 50.0).abs() < 1e-10);
        }
    }
}
