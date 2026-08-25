use apro_geometry::airfoil::NACA4;
use apro_document::vehicle::FinSetParams;
use apro_kernel::{fin_mesh, pattern_mesh, MeshData};

fn get_naca(params: &FinSetParams) -> NACA4 {
    let digits = match &params.airfoil.family {
        apro_document::vehicle::AirfoilFamily::NACA { digits } => digits.as_str(),
    };
    NACA4::from_digits(digits).unwrap_or_else(|| NACA4::from_digits("0012").unwrap())
}

pub fn mesh_finset(params: &FinSetParams) -> MeshData {
    let naca = get_naca(params);

    let n_pts = 24;
    let root_profile = naca.profile(params.root_chord, n_pts);

    let tip_ratio = params.tip_chord / params.root_chord;
    let tip_profile: Vec<[f64; 2]> = root_profile
        .iter()
        .map(|&[x, y]| [x * tip_ratio, y * tip_ratio])
        .collect();

    let single = fin_mesh(&root_profile, &tip_profile, params.span, params.sweep);
    pattern_mesh(&single, params.count)
}

#[cfg(test)]
mod tests {
    use apro_document::vehicle::{FinSetParams, AirfoilParams, AirfoilFamily};
    use super::mesh_finset;

    #[test]
    fn test_finset_mesh() {
        let params = FinSetParams {
            count: 4,
            root_chord: 150.0,
            tip_chord: 80.0,
            span: 60.0,
            sweep: 20.0,
            airfoil: AirfoilParams { family: AirfoilFamily::NACA { digits: "0012".into() } },
            thickness: 3.0,
            material: "Al-6061-T6".into(),
        };
        let mesh = mesh_finset(&params);
        assert!(mesh.positions.len() >= 9);
        assert!(mesh.indices.len() >= 3);
        // 4 fins should have 4x the vertices of a single fin
        let n_pts = 24;
        let n_root_tris = n_pts - 2;
        let n_side_tris = (n_pts - 1) * 2;
        let n_tip_tris = n_pts - 2;
        let n_tris_per_fin = n_side_tris + n_root_tris + n_tip_tris;
        assert_eq!(mesh.indices.len() / 3, n_tris_per_fin * 4);
    }
}
