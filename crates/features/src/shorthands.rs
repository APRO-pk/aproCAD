use apro_document::vehicle::{
    SolidOp, Profile, Axis,
    NoseConeParams, NoseConeProfile as DocProfile,
    BodyTubeParams, TransitionParams, TankParams, DomeKind as DocDome,
    NozzleParams, NozzleKind as DocKind,
    FinSetParams,
};
use apro_geometry::NoseProfile;
use apro_geometry::bodytube::BodyTube;
use apro_geometry::transition::Transition;
use apro_geometry::tank::{Tank, DomeKind as GeomDome};
use apro_geometry::nozzle::{nozzle_contour, NozzleKind as GeomKind};

/// Convert NoseConeParams to a SolidOp stack.
pub fn nosecone_to_ops(params: &NoseConeParams) -> Vec<SolidOp> {
    let profile = convert_nose_profile(&params.profile);
    let tip_r = 0.2; // micro-tip radius to avoid pole singularity in revolve tessellation
    let pts = profile.sample_with_tip(params.length, params.base_radius, 64, tip_r);
    vec![SolidOp::Revolve {
        profile: Profile::Points(pts),
        angle: 360.0,
        axis: Some(Axis::Z),
    }]
}

fn convert_nose_profile(doc: &DocProfile) -> NoseProfile {
    match doc {
        DocProfile::Conical => NoseProfile::Conical,
        DocProfile::Ogive => NoseProfile::Ogive,
        DocProfile::VonKarman => NoseProfile::VonKarman,
        DocProfile::Haack { c } => NoseProfile::Haack { c: *c },
        DocProfile::Power { n } => NoseProfile::Power { n: *n },
        DocProfile::Parabolic { k } => NoseProfile::Parabolic { k: *k },
    }
}

/// Convert BodyTubeParams to a SolidOp stack.
pub fn bodytube_to_ops(params: &BodyTubeParams) -> Vec<SolidOp> {
    let pts = BodyTube::sample(params.length, params.radius, 32);
    vec![SolidOp::Revolve {
        profile: Profile::Points(pts),
        angle: 360.0,
        axis: Some(Axis::Z),
    }]
}

/// Convert TransitionParams to a SolidOp stack.
pub fn transition_to_ops(params: &TransitionParams) -> Vec<SolidOp> {
    let pts = Transition::sample(params.length, params.start_radius, params.end_radius, 32);
    vec![SolidOp::Revolve {
        profile: Profile::Points(pts),
        angle: 360.0,
        axis: Some(Axis::Z),
    }]
}

/// Convert TankParams to a SolidOp stack.
pub fn tank_to_ops(params: &TankParams) -> Vec<SolidOp> {
    let dome = convert_dome(&params.dome);
    let pts = Tank::sample(params.radius, params.cylindrical_length, &dome, 32);
    vec![SolidOp::Revolve {
        profile: Profile::Points(pts),
        angle: 360.0,
        axis: Some(Axis::Z),
    }]
}

fn convert_dome(doc: &DocDome) -> GeomDome {
    match doc {
        DocDome::Hemispherical => GeomDome::Hemispherical,
        DocDome::Ellipsoidal { ratio } => GeomDome::Ellipsoidal { ratio: *ratio },
    }
}

/// Convert NozzleParams to a SolidOp stack.
pub fn nozzle_to_ops(params: &NozzleParams) -> Vec<SolidOp> {
    let kind = convert_nozzle_kind(&params.kind);
    let pts = nozzle_contour(
        &kind,
        params.throat_radius,
        params.expansion_ratio,
        params.percent_bell,
        params.chamber_radius,
        48,
    );
    vec![SolidOp::Revolve {
        profile: Profile::Points(pts),
        angle: 360.0,
        axis: Some(Axis::Z),
    }]
}

fn convert_nozzle_kind(doc: &DocKind) -> GeomKind {
    match doc {
        DocKind::Conical => GeomKind::Conical,
        DocKind::Bell => GeomKind::Bell,
        DocKind::Moc => GeomKind::Moc,
    }
}

/// FinSet does not map to SolidOps (mesh-only construction).
pub fn finset_to_ops(_params: &FinSetParams) -> Result<Vec<SolidOp>, String> {
    Err("FinSet is mesh-only and cannot be represented as SolidOp operations".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::evaluate_solid_ops;

    #[test]
    fn test_nosecone_to_ops_produces_mesh() {
        let params = NoseConeParams {
            profile: DocProfile::VonKarman,
            length: 200.0, base_radius: 50.0, wall: 3.0,
            material: "Al-6061-T6".into(),
        };
        let ops = nosecone_to_ops(&params);
        assert_eq!(ops.len(), 1);
        let mesh = evaluate_solid_ops(&ops).expect("nosecone ops eval failed");
        assert!(mesh.positions.len() >= 9);
    }

    #[test]
    fn test_bodytube_to_ops_produces_mesh() {
        let params = BodyTubeParams {
            length: 500.0, radius: 50.0, wall: 3.0,
            material: "Al-6061-T6".into(),
        };
        let ops = bodytube_to_ops(&params);
        assert_eq!(ops.len(), 1);
        let mesh = evaluate_solid_ops(&ops).expect("bodytube ops eval failed");
        assert!(mesh.positions.len() >= 9);
    }

    #[test]
    fn test_transition_to_ops_produces_mesh() {
        let params = TransitionParams {
            length: 200.0, start_radius: 50.0, end_radius: 70.0, wall: 3.0,
            material: "Al-6061-T6".into(),
        };
        let ops = transition_to_ops(&params);
        assert_eq!(ops.len(), 1);
        let mesh = evaluate_solid_ops(&ops).expect("transition ops eval failed");
        assert!(mesh.positions.len() >= 9);
    }

    #[test]
    fn test_tank_to_ops_produces_mesh() {
        let params = TankParams {
            radius: 50.0, cylindrical_length: 300.0,
            dome: DocDome::Hemispherical, wall: 3.0,
            material: "Al-6061-T6".into(),
        };
        let ops = tank_to_ops(&params);
        assert_eq!(ops.len(), 1);
        let mesh = evaluate_solid_ops(&ops).expect("tank ops eval failed");
        assert!(mesh.positions.len() >= 9);
    }

    #[test]
    fn test_nozzle_to_ops_produces_mesh() {
        let params = NozzleParams {
            kind: DocKind::Conical,
            throat_radius: 25.0, expansion_ratio: 9.0,
            percent_bell: 80.0, chamber_radius: 75.0, wall: 3.0,
            material: "Al-6061-T6".into(),
        };
        let ops = nozzle_to_ops(&params);
        assert_eq!(ops.len(), 1);
        let mesh = evaluate_solid_ops(&ops).expect("nozzle ops eval failed");
        assert!(mesh.positions.len() >= 9);
    }

    #[test]
    fn test_finset_to_ops_errors() {
        let params = FinSetParams {
            count: 4, root_chord: 150.0, tip_chord: 80.0,
            span: 60.0, sweep: 20.0,
            airfoil: apro_document::vehicle::AirfoilParams {
                family: apro_document::vehicle::AirfoilFamily::NACA { digits: "0012".into() },
            },
            thickness: 3.0, material: "Al-6061-T6".into(),
        };
        assert!(finset_to_ops(&params).is_err());
    }
}
