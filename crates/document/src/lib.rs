pub mod vehicle;
pub mod material;
pub mod patch;
pub mod params;

pub mod validation;
pub use vehicle::*;
pub use material::*;
pub use patch::*;
pub use params::*;

#[cfg(test)]
mod tests {
    use crate::vehicle::*;

    fn check_nosecone(ron_str: &str, expected_profile: NoseConeProfile) {
        let component: Component = ron::from_str(ron_str).expect("Failed to parse RON");
        assert_eq!(component.name, "NoseCone-1");
        match &component.kind {
            ComponentKind::NoseCone(params) => {
                assert_eq!(params.profile, expected_profile);
                assert!((params.length - 300.0).abs() < 1e-6);
                assert!((params.base_radius - 54.0).abs() < 1e-6);
                assert!((params.wall - 2.0).abs() < 1e-6);
                assert_eq!(params.material, "Al-6061-T6");
            }
            _ => panic!("Expected NoseCone variant"),
        }
        let ron_out = ron::to_string(&component).expect("Failed to serialize RON");
        let _: Component = ron::from_str(&ron_out).expect("Round-trip failed");
    }

    #[test]
    fn test_conical() {
        check_nosecone(r#"Component( name: "NoseCone-1", kind: NoseCone(NoseConeParams(profile: Conical, length: 300.0, base_radius: 54.0, wall: 2.0, material: "Al-6061-T6")) )"#, NoseConeProfile::Conical);
    }

    #[test]
    fn test_ogive() {
        check_nosecone(r#"Component( name: "NoseCone-1", kind: NoseCone(NoseConeParams(profile: Ogive, length: 300.0, base_radius: 54.0, wall: 2.0, material: "Al-6061-T6")) )"#, NoseConeProfile::Ogive);
    }

    #[test]
    fn test_vonkarman() {
        check_nosecone(r#"Component( name: "NoseCone-1", kind: NoseCone(NoseConeParams(profile: VonKarman, length: 300.0, base_radius: 54.0, wall: 2.0, material: "Al-6061-T6")) )"#, NoseConeProfile::VonKarman);
    }

    #[test]
    fn test_haack() {
        check_nosecone(r#"Component( name: "NoseCone-1", kind: NoseCone(NoseConeParams(profile: Haack(c: 0.333), length: 300.0, base_radius: 54.0, wall: 2.0, material: "Al-6061-T6")) )"#, NoseConeProfile::Haack { c: 0.333 });
    }

    #[test]
    fn test_power() {
        check_nosecone(r#"Component( name: "NoseCone-1", kind: NoseCone(NoseConeParams(profile: Power(n: 0.75), length: 300.0, base_radius: 54.0, wall: 2.0, material: "Al-6061-T6")) )"#, NoseConeProfile::Power { n: 0.75 });
    }

    #[test]
    fn test_parabolic() {
        check_nosecone(r#"Component( name: "NoseCone-1", kind: NoseCone(NoseConeParams(profile: Parabolic(k: 0.5), length: 300.0, base_radius: 54.0, wall: 2.0, material: "Al-6061-T6")) )"#, NoseConeProfile::Parabolic { k: 0.5 });
    }

    #[test]
    fn test_parse_bodytube() {
        let ron_str = r#"Component( name: "Tube-1", kind: BodyTube(BodyTubeParams(length: 500.0, radius: 54.0, wall: 2.0, material: "Al-6061-T6")) )"#;
        let comp: Component = ron::from_str(ron_str).expect("parse");
        assert_eq!(comp.name, "Tube-1");
        match &comp.kind {
            ComponentKind::BodyTube(p) => {
                assert!((p.length - 500.0).abs() < 1e-6);
                assert!((p.radius - 54.0).abs() < 1e-6);
            }
            _ => panic!("expected BodyTube"),
        }
        let ron_out = ron::to_string(&comp).expect("serialize");
        let _: Component = ron::from_str(&ron_out).expect("round-trip");
    }

    #[test]
    fn test_parse_transition() {
        let ron_str = r#"Component( name: "Trans-1", kind: Transition(TransitionParams(length: 200.0, start_radius: 54.0, end_radius: 70.0, wall: 2.0, material: "Al-6061-T6")) )"#;
        let comp: Component = ron::from_str(ron_str).expect("parse");
        match &comp.kind {
            ComponentKind::Transition(p) => {
                assert!((p.length - 200.0).abs() < 1e-6);
                assert!((p.start_radius - 54.0).abs() < 1e-6);
                assert!((p.end_radius - 70.0).abs() < 1e-6);
            }
            _ => panic!("expected Transition"),
        }
    }

    #[test]
    fn test_parse_tank_hemispherical() {
        let ron_str = r#"Component( name: "Tank-1", kind: Tank(TankParams(radius: 54.0, cylindrical_length: 800.0, dome: Hemispherical, wall: 3.0, material: "Al-6061-T6")) )"#;
        let comp: Component = ron::from_str(ron_str).expect("parse");
        match &comp.kind {
            ComponentKind::Tank(p) => {
                assert!((p.radius - 54.0).abs() < 1e-6);
                assert!((p.cylindrical_length - 800.0).abs() < 1e-6);
                assert_eq!(p.dome, DomeKind::Hemispherical);
            }
            _ => panic!("expected Tank"),
        }
    }

    #[test]
    fn test_parse_tank_ellipsoidal() {
        let ron_str = r#"Component( name: "Tank-2", kind: Tank(TankParams(radius: 54.0, cylindrical_length: 600.0, dome: Ellipsoidal(ratio: 2.0), wall: 3.0, material: "Al-6061-T6")) )"#;
        let comp: Component = ron::from_str(ron_str).expect("parse");
        match &comp.kind {
            ComponentKind::Tank(p) => {
                assert_eq!(p.dome, DomeKind::Ellipsoidal { ratio: 2.0 });
            }
            _ => panic!("expected Tank"),
        }
    }

    #[test]
    fn test_parse_nozzle_conical() {
        let ron_str = r#"Component( name: "Nozzle-1", kind: Nozzle(NozzleParams(kind: Conical, throat_radius: 20.0, expansion_ratio: 9.0, percent_bell: 100.0, chamber_radius: 60.0, wall: 3.0, material: "Inconel-718")) )"#;
        let comp: Component = ron::from_str(ron_str).expect("parse");
        match &comp.kind {
            ComponentKind::Nozzle(p) => {
                assert_eq!(p.kind, NozzleKind::Conical);
                assert!((p.throat_radius - 20.0).abs() < 1e-6);
            }
            _ => panic!("expected Nozzle"),
        }
    }

    #[test]
    fn test_parse_nozzle_bell() {
        let ron_str = r#"Component( name: "Nozzle-2", kind: Nozzle(NozzleParams(kind: Bell, throat_radius: 20.0, expansion_ratio: 16.0, percent_bell: 85.0, chamber_radius: 60.0, wall: 3.0, material: "Inconel-718")) )"#;
        let comp: Component = ron::from_str(ron_str).expect("parse");
        match &comp.kind {
            ComponentKind::Nozzle(p) => assert_eq!(p.kind, NozzleKind::Bell),
            _ => panic!("expected Nozzle"),
        }
    }

    #[test]
    fn test_parse_nozzle_roundtrip() {
        let params = NozzleParams {
            kind: NozzleKind::Bell, throat_radius: 20.0, expansion_ratio: 16.0,
            percent_bell: 85.0, chamber_radius: 60.0, wall: 3.0, material: "Inconel-718".into(),
        };
        let c = Component { name: "N".into(), material: String::new(), color: None, visible: true, transform: Transform::default(), kind: ComponentKind::Nozzle(params) };
        let ron_str = ron::to_string(&c).expect("serialize");
        let _: Component = ron::from_str(&ron_str).expect("round-trip");
    }

    #[test]
    fn test_parse_finset() {
        let ron_str = r#"Component( name: "FinSet-1", kind: FinSet(FinSetParams(count: 4, root_chord: 120.0, tip_chord: 60.0, span: 60.0, sweep: 15.0, airfoil: AirfoilParams(family: NACA(digits: "0012")), thickness: 3.0, material: "Al-6061-T6")) )"#;
        let comp: Component = ron::from_str(ron_str).expect("parse");
        match &comp.kind {
            ComponentKind::FinSet(p) => {
                assert_eq!(p.count, 4);
                assert!((p.root_chord - 120.0).abs() < 1e-6);
            }
            _ => panic!("expected FinSet"),
        }
    }

    #[test]
    fn test_roundtrip_all_profiles() {
        let profiles = vec![
            NoseConeProfile::Conical,
            NoseConeProfile::Ogive,
            NoseConeProfile::VonKarman,
            NoseConeProfile::Haack { c: 0.333 },
            NoseConeProfile::Power { n: 0.75 },
            NoseConeProfile::Parabolic { k: 0.5 },
        ];
        for profile in profiles {
            let params = NoseConeParams {
                profile: profile.clone(),
                length: 200.0,
                base_radius: 50.0,
                wall: 3.0,
                material: "Test".into(),
            };
            let c = Component { name: "T".into(), material: String::new(), color: None, visible: true, transform: Transform::default(), kind: ComponentKind::NoseCone(params) };
            let ron_str = ron::to_string(&c).expect("serialize");
            let back: Component = ron::from_str(&ron_str).expect("deserialize");
            match back.kind {
                ComponentKind::NoseCone(p) => assert_eq!(p.profile, profile),
                _ => panic!("wrong variant"),
            }
        }
    }

    #[test]
    fn test_parse_solid_custom() {
        let ron_str = r#"Component( name: "Vase", material: "Al-6061-T6", kind: Solid([Revolve(profile: Points([(0.0,0.0),(50.0,100.0),(54.0,300.0)]), angle: 360.0, axis: None)]))"#;
        let comp: Component = ron::from_str(ron_str).expect("parse");
        assert_eq!(comp.name, "Vase");
        match &comp.kind {
            ComponentKind::Solid(ops) => {
                assert_eq!(ops.len(), 1);
                match &ops[0] {
                    SolidOp::Revolve { profile, angle, axis } => {
                        assert!((*angle - 360.0).abs() < 1e-6);
                        assert_eq!(axis, &None);
                        match profile {
                            Profile::Points(pts) => assert_eq!(pts.len(), 3),
                            _ => panic!("expected Points"),
                        }
                    }
                    _ => panic!("expected Revolve"),
                }
            }
            _ => panic!("expected Solid"),
        }
        let ron_out = ron::to_string(&comp).expect("serialize");
        let back: Component = ron::from_str(&ron_out).expect("round-trip");
        assert!(matches!(back.kind, ComponentKind::Solid(_)));
    }
}
