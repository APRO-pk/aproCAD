#[derive(Debug, Clone)]
pub struct NACA4 {
    pub max_camber_pct: f64,
    pub max_camber_pos: f64,
    pub max_thickness_pct: f64,
}

impl NACA4 {
    pub fn from_digits(digits: &str) -> Option<Self> {
        let d: Vec<u32> = digits.chars().filter_map(|c| c.to_digit(10)).collect();
        if d.len() < 4 { return None; }
        Some(NACA4 {
            max_camber_pct: d[0] as f64 / 100.0,
            max_camber_pos: d[1] as f64 / 10.0,
            max_thickness_pct: (d[2] * 10 + d[3]) as f64 / 100.0,
        })
    }

    pub fn thickness(&self, t: f64) -> f64 {
        self.max_thickness_pct * (0.2969 * t.sqrt() - 0.1260 * t - 0.3516 * t.powi(2)
            + 0.2843 * t.powi(3) - 0.1015 * t.powi(4))
    }

    pub fn camber(&self, t: f64) -> f64 {
        if self.max_camber_pct.abs() < 1e-10 || self.max_camber_pos.abs() < 1e-10 {
            return 0.0;
        }
        let m = self.max_camber_pct;
        let p = self.max_camber_pos;
        if t < p {
            m / (p * p) * (2.0 * p * t - t * t)
        } else {
            m / ((1.0 - p) * (1.0 - p)) * ((1.0 - 2.0 * p) + 2.0 * p * t - t * t)
        }
    }

    pub fn camber_slope(&self, t: f64) -> f64 {
        if self.max_camber_pct.abs() < 1e-10 || self.max_camber_pos.abs() < 1e-10 {
            return 0.0;
        }
        let m = self.max_camber_pct;
        let p = self.max_camber_pos;
        if t < p {
            2.0 * m / (p * p) * (p - t)
        } else {
            2.0 * m / ((1.0 - p) * (1.0 - p)) * (p - t)
        }
    }

    pub fn upper_lower(&self, x: f64, chord: f64) -> (f64, f64) {
        let t = (x / chord).clamp(0.0, 1.0);
        let yt = self.thickness(t) * chord;
        let yc = self.camber(t) * chord;
        let theta = self.camber_slope(t).atan();
        let _xu = x - yt * theta.sin();
        let yu = yc + yt * theta.cos();
        let _xl = x + yt * theta.sin();
        let yl = yc - yt * theta.cos();
        (yu, yl)
    }

    pub fn profile(&self, chord: f64, n: usize) -> Vec<[f64; 2]> {
        let n_half = n / 2;
        let mut pts = Vec::with_capacity(n);
        for i in 0..n_half {
            let t = (n_half - i) as f64 / n_half as f64;
            let x = t * chord;
            let (yu, _) = self.upper_lower(x, chord);
            pts.push([x, yu]);
        }
        for i in 1..=n_half {
            let t = i as f64 / n_half as f64;
            let x = t * chord;
            let (_, yl) = self.upper_lower(x, chord);
            pts.push([x, yl]);
        }
        pts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_naca_0012_symmetric() {
        let naca = NACA4::from_digits("0012").unwrap();
        assert!((naca.max_camber_pct).abs() < 1e-10);
        assert!((naca.max_thickness_pct - 0.12).abs() < 1e-10);
        let (yu, yl) = naca.upper_lower(0.25, 1.0);
        assert!(yu > 0.0);
        assert!(yl < 0.0);
        assert!((yu + yl).abs() < 1e-10, "yu={}, yl={}", yu, yl);
    }

    #[test]
    fn test_naca_2412_cambered() {
        let naca = NACA4::from_digits("2412").unwrap();
        assert!((naca.max_camber_pct - 0.02).abs() < 1e-10);
        assert!((naca.max_camber_pos - 0.4).abs() < 1e-10);
    }

    #[test]
    fn test_profile_has_correct_count() {
        let naca = NACA4::from_digits("0012").unwrap();
        let pts = naca.profile(100.0, 20);
        assert_eq!(pts.len(), 20);
    }

    #[test]
    fn test_profile_closes() {
        let naca = NACA4::from_digits("0012").unwrap();
        let pts = naca.profile(100.0, 20);
        let first = pts.first().unwrap();
        let last = pts.last().unwrap();
        assert!((first[0] - 100.0).abs() < 1e-10);
        assert!((last[0] - 100.0).abs() < 1e-10);
        assert!((first[1]).abs() < 1.0, "TE upper y={}", first[1]);
        assert!((last[1]).abs() < 1.0, "TE lower y={}", last[1]);
    }

    #[test]
    fn test_trailing_edge_closure() {
        let naca = NACA4::from_digits("4412").unwrap();
        let pts = naca.profile(100.0, 16);
        assert!((pts[0][0] - 100.0).abs() < 1e-10);
        assert!((pts[pts.len() - 1][0] - 100.0).abs() < 1e-10);
    }
}
