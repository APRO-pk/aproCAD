use apro_document::params::{ParamEnv, resolve_parameters};
use apro_document::vehicle::{Component, ComponentKind, Vehicle};
use apro_kernel::{transform_mesh_srt, MeshData};
use crate::eval::{evaluate_solid_ops_full, ResolvedSketch};
use crate::sketch::compile_sketch;
use crate::shorthands::{nosecone_to_ops, bodytube_to_ops, transition_to_ops, tank_to_ops, nozzle_to_ops};

/// Resolve the vehicle's parameter block into an environment. On failure the
/// vehicle still evaluates (with unresolved parameters = empty env) but the
/// problem is reported on stderr; validation surfaces it as an Error issue.
pub fn resolve_vehicle_params(vehicle: &Vehicle) -> ParamEnv {
    let mut env = ParamEnv::new();
    if let Some(ps) = &vehicle.parameters {
        match resolve_parameters(ps) {
            Ok(resolved) => env.extend(resolved.into_iter()),
            Err(e) => eprintln!("[warn] Parameters: {e}"),
        }
    }
    env
}

pub fn build_vehicle_mesh(vehicle: &Vehicle) -> MeshData {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    let env = resolve_vehicle_params(vehicle);

    for comp in &vehicle.components {
        let mut stack = Vec::new();
        let mut resolver = |name: &str| -> Result<MeshData, String> {
            resolve_component_mesh(vehicle, name, &mut stack)
        };
        let mesh = build_component_mesh_with(vehicle, comp, &env, &mut resolver);
        let tf = &comp.transform;
        let s = tf.scale.unwrap_or((1.0, 1.0, 1.0));
        let transformed = transform_mesh_srt(
            &mesh,
            &[tf.position.0, tf.position.1, tf.position.2],
            &[tf.rotation.0, tf.rotation.1, tf.rotation.2],
            &[s.0, s.1, s.2],
        );
        let base = positions.len() as u32 / 3;
        positions.extend_from_slice(&transformed.positions);
        normals.extend_from_slice(&transformed.normals);
        for idx in &transformed.indices {
            indices.push(base + idx);
        }
    }

    MeshData { positions, normals, indices }
}

/// Resolve a sibling component's local (untransformed) mesh for a Boolean op.
/// `stack` holds the chain of component names currently being resolved, so
/// circular references are detected and reported instead of recursing forever.
fn resolve_component_mesh(
    vehicle: &Vehicle,
    name: &str,
    stack: &mut Vec<String>,
) -> Result<MeshData, String> {
    if stack.iter().any(|n| n == name) {
        return Err(format!("circular boolean reference: component '{name}' depends on itself"));
    }
    let comp = vehicle.components.iter().find(|c| c.name == name)
        .ok_or_else(|| format!("boolean target component '{name}' not found in vehicle"))?;
    stack.push(name.to_string());
    let env = resolve_vehicle_params(vehicle);
    let mut resolver = |dep: &str| -> Result<MeshData, String> {
        resolve_component_mesh(vehicle, dep, stack)
    };
    let result = try_build_component_mesh(vehicle, comp, &env, &mut resolver);
    stack.pop();
    result
}

/// Build a component's local mesh, reporting failures via `Err` so callers can
/// decide how to surface them (warning vs. silent cache miss). Boolean targets
/// resolve through `resolve`; sketches resolve against the vehicle's sketch
/// components; profile expressions evaluate against `params`.
pub fn try_build_component_mesh(
    vehicle: &Vehicle,
    comp: &Component,
    params: &ParamEnv,
    resolve: &mut dyn FnMut(&str) -> Result<MeshData, String>,
) -> Result<MeshData, String> {
    let mut eval = |ops: &[apro_document::vehicle::SolidOp]| -> Result<MeshData, String> {
        let mut resolver = |name: &str| -> Result<MeshData, String> {
            resolve(name)
        };
        let mut sketch_lookup = |name: &str| -> Result<ResolvedSketch, String> {
            lookup_sketch(vehicle, name)
        };
        evaluate_solid_ops_full(ops, params, &mut sketch_lookup, &mut resolver)
    };
    match &comp.kind {
        ComponentKind::NoseCone(p) => eval(&nosecone_to_ops(p)),
        ComponentKind::BodyTube(p) => eval(&bodytube_to_ops(p)),
        ComponentKind::Transition(p) => eval(&transition_to_ops(p)),
        ComponentKind::Tank(p) => eval(&tank_to_ops(p)),
        ComponentKind::Nozzle(p) => eval(&nozzle_to_ops(p)),
        ComponentKind::FinSet(p) => Ok(crate::fin::mesh_finset(p)),
        ComponentKind::Solid(ops) => eval(ops),
        // A sketch is construction geometry: it defines no solid on its own.
        // Solid components reference it through `Profile::Reference`.
        ComponentKind::Sketch(_) => Ok(MeshData::default()),
    }
}

/// Resolve a named sketch component into a closed profile loop plus its
/// workplane placement.
pub fn lookup_sketch(vehicle: &Vehicle, name: &str) -> Result<ResolvedSketch, String> {
    let comp = vehicle
        .components
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| format!("sketch '{name}' not found in vehicle"))?;
    let sketch = match &comp.kind {
        ComponentKind::Sketch(s) => s,
        _ => return Err(format!("'{name}' is not a sketch")),
    };
    let compiled = compile_sketch(sketch);
    let loop_pts = compiled.single_profile().map_err(|e| format!("sketch '{name}': {e}"))?;
    Ok(ResolvedSketch {
        loop_pts,
        plane: sketch.plane,
        origin: sketch.plane.origin_at(sketch.offset),
    })
}

/// Build a component's local mesh, converting failures into an empty mesh with a
/// warning (used by the UI assembly path where an error is not fatal).
pub fn build_component_mesh_with(
    vehicle: &Vehicle,
    comp: &Component,
    params: &ParamEnv,
    resolve: &mut dyn FnMut(&str) -> Result<MeshData, String>,
) -> MeshData {
    match try_build_component_mesh(vehicle, comp, params, resolve) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[warn] Component '{}' evaluation failed: {e}", comp.name);
            MeshData { positions: vec![], normals: vec![], indices: vec![] }
        }
    }
}

#[cfg(test)]
mod tests {
    use apro_document::vehicle::*;
    use super::{build_vehicle_mesh, lookup_sketch};
    use apro_kernel::MeshData;

    #[test]
    fn test_single_component_vehicle() {
        let v = Vehicle { uid: None, parameters: None,
            name: "Test".into(),
            units: Units::Millimeters,
            components: vec![Component {
                name: "Nose".into(),
                material: "Al-6061-T6".into(),
                visible: true,
                transform: Transform::default(),
                color: None,
                kind: ComponentKind::NoseCone(NoseConeParams {
                    profile: NoseConeProfile::VonKarman,
                    length: 200.0,
                    base_radius: 50.0,
                    wall: 3.0,
                    material: "Al-6061-T6".into(),
                }),
            }],
        };
        let mesh = build_vehicle_mesh(&v);
        assert!(mesh.positions.len() >= 9);
        assert!(mesh.indices.len() >= 3);
    }

    #[test]
    fn test_multi_component_vehicle() {
        let v = Vehicle { uid: None, parameters: None,
            name: "Full Rocket".into(),
            units: Units::Millimeters,
            components: vec![
                Component {
                    name: "Nose".into(),
                    material: "Al-6061-T6".into(),
                    visible: true,
                    transform: Transform::default(),
                    color: None,
                    kind: ComponentKind::NoseCone(NoseConeParams {
                        profile: NoseConeProfile::VonKarman,
                        length: 200.0, base_radius: 50.0, wall: 3.0,
                        material: "Al-6061-T6".into(),
                    }),
                },
                Component {
                    name: "Body".into(),
                    material: "Al-6061-T6".into(),
                    visible: true,
                    transform: Transform { position: (0.0, 0.0, 200.0), rotation: (0.0, 0.0, 0.0), scale: None },
                    color: None,
                    kind: ComponentKind::BodyTube(BodyTubeParams {
                        length: 500.0, radius: 50.0, wall: 3.0,
                        material: "Al-6061-T6".into(),
                    }),
                },
            ],
        };
        let mesh = build_vehicle_mesh(&v);
        assert!(mesh.positions.len() >= 9);
        let max_z = mesh.positions.iter().skip(2).step_by(3).copied().fold(0.0_f32, f32::max);
        assert!(max_z > 699.0, "expected z >= 700, got {}", max_z);
    }

    #[test]
    fn test_transform_xyz_position_offset() {
        let v = Vehicle { uid: None, parameters: None,
            name: "Offset".into(),
            units: Units::Millimeters,
            components: vec![Component {
                name: "Body".into(),
                material: "Al-6061-T6".into(),
                visible: true,
                transform: Transform { position: (100.0, 200.0, 300.0), rotation: (0.0, 0.0, 0.0), scale: None },
                color: None,
                kind: ComponentKind::BodyTube(BodyTubeParams {
                    length: 100.0, radius: 10.0, wall: 2.0,
                    material: "Al-6061-T6".into(),
                }),
            }],
        };
        let mesh = build_vehicle_mesh(&v);
        assert!(mesh.positions.len() >= 9);
        // All X positions should be shifted by ~100, Y by ~200, Z by ~300
        let min_x = mesh.positions.iter().step_by(3).copied().fold(f32::MAX, f32::min);
        let min_y = mesh.positions.iter().skip(1).step_by(3).copied().fold(f32::MAX, f32::min);
        let min_z = mesh.positions.iter().skip(2).step_by(3).copied().fold(f32::MAX, f32::min);
        assert!(min_x >= 90.0, "min_x expected >= 90, got {}", min_x);
        assert!(min_y >= 190.0, "min_y expected >= 190, got {}", min_y);
        assert!(min_z >= 290.0, "min_z expected >= 290, got {}", min_z);
    }

    #[test]
    fn test_transform_rotation_moves_points() {
        // Extrude a rectangle, rotate 90° around X so Y→Z, verify Z values change
        let v = Vehicle { uid: None, parameters: None,
            name: "Rotated".into(),
            units: Units::Millimeters,
            components: vec![Component {
                name: "Rect".into(),
                material: "Al-6061-T6".into(),
                visible: true,
                transform: Transform { position: (0.0, 0.0, 0.0), rotation: (90.0, 0.0, 0.0), scale: None },
                color: None,
                kind: ComponentKind::Solid(vec![
                    apro_document::vehicle::SolidOp::Extrude {
                        profile: apro_document::vehicle::Profile::Rectangle {
                            width: 100.0, height: 50.0, corner_radius: None,
                        },
                        height: 200.0,
                        direction: None,
                        taper: None,
                    },
                ]),
            }],
        };
        let mesh = build_vehicle_mesh(&v);
        assert!(mesh.positions.len() >= 9);
        // After 90° X rotation, original Z values move to -Y
        let min_y = mesh.positions.iter().skip(1).step_by(3).copied().fold(f32::MAX, f32::min);
        let max_z = mesh.positions.iter().skip(2).step_by(3).copied().fold(f32::MIN, f32::max);
        assert!(min_y < -180.0, "min_y expected < -180 (Z rotated to -Y), got {}", min_y);
        assert!(max_z > 20.0, "max_z expected > 20 (Y rotated to Z), got {}", max_z);
    }

    #[test]
    fn test_boolean_difference_across_components() {
        // Base block (local box z 0..20) booleans out the Bore component's
        // LOCAL cylinder mesh (z 0..20). The Bore is rendered far away in the
        // assembly, so the hole at the base's top face must be open.
        let v = Vehicle { uid: None, parameters: None,
            name: "Hole".into(),
            units: Units::Millimeters,
            components: vec![
                Component {
                    name: "Base".into(),
                    material: "Al-6061-T6".into(),
                    visible: true,
                    transform: Transform::default(),
                    color: None,
                    kind: ComponentKind::Solid(vec![
                        SolidOp::Extrude {
                            profile: Profile::Rectangle { width: 60.0, height: 60.0, corner_radius: None },
                            height: 20.0,
                            direction: None,
                            taper: None,
                        },
                        SolidOp::Boolean {
                            kind: BooleanKind::Difference,
                            target: SolidRef::Component("Bore".into()),
                        },
                    ]),
                },
                Component {
                    name: "Bore".into(),
                    material: "Al-6061-T6".into(),
                    visible: true,
                    transform: Transform { position: (0.0, 0.0, 500.0), rotation: (0.0, 0.0, 0.0), scale: None },
                    color: None,
                    kind: ComponentKind::Solid(vec![
                        SolidOp::Extrude {
                            profile: Profile::Circle { radius: 10.0 },
                            height: 20.0,
                            direction: None,
                            taper: None,
                        },
                    ]),
                },
            ],
        };
        let mesh = build_vehicle_mesh(&v);
        assert!(mesh.positions.len() >= 9);
        // Top of the base sits at z=20; the hole (r=10) must be open there.
        let center_filled = mesh.positions.chunks(3).any(|p| {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            (p[2] - 20.0).abs() < 1.0 && r < 9.0
        });
        assert!(!center_filled, "base top face hole must be open (r<9 removed)");
        // The Bore cylinder is rendered at z=500, so the assembly reaches there.
        let max_z = mesh.positions.iter().skip(2).step_by(3).copied().fold(f32::MIN, f32::max);
        assert!(max_z > 490.0, "expected Bore rendered near z=500, got max_z {max_z}");
    }

    #[test]
    fn test_boolean_circular_reference_errors() {
        // A depends on B; B depends on A -> the circular reference must be
        // reported instead of recursing forever (would stack overflow).
        let v = Vehicle { uid: None, parameters: None,
            name: "Circular".into(),
            units: Units::Millimeters,
            components: vec![
                Component {
                    name: "A".into(),
                    material: "Al-6061-T6".into(),
                    visible: true,
                    transform: Transform::default(),
                    color: None,
                    kind: ComponentKind::Solid(vec![
                        SolidOp::Extrude {
                            profile: Profile::Rectangle { width: 60.0, height: 60.0, corner_radius: None },
                            height: 20.0,
                            direction: None,
                            taper: None,
                        },
                        SolidOp::Boolean {
                            kind: BooleanKind::Difference,
                            target: SolidRef::Component("B".into()),
                        },
                    ]),
                },
                Component {
                    name: "B".into(),
                    material: "Al-6061-T6".into(),
                    visible: true,
                    transform: Transform::default(),
                    color: None,
                    kind: ComponentKind::Solid(vec![
                        SolidOp::Extrude {
                            profile: Profile::Rectangle { width: 60.0, height: 60.0, corner_radius: None },
                            height: 20.0,
                            direction: None,
                            taper: None,
                        },
                        SolidOp::Boolean {
                            kind: BooleanKind::Union,
                            target: SolidRef::Component("A".into()),
                        },
                    ]),
                },
            ],
        };
        // Must not hang or panic; A and B both produce empty meshes with a warning.
        let mesh = build_vehicle_mesh(&v);
        assert!(mesh.positions.is_empty(), "circular refs must yield no geometry");
    }

    // -----------------------------------------------------------------------
    // Sketch -> solid
    // -----------------------------------------------------------------------

    fn sketch_comp(name: &str, plane: SketchPlane, offset: f64, entities: Vec<SketchEntity>) -> Component {
        Component {
            name: name.into(),
            material: String::new(),
            visible: true,
            transform: Transform::default(),
            color: None,
            kind: ComponentKind::Sketch(SketchParams { plane, offset, entities }),
        }
    }

    fn extrude_of(name: &str, height: f64) -> Component {
        Component {
            name: "Part".into(),
            material: "Al-6061-T6".into(),
            visible: true,
            transform: Transform::default(),
            color: None,
            kind: ComponentKind::Solid(vec![SolidOp::Extrude {
                profile: Profile::Reference(name.into()),
                height,
                direction: None,
                taper: None,
            }]),
        }
    }

    fn vehicle_of(comps: Vec<Component>) -> Vehicle {
        Vehicle { uid: None, parameters: None, name: "SketchDoc".into(), units: Units::Millimeters, components: comps }
    }

    fn bbox(mesh: &MeshData) -> [f32; 6] {
        apro_kernel::aabb(mesh).expect("mesh should have geometry")
    }

    #[test]
    fn test_sketch_rectangle_extrudes_to_a_block() {
        let v = vehicle_of(vec![
            sketch_comp(
                "Sketch1",
                SketchPlane::XY,
                0.0,
                vec![SketchEntity::Rectangle { corner1: [0.0, 0.0], corner2: [10.0, 10.0] }],
            ),
            extrude_of("Sketch1", 4.0),
        ]);
        let mesh = build_vehicle_mesh(&v);
        assert!(!mesh.positions.is_empty(), "sketch extrude must produce geometry");
        // 10 x 10 x 4 = 400.
        let vol = apro_kernel::signed_volume(&mesh);
        assert!((vol - 400.0).abs() < 1.0, "expected ~400, got {vol}");
        let b = bbox(&mesh);
        assert!(b[0].abs() < 0.01 && b[1].abs() < 0.01 && b[2].abs() < 0.01, "min corner: {b:?}");
        assert!((b[3] - 10.0).abs() < 0.01 && (b[4] - 10.0).abs() < 0.01 && (b[5] - 4.0).abs() < 0.01, "max corner: {b:?}");
    }

    #[test]
    fn test_sketch_circle_extrudes_and_tessellates() {
        let v = vehicle_of(vec![
            sketch_comp(
                "Disc",
                SketchPlane::XY,
                0.0,
                vec![SketchEntity::Circle { center: [0.0, 0.0], radius: 5.0 }],
            ),
            extrude_of("Disc", 4.0),
        ]);
        let mesh = build_vehicle_mesh(&v);
        let vol = apro_kernel::signed_volume(&mesh);
        // A 64-gon inscribed in r=5, 4 tall -> slightly under pi*25*4 = 314.16.
        assert!(vol > 310.0 && vol < 314.5, "expected ~313, got {vol}");
    }

    #[test]
    fn test_lines_forming_a_square_extrude_like_a_rectangle() {
        // The whole point of geometric stitching: four separate picks close.
        let sq = vec![
            SketchEntity::Line { start: [0.0, 0.0], end: [10.0, 0.0] },
            SketchEntity::Line { start: [10.0, 0.0], end: [10.0, 10.0] },
            SketchEntity::Line { start: [10.0, 10.0], end: [0.0, 10.0] },
            SketchEntity::Line { start: [0.0, 10.0], end: [0.0, 0.0] },
        ];
        let v = vehicle_of(vec![
            sketch_comp("Loop", SketchPlane::XY, 0.0, sq.clone()),
            extrude_of("Loop", 2.0),
        ]);
        let mesh = build_vehicle_mesh(&v);
        let vol = apro_kernel::signed_volume(&mesh);
        assert!((vol - 200.0).abs() < 1.0, "expected ~200, got {vol}");

        // A rectangle entity with the same corners must give the same volume.
        let v2 = vehicle_of(vec![
            sketch_comp(
                "Loop",
                SketchPlane::XY,
                0.0,
                vec![SketchEntity::Rectangle { corner1: [0.0, 0.0], corner2: [10.0, 10.0] }],
            ),
            extrude_of("Loop", 2.0),
        ]);
        let vol2 = apro_kernel::signed_volume(&build_vehicle_mesh(&v2));
        assert!((vol - vol2).abs() < 0.5, "lines {vol} vs rectangle {vol2}");
    }

    #[test]
    fn test_arc_and_line_close_into_a_d_profile() {
        let half_disc = vec![
            SketchEntity::Arc {
                center: [0.0, 0.0],
                radius: 10.0,
                start_angle: 0.0,
                end_angle: std::f64::consts::PI,
            },
            SketchEntity::Line { start: [-10.0, 0.0], end: [10.0, 0.0] },
        ];
        let v = vehicle_of(vec![
            sketch_comp("D", SketchPlane::XY, 0.0, half_disc),
            extrude_of("D", 3.0),
        ]);
        let mesh = build_vehicle_mesh(&v);
        assert!(!mesh.positions.is_empty(), "arc + chord must close a profile");
        let vol = apro_kernel::signed_volume(&mesh);
        let expected = std::f64::consts::PI * 100.0 / 2.0 * 3.0;
        assert!((vol - expected).abs() / expected < 0.01, "expected ~{expected}, got {vol}");
    }

    #[test]
    fn test_xz_plane_sketch_stands_up_in_y() {
        // A sketch on XZ is drawn in (u=x, v=z) and must extrude along -Y.
        let v = vehicle_of(vec![
            sketch_comp(
                "Side",
                SketchPlane::XZ,
                0.0,
                vec![SketchEntity::Rectangle { corner1: [0.0, 0.0], corner2: [10.0, 5.0] }],
            ),
            extrude_of("Side", 4.0),
        ]);
        let mesh = build_vehicle_mesh(&v);
        let b = bbox(&mesh);
        // u spans x 0..10, v spans z 0..5, and the thickness runs in -Y by 4.
        assert!((b[0]).abs() < 0.01 && (b[3] - 10.0).abs() < 0.01, "x span: {b:?}");
        assert!((b[2]).abs() < 0.01 && (b[5] - 5.0).abs() < 0.01, "z span: {b:?}");
        assert!((b[4]).abs() < 0.01 && (b[1] + 4.0).abs() < 0.01, "y span should be -4..0: {b:?}");
        let vol = apro_kernel::signed_volume(&mesh);
        assert!((vol - 200.0).abs() < 1.0, "expected ~200, got {vol}");
    }

    #[test]
    fn test_sketch_offset_lifts_the_part() {
        let v = vehicle_of(vec![
            sketch_comp(
                "Lifted",
                SketchPlane::XY,
                12.0,
                vec![SketchEntity::Rectangle { corner1: [0.0, 0.0], corner2: [4.0, 4.0] }],
            ),
            extrude_of("Lifted", 3.0),
        ]);
        let b = bbox(&build_vehicle_mesh(&v));
        assert!((b[2] - 12.0).abs() < 0.01, "bottom should sit at z=12: {b:?}");
        assert!((b[5] - 15.0).abs() < 0.01, "top should sit at z=15: {b:?}");
    }

    #[test]
    fn test_missing_sketch_is_reported_not_silent() {
        let v = vehicle_of(vec![extrude_of("NoSuchSketch", 5.0)]);
        // The vehicle path swallows the error into an empty mesh + warning...
        assert!(build_vehicle_mesh(&v).positions.is_empty());
        // ...but the typed lookup surfaces the real reason.
        let err = lookup_sketch(&v, "NoSuchSketch").unwrap_err();
        assert!(err.contains("not found"), "got: {err}");
    }

    #[test]
    fn test_referencing_a_non_sketch_component_is_an_error() {
        let v = vehicle_of(vec![
            Component {
                name: "NotASketch".into(),
                material: "Al-6061-T6".into(),
                visible: true,
                transform: Transform::default(),
                color: None,
                kind: ComponentKind::Solid(vec![SolidOp::Extrude {
                    profile: Profile::Rectangle { width: 5.0, height: 5.0, corner_radius: None },
                    height: 5.0,
                    direction: None,
                    taper: None,
                }]),
            },
            extrude_of("NotASketch", 5.0),
        ]);
        let err = lookup_sketch(&v, "NotASketch").unwrap_err();
        assert!(err.contains("is not a sketch"), "got: {err}");
    }

    #[test]
    fn test_open_sketch_reports_why_it_cannot_extrude() {
        let v = vehicle_of(vec![
            sketch_comp(
                "Open",
                SketchPlane::XY,
                0.0,
                vec![SketchEntity::Line { start: [0.0, 0.0], end: [10.0, 0.0] }],
            ),
            extrude_of("Open", 3.0),
        ]);
        let err = lookup_sketch(&v, "Open").unwrap_err();
        assert!(err.contains("not closed"), "got: {err}");
    }

    #[test]
    fn test_sketch_component_alone_produces_no_solid() {
        let v = vehicle_of(vec![sketch_comp(
            "Only",
            SketchPlane::XY,
            0.0,
            vec![SketchEntity::Circle { center: [0.0, 0.0], radius: 5.0 }],
        )]);
        // Construction geometry must not appear as a mesh in the assembly.
        assert!(build_vehicle_mesh(&v).positions.is_empty());
    }

    #[test]
    fn test_sketch_reference_respects_height_and_direction() {
        let mk = |dir: Option<Direction>| {
            vehicle_of(vec![
                sketch_comp(
                    "S",
                    SketchPlane::XY,
                    0.0,
                    vec![SketchEntity::Rectangle { corner1: [0.0, 0.0], corner2: [2.0, 2.0] }],
                ),
                Component {
                    name: "P".into(),
                    material: "Al-6061-T6".into(),
                    visible: true,
                    transform: Transform::default(),
                    color: None,
                    kind: ComponentKind::Solid(vec![SolidOp::Extrude {
                        profile: Profile::Reference("S".into()),
                        height: 6.0,
                        direction: dir,
                        taper: None,
                    }]),
                },
            ])
        };
        let up = bbox(&build_vehicle_mesh(&mk(None)));
        assert!((up[2]).abs() < 0.01 && (up[5] - 6.0).abs() < 0.01, "up: {up:?}");
        let down = bbox(&build_vehicle_mesh(&mk(Some(Direction::NegZ))));
        assert!((down[2] + 6.0).abs() < 0.01 && (down[5]).abs() < 0.01, "down: {down:?}");
    }

    #[test]
    fn test_tapered_extrude_honours_height() {
        // Regression: the taper path used to loft to a unit height, so any
        // `Extrude(taper: Some(..), height: N)` came out 1 unit tall.
        let v = vehicle_of(vec![Component {
            name: "Tapered".into(),
            material: "Al-6061-T6".into(),
            visible: true,
            transform: Transform::default(),
            color: None,
            kind: ComponentKind::Solid(vec![SolidOp::Extrude {
                profile: Profile::Rectangle { width: 10.0, height: 10.0, corner_radius: None },
                height: 5.0,
                direction: None,
                taper: Some(0.2),
            }]),
        }]);
        let b = bbox(&build_vehicle_mesh(&v));
        assert!((b[2]).abs() < 0.01, "base sits at z=0: {b:?}");
        assert!((b[5] - 5.0).abs() < 0.01, "tapered solid must be 5 tall, not 1: {b:?}");
        // `taper` scales the top face about the origin by (1 + taper): a
        // 10-wide base (x -5..5) flares to 12 wide (x -6..6).
        assert!((b[0] + 6.0).abs() < 0.01 && (b[3] - 6.0).abs() < 0.01, "top flare: {b:?}");
    }
}
