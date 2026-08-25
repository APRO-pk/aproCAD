use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NoseProfile {
    Conical,
    Ogive,
    VonKarman,
    Haack { c: f64 },
    Power { n: f64 },
    Parabolic { k: f64 },
}

impl NoseProfile {
    pub fn radius(&self, x: f64, length: f64, base_radius: f64) -> f64 {
        let t = (x / length).clamp(0.0, 1.0);
        match *self {
            NoseProfile::Conical => base_radius * t,
            NoseProfile::Power { n } => base_radius * t.powf(n),
            NoseProfile::Ogive => {
                let rho = (base_radius * base_radius + length * length) / (2.0 * base_radius);
                (rho * rho - (length - x).powi(2)).sqrt() - (rho - base_radius)
            }
            NoseProfile::Parabolic { k } => {
                base_radius * (2.0 * t - k * t * t) / (2.0 - k)
            }
            NoseProfile::VonKarman => haack(x, length, base_radius, 0.0),
            NoseProfile::Haack { c } => haack(x, length, base_radius, c),
        }
    }

    pub fn sample(&self, length: f64, base_radius: f64, n: usize) -> Vec<[f64; 2]> {
        self.sample_with_tip(length, base_radius, n, 0.0)
    }

    /// Sample profile points with a minimum tip radius.
    /// Instead of reaching r=0 at the tip, the profile stops at `tip_radius`
    /// and duplicates the last point at a slightly inset x to create a flat
    /// tip cap. This avoids the apex/pole singularity in revolve tessellation.
    pub fn sample_with_tip(&self, length: f64, base_radius: f64, n: usize, tip_radius: f64) -> Vec<[f64; 2]> {
        let n = n.max(2);
        let mut pts = Vec::with_capacity(n + 1);
        // Find the x where radius crosses tip_radius (or approximate)
        let tip_r = tip_radius.max(0.0);
        for i in 0..n {
            let x = length * i as f64 / (n - 1) as f64;
            pts.push([x, self.radius(x, length, base_radius).max(tip_r)]);
        }
        if tip_r > 0.0 && pts.last().map_or(false, |p| p[1] > tip_r) {
            // Add a final point at x=slightly beyond length to close the tip flat
            pts.push([length * 1.001, tip_r]);
        }
        pts
    }
}

fn haack(x: f64, length: f64, base_radius: f64, c: f64) -> f64 {
    let theta = (1.0 - 2.0 * x / length).clamp(-1.0, 1.0).acos();
    base_radius * ((theta - (2.0 * theta).sin() / 2.0 + c * theta.sin().powi(3)) / PI).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conical_tip_zero() {
        let p = NoseProfile::Conical;
        assert!((p.radius(0.0, 100.0, 50.0)).abs() < 1e-10);
    }

    #[test]
    fn test_conical_base() {
        let p = NoseProfile::Conical;
        assert!((p.radius(100.0, 100.0, 50.0) - 50.0).abs() < 1e-10);
    }

    #[test]
    fn test_von_karman_tip() {
        let p = NoseProfile::VonKarman;
        assert!((p.radius(0.0, 100.0, 50.0)).abs() < 1e-10);
    }

    #[test]
    fn test_von_karman_base() {
        let p = NoseProfile::VonKarman;
        assert!((p.radius(100.0, 100.0, 50.0) - 50.0).abs() < 1e-6);
    }

    #[test]
    fn test_sample_count() {
        let p = NoseProfile::VonKarman;
        assert_eq!(p.sample(100.0, 50.0, 20).len(), 20);
    }

    #[test]
    fn test_haack_c_one_third() {
        let p = NoseProfile::Haack { c: 1.0 / 3.0 };
        assert!((p.radius(0.0, 100.0, 50.0)).abs() < 1e-10);
        assert!((p.radius(100.0, 100.0, 50.0) - 50.0).abs() < 1e-6);
    }

    #[test]
    fn test_power_nose() {
        let p = NoseProfile::Power { n: 0.5 };
        assert!((p.radius(0.0, 100.0, 50.0)).abs() < 1e-10);
        assert!((p.radius(100.0, 100.0, 50.0) - 50.0).abs() < 1e-10);
    }

    #[test]
    fn test_parabolic() {
        let p = NoseProfile::Parabolic { k: 0.5 };
        assert!((p.radius(0.0, 100.0, 50.0)).abs() < 1e-10);
        assert!((p.radius(100.0, 100.0, 50.0) - 50.0).abs() < 1e-10);
    }

    #[test]
    fn test_ogive_tip() {
        let p = NoseProfile::Ogive;
        assert!((p.radius(0.0, 100.0, 50.0)).abs() < 1e-10);
    }

    #[test]
    fn test_all_profiles_monotonic() {
        let profiles = vec![
            NoseProfile::Conical,
            NoseProfile::Ogive,
            NoseProfile::VonKarman,
            NoseProfile::Haack { c: 1.0 / 3.0 },
            NoseProfile::Power { n: 0.5 },
            NoseProfile::Parabolic { k: 0.5 },
        ];
        for p in profiles {
            let samples = p.sample(100.0, 50.0, 50);
            for w in samples.windows(2) {
                assert!(w[1][1] >= w[0][1], "profile {:?} not monotonic at x={}", p, w[1][0]);
            }
        }
    }
}
