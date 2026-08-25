#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DomeKind {
    Hemispherical,
    Ellipsoidal { ratio: f64 },
}

pub struct Tank;

impl Tank {
    pub fn sample(radius: f64, cylindrical_length: f64, dome: &DomeKind, n: usize) -> Vec<[f64; 2]> {
        let n_dome = (n / 2).max(2);
        let n_cyl = (n - n_dome).max(2);

        let dome_height = match dome {
            DomeKind::Hemispherical => radius,
            DomeKind::Ellipsoidal { ratio } => radius / ratio,
        };

        let mut pts = Vec::with_capacity(n);

        for i in 0..n_dome {
            let t = if n_dome > 1 { i as f64 / (n_dome - 1) as f64 } else { 0.0 };
            let x = t * dome_height;
            let h = dome_height - x;
            let r = match dome {
                DomeKind::Hemispherical => {
                    (radius * radius - h * h).sqrt()
                }
                DomeKind::Ellipsoidal { .. } => {
                    radius * (1.0 - (h / dome_height).powi(2)).sqrt()
                }
            };
            pts.push([x, r]);
        }

        for i in 0..n_cyl {
            let t = if n_cyl > 1 { i as f64 / (n_cyl - 1) as f64 } else { 0.0 };
            let x = dome_height + t * cylindrical_length;
            pts.push([x, radius]);
        }

        pts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hemispherical_tip() {
        let pts = Tank::sample(50.0, 200.0, &DomeKind::Hemispherical, 20);
        assert!((pts[0][0]).abs() < 1e-10);
        assert!((pts[0][1]).abs() < 1e-10);
    }

    #[test]
    fn test_hemispherical_cylinder_base() {
        let pts = Tank::sample(50.0, 200.0, &DomeKind::Hemispherical, 20);
        let last = pts.last().unwrap();
        assert!((last[1] - 50.0).abs() < 1e-10);
        assert!((last[0] - (50.0 + 200.0)).abs() < 1e-10);
    }

    #[test]
    fn test_ellipsoidal_radius_at_junction() {
        let pts = Tank::sample(50.0, 200.0, &DomeKind::Ellipsoidal { ratio: 2.0 }, 20);
        let dome_height = 50.0 / 2.0;
        let cyl_start = pts.iter().position(|p| (p[0] - dome_height).abs() < 0.01).unwrap();
        assert!((pts[cyl_start][1] - 50.0).abs() < 1.0);
    }

    #[test]
    fn test_monotonic() {
        let kinds = vec![
            DomeKind::Hemispherical,
            DomeKind::Ellipsoidal { ratio: 1.5 },
            DomeKind::Ellipsoidal { ratio: 3.0 },
        ];
        for kind in kinds {
            let pts = Tank::sample(50.0, 200.0, &kind, 40);
            for w in pts.windows(2) {
                assert!(w[1][1] >= w[0][1] - 1e-10, "not monotonic: {:?}", kind);
            }
        }
    }

    #[test]
    fn test_ellipsoidal_fin_pan() {
        let pts = Tank::sample(60.0, 300.0, &DomeKind::Ellipsoidal { ratio: 2.0 }, 10);
        assert!(pts.len() >= 4);
    }
}
