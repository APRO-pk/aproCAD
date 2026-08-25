use apro_document::params::{ParamEnv, resolve_parameters};
use apro_document::vehicle::{Component, ComponentKind, Vehicle};
use apro_kernel::{transform_mesh_srt, MeshData};
use crate::eval::evaluate_solid_ops_with;
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
/// resolve through `resolve`; profile expressions evaluate against `params`.
pub fn try_build_component_mesh(
    _vehicle: &Vehicle,
    comp: &Component,
    params: &ParamEnv,
    resolve: &mut dyn FnMut(&str) -> Result<MeshData, String>,
) -> Result<MeshData, String> {
    let mut eval = |ops: &[apro_document::vehicle::SolidOp]| -> Result<MeshData, String> {
        let mut resolver = |name: &str| -> Result<MeshData, String> {
            resolve(name)
        };
        evaluate_solid_ops_with(ops, params, &mut resolver)
    };
    match &comp.kind {
        ComponentKind::NoseCone(p) => eval(&nosecone_to_ops(p)),
        ComponentKind::BodyTube(p) => eval(&bodytube_to_ops(p)),
        ComponentKind::Transition(p) => eval(&transition_to_ops(p)),
        ComponentKind::Tank(p) => eval(&tank_to_ops(p)),
        ComponentKind::Nozzle(p) => eval(&nozzle_to_ops(p)),
        ComponentKind::FinSet(p) => Ok(crate::fin::mesh_finset(p)),
        ComponentKind::Solid(ops) => eval(ops),
    }
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
    use super::build_vehicle_mesh;

    #[test]
    fn test_single_component_vehicle() {
        let v = Vehicle { parameters: None,
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
        let v = Vehicle { parameters: None,
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
        let v = Vehicle { parameters: None,
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
        let v = Vehicle { parameters: None,
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
        let v = Vehicle { parameters: None,
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
        let v = Vehicle { parameters: None,
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
}
