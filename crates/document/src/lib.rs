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

    // -----------------------------------------------------------------------
    // Sketch entities
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_sketch_all_entity_kinds() {
        let ron_str = r#"Component(
            name: "Sketch1",
            material: "",
            kind: Sketch(SketchParams(
                plane: XY,
                offset: 0.0,
                entities: [
                    Line(start: (0.0, 0.0), end: (60.0, 0.0)),
                    Rectangle(corner1: (0.0, 0.0), corner2: (10.0, 5.0)),
                    Circle(center: (20.0, 20.0), radius: 7.5),
                    Arc(center: (0.0, 0.0), radius: 10.0, start_angle: 0.0, end_angle: 1.5707963),
                    Spline(points: [(0.0, 0.0), (5.0, 8.0), (10.0, 0.0)], closed: false),
                ],
            )),
        )"#;
        let comp: Component = ron::from_str(ron_str).expect("parse sketch");
        assert_eq!(comp.name, "Sketch1");
        match &comp.kind {
            ComponentKind::Sketch(s) => {
                assert_eq!(s.plane, SketchPlane::XY);
                assert_eq!(s.offset, 0.0);
                assert_eq!(s.entities.len(), 5);
                assert!(matches!(s.entities[0], SketchEntity::Line { .. }));
                assert!(matches!(s.entities[1], SketchEntity::Rectangle { .. }));
                assert!(matches!(s.entities[2], SketchEntity::Circle { radius, .. } if (radius - 7.5).abs() < 1e-9));
                assert!(matches!(s.entities[3], SketchEntity::Arc { radius, .. } if (radius - 10.0).abs() < 1e-9));
                assert!(matches!(&s.entities[4], SketchEntity::Spline { points, closed } if points.len() == 3 && !closed));
            }
            _ => panic!("expected Sketch"),
        }
        let ron_out = ron::to_string(&comp).expect("serialize");
        let back: Component = ron::from_str(&ron_out).expect("round-trip");
        assert_eq!(back, comp, "sketch must survive a RON round-trip");
    }

    #[test]
    fn test_sketch_plane_defaults_to_xy() {
        // `plane` and `offset` are optional so AI-authored sketches stay short.
        let ron_str = r#"Component(name: "S", kind: Sketch(SketchParams(entities: [
            Circle(center: (0.0, 0.0), radius: 5.0),
        ])))"#;
        let comp: Component = ron::from_str(ron_str).expect("parse");
        match &comp.kind {
            ComponentKind::Sketch(s) => {
                assert_eq!(s.plane, SketchPlane::XY);
                assert_eq!(s.offset, 0.0);
            }
            _ => panic!("expected Sketch"),
        }
    }

    #[test]
    fn test_sketch_plane_variants_and_frames() {
        for (plane, expect_normal) in [
            (SketchPlane::XY, [0.0, 0.0, 1.0]),
            (SketchPlane::XZ, [0.0, -1.0, 0.0]),
            (SketchPlane::YZ, [1.0, 0.0, 0.0]),
        ] {
            let (u, v, n) = plane.basis();
            assert_eq!(n, expect_normal, "{plane:?} normal");

            // The frame must be right-handed: u x v == n. A left-handed frame
            // would mirror the sketch and break revolve angle conventions.
            let cross = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            for i in 0..3 {
                assert!((cross[i] - n[i]).abs() < 1e-12, "{plane:?} not right-handed");
            }
            // Orthonormal.
            assert!((u[0] * v[0] + u[1] * v[1] + u[2] * v[2]).abs() < 1e-12);
        }
    }

    #[test]
    fn test_plane_origin_and_world_placement() {
        assert_eq!(SketchPlane::XY.origin_at(25.0), (0.0, 0.0, 25.0));
        assert_eq!(SketchPlane::XZ.origin_at(25.0), (0.0, -25.0, 0.0));
        assert_eq!(SketchPlane::YZ.origin_at(25.0), (25.0, 0.0, 0.0));

        // A point on the XZ plane places as (u, 0, v): u runs along +X, v along +Z.
        let w = SketchPlane::XZ.to_world([3.0, 7.0], (0.0, 0.0, 0.0));
        assert_eq!(w, [3.0, 0.0, 7.0]);
        // YZ: u runs along +Y, v along +Z.
        let w = SketchPlane::YZ.to_world([3.0, 7.0], (0.0, 0.0, 0.0));
        assert_eq!(w, [0.0, 3.0, 7.0]);
    }

    #[test]
    fn test_profile_reference_round_trips() {
        let ron_str = r#"Component(name: "Part", kind: Solid([
            Extrude(profile: Reference("Sketch1"), height: 12.0, direction: None, taper: None),
        ]))"#;
        let comp: Component = ron::from_str(ron_str).expect("parse");
        match &comp.kind {
            ComponentKind::Solid(ops) => match &ops[0] {
                SolidOp::Extrude { profile, height, .. } => {
                    assert_eq!(profile, &Profile::Reference("Sketch1".into()));
                    assert!((*height - 12.0).abs() < 1e-9);
                }
                _ => panic!("expected Extrude"),
            },
            _ => panic!("expected Solid"),
        }
        let ron_out = ron::to_string(&comp).expect("serialize");
        let back: Component = ron::from_str(&ron_out).expect("round-trip");
        assert_eq!(back, comp);
    }
}
