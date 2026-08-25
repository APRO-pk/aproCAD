#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NozzleKind {
    Conical,
    Bell,
    Moc,
}

pub fn nozzle_contour(
    kind: &NozzleKind,
    throat_radius: f64,
    expansion_ratio: f64,
    percent_bell: f64,
    chamber_radius: f64,
    n: usize,
) -> Vec<[f64; 2]> {
    let exit_radius = throat_radius * expansion_ratio.sqrt();

    let conv_angle = 30.0_f64.to_radians();
    let chamber_len = 2.0 * chamber_radius;
    let conv_len = (chamber_radius - throat_radius) / conv_angle.tan();

    let n_chamber = (n / 4).max(2);
    let n_conv = (n / 4).max(2);
    let n_div = n.saturating_sub(n_chamber + n_conv).max(2);

    let mut pts = Vec::with_capacity(n);

    for i in 0..n_chamber {
        let t = i as f64 / (n_chamber - 1) as f64;
        pts.push([t * chamber_len, chamber_radius]);
    }

    let throat_x = chamber_len + conv_len;
    for i in 0..n_conv {
        let t = i as f64 / (n_conv - 1) as f64;
        let x = chamber_len + t * conv_len;
        let r = chamber_radius - (chamber_radius - throat_radius) * t;
        pts.push([x, r]);
    }

    let div_angle = 15.0_f64.to_radians();
    let div_len_full = (exit_radius - throat_radius) / div_angle.tan();

    match kind {
        NozzleKind::Conical => {
            for i in 0..n_div {
                let t = (i + 1) as f64 / n_div as f64;
                let x = throat_x + t * div_len_full;
                let r = throat_radius + (exit_radius - throat_radius) * t;
                pts.push([x, r]);
            }
        }
        NozzleKind::Bell | NozzleKind::Moc => {
            let theta_n = 30.0_f64.to_radians();
            let theta_e = 8.0_f64.to_radians();
            let bell_len = (percent_bell / 100.0) * div_len_full;

            let t_control = theta_n.tan() / (theta_n.tan() + (-theta_e).tan());
            let control_r = 2.0 * throat_radius;
            let control_pt = [throat_x + t_control * bell_len, control_r];

            for i in 0..n_div {
                let t = (i + 1) as f64 / n_div as f64;
                let x = throat_x + t * bell_len;
                let r = (1.0 - t).powi(2) * throat_radius
                    + 2.0 * (1.0 - t) * t * control_pt[1]
                    + t.powi(2) * exit_radius;
                pts.push([x, r]);
            }
        }
    }

    pts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conical_contour() {
        let pts = nozzle_contour(&NozzleKind::Conical, 25.0, 9.0, 100.0, 75.0, 20);
        assert!(pts.len() >= 6);
        assert!((pts[0][1] - 75.0).abs() < 1e-10, "chamber radius");
        let last = pts.last().unwrap();
        let expected_exit = 25.0 * (9.0_f64).sqrt();
        assert!((last[1] - expected_exit).abs() < 1.0, "exit radius");
    }

    #[test]
    fn test_bell_contour() {
        let pts = nozzle_contour(&NozzleKind::Bell, 25.0, 9.0, 80.0, 75.0, 20);
        assert!(pts.len() >= 6);
        let last = pts.last().unwrap();
        let expected_exit = 25.0 * (9.0_f64).sqrt();
        assert!((last[1] - expected_exit).abs() < 1.0, "exit radius");
    }

    #[test]
    fn test_conical_monotonic_divergent() {
        let pts = nozzle_contour(&NozzleKind::Conical, 25.0, 9.0, 100.0, 75.0, 30);
        let throat_idx = pts.iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a[1].partial_cmp(&b[1]).unwrap())
            .map(|(i, _)| i)
            .unwrap();
        for w in pts[throat_idx..].windows(2) {
            assert!(w[1][1] >= w[0][1] - 1e-10,
                "divergent not monotonic at x={}: {} -> {}", w[1][0], w[0][1], w[1][1]);
        }
    }

    #[test]
    fn test_chamber_radius() {
        let pts = nozzle_contour(&NozzleKind::Bell, 20.0, 16.0, 100.0, 60.0, 10);
        assert!((pts[0][1] - 60.0).abs() < 1e-10);
    }

    #[test]
    fn test_throat_is_minimum() {
        let pts = nozzle_contour(&NozzleKind::Conical, 20.0, 16.0, 100.0, 60.0, 40);
        let _throat_idx = pts.len() / 2;
        let min_r = pts.iter().map(|p| p[1]).fold(f64::INFINITY, |a, b| a.min(b));
        assert!((min_r - 20.0).abs() < 2.0);
    }
}
