use schemars::JsonSchema;
use serde::{Serialize, Deserialize};
use crate::vehicle::*;
use crate::validation::{validate_component, validate_vehicle};

/// Serialize to RON with explicit struct names. RON's default `to_string`
/// omits struct names (`(name: ...)`), which the frontend cannot distinguish
/// from a Component. `struct_names(true)` yields `Vehicle(...)`,
/// `Component(...)`-prefixed, human-readable output that round-trips.
pub fn to_named_ron<T: serde::Serialize>(value: &T) -> Result<String, ron::Error> {
    ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::new().struct_names(true))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum Patch {
    SetProperty { component_name: String, key: String, value: String },
    /// Set (or add) a named parameter. `value` is either a literal number or
    /// an expression string over other parameter names.
    SetParameter { name: String, value: String },
    AddComponent { after_component: Option<String>, component: Component },
    RemoveComponent { component_name: String },
    ReorderComponents { from_index: usize, to_index: usize },
    /// Batch of patches applied atomically: either all succeed or none are applied.
    PatchList { patches: Vec<Patch> },
    /// Agent loop uses this when a todo requires no change to the design.
    Noop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchResult {
    pub vehicle_ron: Option<String>,
    pub issues: Vec<Issue>,
    pub success: bool,
    pub applied: bool,
}

fn set_property_in_kind(kind: &mut ComponentKind, key: &str, value: &str) -> Result<(), String> {
    let pf = |s: &str| s.parse::<f64>().map_err(|_| format!("invalid float: {}", s));
    let pu = |s: &str| s.parse::<u32>().map_err(|_| format!("invalid int: {}", s));
    match kind {
        ComponentKind::NoseCone(p) => match key {
            "length" => { p.length = pf(value)?; Ok(()) }
            "base_radius" => { p.base_radius = pf(value)?; Ok(()) }
            "wall" => { p.wall = pf(value)?; Ok(()) }
            "material" => { p.material = value.into(); Ok(()) }
            "profile" => {
                p.profile = match value {
                    "Conical" => NoseConeProfile::Conical,
                    "Ogive" => NoseConeProfile::Ogive,
                    "VonKarman" => NoseConeProfile::VonKarman,
                    "Haack" | "Haack { c: 0.333 }" => NoseConeProfile::Haack { c: 0.333 },
                    "Power" | "Power { n: 0.75 }" => NoseConeProfile::Power { n: 0.75 },
                    "Parabolic" | "Parabolic { k: 0.5 }" => NoseConeProfile::Parabolic { k: 0.5 },
                    _ => return Err(format!("unknown profile: {}", value)),
                };
                Ok(())
            }
            "c" => { p.profile = NoseConeProfile::Haack { c: pf(value)? }; Ok(()) }
            "n" => { p.profile = NoseConeProfile::Power { n: pf(value)? }; Ok(()) }
            "k" => { p.profile = NoseConeProfile::Parabolic { k: pf(value)? }; Ok(()) }
            _ => Err(format!("unknown key: {}", key)),
        },
        ComponentKind::BodyTube(p) => match key {
            "length" => { p.length = pf(value)?; Ok(()) }
            "radius" => { p.radius = pf(value)?; Ok(()) }
            "wall" => { p.wall = pf(value)?; Ok(()) }
            "material" => { p.material = value.into(); Ok(()) }
            _ => Err(format!("unknown key: {}", key)),
        },
        ComponentKind::Transition(p) => match key {
            "length" => { p.length = pf(value)?; Ok(()) }
            "start_radius" => { p.start_radius = pf(value)?; Ok(()) }
            "end_radius" => { p.end_radius = pf(value)?; Ok(()) }
            "wall" => { p.wall = pf(value)?; Ok(()) }
            "material" => { p.material = value.into(); Ok(()) }
            _ => Err(format!("unknown key: {}", key)),
        },
        ComponentKind::Tank(p) => match key {
            "radius" => { p.radius = pf(value)?; Ok(()) }
            "cylindrical_length" => { p.cylindrical_length = pf(value)?; Ok(()) }
            "wall" => { p.wall = pf(value)?; Ok(()) }
            "material" => { p.material = value.into(); Ok(()) }
            "ratio" => { p.dome = DomeKind::Ellipsoidal { ratio: pf(value)? }; Ok(()) }
            _ => Err(format!("unknown key: {}", key)),
        },
        ComponentKind::Nozzle(p) => match key {
            "throat_radius" => { p.throat_radius = pf(value)?; Ok(()) }
            "expansion_ratio" => { p.expansion_ratio = pf(value)?; Ok(()) }
            "percent_bell" => { p.percent_bell = pf(value)?; Ok(()) }
            "chamber_radius" => { p.chamber_radius = pf(value)?; Ok(()) }
            "wall" => { p.wall = pf(value)?; Ok(()) }
            "material" => { p.material = value.into(); Ok(()) }
            _ => Err(format!("unknown key: {}", key)),
        },
        ComponentKind::FinSet(p) => match key {
            "count" => { p.count = pu(value)?; Ok(()) }
            "root_chord" => { p.root_chord = pf(value)?; Ok(()) }
            "tip_chord" => { p.tip_chord = pf(value)?; Ok(()) }
            "span" => { p.span = pf(value)?; Ok(()) }
            "sweep" => { p.sweep = pf(value)?; Ok(()) }
            "thickness" => { p.thickness = pf(value)?; Ok(()) }
            "material" => { p.material = value.into(); Ok(()) }
            _ => Err(format!("unknown key: {}", key)),
        },
        ComponentKind::Solid(ops) => {
            let parts: Vec<&str> = key.splitn(3, '_').collect();
            if parts.len() < 3 || parts[0] != "op" {
                return Err(format!("invalid Solid op key format: {}", key));
            }
            let idx: usize = parts[1].parse().map_err(|_| format!("invalid op index: {}", parts[1]))?;
            if idx >= ops.len() {
                return Err(format!("op index {} out of range (len {})", idx, ops.len()));
            }
            let field = parts[2];
            let parse_pts = |s: &str| -> Result<Vec<[f64; 2]>, String> {
                let s = s.trim();
                if !s.starts_with('[') || !s.ends_with(']') {
                    return Err(format!("expected point array [(x,y),...], got: {}", s));
                }
                let inner = &s[1..s.len()-1];
                if inner.is_empty() { return Ok(Vec::new()); }
                let items: Vec<&str> = if inner.contains("),(") {
                    inner.split("),(").collect()
                } else if inner.contains("],[") {
                    inner.split("],[").collect()
                } else {
                    vec![inner]
                };
                let mut pts = Vec::new();
                for item in items {
                    let s = item.trim_matches(|c| c == '[' || c == ']' || c == '(' || c == ')' || c == ' ');
                    let coords: Vec<&str> = s.split(',').collect();
                    if coords.len() != 2 {
                        return Err(format!("expected (x,y) or [x,y], got: {}", item));
                    }
                    let x: f64 = coords[0].trim().parse().map_err(|_| format!("invalid x: {}", coords[0]))?;
                    let y: f64 = coords[1].trim().parse().map_err(|_| format!("invalid y: {}", coords[1]))?;
                    pts.push([x, y]);
                }
                Ok(pts)
            };
            let pf = |s: &str| s.parse::<f64>().map_err(|_| format!("invalid float: {}", s));
            match &mut ops[idx] {
                SolidOp::Revolve { profile, angle, axis: _ } => match field {
                    "profile" => { *profile = Profile::Points(parse_pts(value)?); Ok(()) }
                    "angle" => { *angle = pf(value)?; Ok(()) }
                    _ => Err(format!("unknown Revolve field: {}", field)),
                },
                SolidOp::Extrude { profile, height, direction: _, taper: _ } => match field {
                    "profile" => { *profile = Profile::Points(parse_pts(value)?); Ok(()) }
                    "height" => { *height = pf(value)?; Ok(()) }
                    _ => Err(format!("unknown Extrude field: {}", field)),
                },
                SolidOp::Loft { profiles: _, guide_curves: _ } => {
                    if field == "profile" || field.starts_with("profile_") {
                        return Err("Loft profile editing not supported via property table".into());
                    }
                    Err(format!("unknown Loft field: {}", field))
                }
                SolidOp::Sweep { profile: _, path: _, twist: _ } => {
                    Err("Sweep field editing not supported via property table".into())
                }
                SolidOp::RevolveChain { segments: _, angle } => match field {
                    "angle" => { *angle = pf(value)?; Ok(()) }
                    _ => Err("RevolveChain segment editing not supported via property table".into()),
                },
                _ => Err("Editing of this op not supported via property table".into()),
            }
        }
        ComponentKind::Sketch(s) => match key {
            "plane" => {
                s.plane = match value.trim() {
                    "XY" | "xy" => SketchPlane::XY,
                    "XZ" | "xz" => SketchPlane::XZ,
                    "YZ" | "yz" => SketchPlane::YZ,
                    other => return Err(format!("unknown sketch plane: {}", other)),
                };
                Ok(())
            }
            "offset" => { s.offset = pf(value)?; Ok(()) }
            _ => Err("Sketch entities are edited in the RON editor, not the property table".into()),
        },
    }
}

fn set_component_property(comp: &mut Component, key: &str, value: &str) -> Result<(), String> {
    let pf = |s: &str| s.parse::<f64>().map_err(|_| format!("invalid float: {}", s));
    match key {
        "pos_x" => { comp.transform.position.0 = pf(value)?; Ok(()) }
        "pos_y" => { comp.transform.position.1 = pf(value)?; Ok(()) }
        "pos_z" => { comp.transform.position.2 = pf(value)?; Ok(()) }
        "rot_x_deg" => { comp.transform.rotation.0 = pf(value)?; Ok(()) }
        "rot_y_deg" => { comp.transform.rotation.1 = pf(value)?; Ok(()) }
        "rot_z_deg" => { comp.transform.rotation.2 = pf(value)?; Ok(()) }
        "scale_x" => {
            let s = comp.transform.scale.unwrap_or((1.0, 1.0, 1.0));
            comp.transform.scale = Some((pf(value)?, s.1, s.2)); Ok(())
        }
        "scale_y" => {
            let s = comp.transform.scale.unwrap_or((1.0, 1.0, 1.0));
            comp.transform.scale = Some((s.0, pf(value)?, s.2)); Ok(())
        }
        "scale_z" => {
            let s = comp.transform.scale.unwrap_or((1.0, 1.0, 1.0));
            comp.transform.scale = Some((s.0, s.1, pf(value)?)); Ok(())
        }
        "material" => { comp.material = value.into(); Ok(()) }
        "color" => {
            let trimmed = value.trim();
            comp.color = if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") || trimmed.eq_ignore_ascii_case("auto") {
                None
            } else {
                Some(trimmed.to_string())
            };
            Ok(())
        }
        "visible" => { comp.visible = value.parse::<bool>().map_err(|_| format!("invalid bool: {}", value))?; Ok(()) }
        _ => Err(format!("unknown component property: {}", key)),
    }
}

pub fn apply_patch(vehicle: &mut Vehicle, patch: &Patch) -> PatchResult {
    let mut issues = Vec::new();

    match patch {
        Patch::SetProperty { component_name, key, value } => {
            let comp = match vehicle.components.iter_mut().find(|c| c.name == *component_name) {
                Some(c) => c,
                None => {
                    issues.push(Issue {
                        severity: IssueSeverity::Error,
                        message: format!("Component '{}' not found", component_name),
                        component: None,
                    });
                    return PatchResult { vehicle_ron: None, issues, success: false, applied: false };
                }
            };
            // Try component-level properties first (transform, material, visible)
            if let Err(_) = set_component_property(comp, key, value) {
                // Fall through to kind-specific property
                if let Err(e) = set_property_in_kind(&mut comp.kind, key, value) {
                    issues.push(Issue {
                        severity: IssueSeverity::Error,
                        message: e,
                        component: Some(component_name.clone()),
                    });
                    return PatchResult { vehicle_ron: None, issues, success: false, applied: false };
                }
            }
            issues.extend(validate_component(comp));
        }
        Patch::SetParameter { name, value } => {
            let expr = match value.trim().parse::<f64>() {
                Ok(n) => crate::params::Expr::Number(n),
                Err(_) => crate::params::Expr::Expression(value.trim().to_string()),
            };
            let parameters = vehicle.parameters.get_or_insert_with(Vec::new);
            match parameters.iter_mut().find(|p| p.name == *name) {
                Some(p) => p.value = expr,
                None => parameters.push(crate::params::Parameter { name: name.clone(), value: expr }),
            }
        }
        Patch::AddComponent { after_component, component } => {
            let idx = after_component.as_ref().and_then(|name| {
                vehicle.components.iter().position(|c| c.name == *name)
            }).map(|i| i + 1).unwrap_or(vehicle.components.len());
            issues.extend(validate_component(component));
            if !issues.iter().any(|i| i.severity == IssueSeverity::Error) {
                vehicle.components.insert(idx, component.clone());
            } else {
                return PatchResult { vehicle_ron: None, issues, success: false, applied: false };
            }
        }
        Patch::RemoveComponent { component_name } => {
            let pos = vehicle.components.iter().position(|c| c.name == *component_name);
            match pos {
                Some(i) => { vehicle.components.remove(i); }
                None => {
                    issues.push(Issue {
                        severity: IssueSeverity::Error,
                        message: format!("Component '{}' not found", component_name),
                        component: None,
                    });
                    return PatchResult { vehicle_ron: None, issues, success: false, applied: false };
                }
            }
        }
        Patch::ReorderComponents { from_index, to_index } => {
            if *from_index >= vehicle.components.len() || *to_index >= vehicle.components.len() {
                issues.push(Issue {
                    severity: IssueSeverity::Error,
                    message: format!("Invalid indices: from={}, to={}, len={}", from_index, to_index, vehicle.components.len()),
                    component: None,
                });
                return PatchResult { vehicle_ron: None, issues, success: false, applied: false };
            }
            let c = vehicle.components.remove(*from_index);
            vehicle.components.insert(*to_index, c);
        }
        Patch::Noop => {}
        Patch::PatchList { patches } => {
            // All-or-nothing: apply to a clone so a partial failure leaves the
            // vehicle untouched.
            let mut candidate = vehicle.clone();
            let mut all_issues = Vec::new();
            for p in patches {
                let r = apply_patch(&mut candidate, p);
                all_issues.extend(r.issues);
                if !r.success {
                    return PatchResult { vehicle_ron: None, issues: all_issues, success: false, applied: false };
                }
            }
            issues = all_issues;
            *vehicle = candidate;
        }
    }

    issues.extend(validate_vehicle(vehicle));

    let has_errors = issues.iter().any(|i| i.severity == IssueSeverity::Error);
    let applied = !has_errors;
    let vehicle_ron = if applied {
        to_named_ron(vehicle).ok()
    } else {
        None
    };

    PatchResult { vehicle_ron, issues, success: !has_errors, applied }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_vehicle() -> Vehicle {
        Vehicle { parameters: None,
            name: "Test".into(),
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
                        length: 500.0, radius: 50.0, wall: 2.0,
                        material: "Al-6061-T6".into(),
                    }),
                },
            ],
        }
    }

    #[test]
    fn test_set_property_success() {
        let mut v = make_test_vehicle();
        let result = apply_patch(&mut v, &Patch::SetProperty {
            component_name: "Nose".into(),
            key: "length".into(),
            value: "300.0".into(),
        });
        assert!(result.success);
        assert!(result.applied);
        assert!(result.vehicle_ron.is_some());
        let restored: Vehicle = ron::from_str(&result.vehicle_ron.unwrap()).unwrap();
        match &restored.components[0].kind {
            ComponentKind::NoseCone(p) => assert!((p.length - 300.0).abs() < 1e-6),
            _ => panic!(),
        }
    }

    #[test]
    fn test_set_property_unknown_component() {
        let mut v = make_test_vehicle();
        let result = apply_patch(&mut v, &Patch::SetProperty {
            component_name: "Ghost".into(),
            key: "length".into(),
            value: "100.0".into(),
        });
        assert!(!result.success);
        assert!(!result.applied);
    }

    #[test]
    fn test_add_component() {
        let mut v = make_test_vehicle();
        let new_comp = Component {
            name: "Tail".into(),
            material: "Al-6061-T6".into(),
            visible: true,
            transform: Transform { position: (0.0, 0.0, 700.0), rotation: (0.0, 0.0, 0.0), scale: None },
            color: None,
            kind: ComponentKind::Transition(TransitionParams {
                length: 100.0, start_radius: 50.0, end_radius: 30.0, wall: 2.0,
                material: "Al-6061-T6".into(),
            }),
        };
        let result = apply_patch(&mut v, &Patch::AddComponent {
            after_component: Some("Body".into()),
            component: new_comp,
        });
        assert!(result.success);
        assert_eq!(v.components.len(), 3);
        assert_eq!(v.components[2].name, "Tail");
    }

    #[test]
    fn test_remove_component() {
        let mut v = make_test_vehicle();
        let result = apply_patch(&mut v, &Patch::RemoveComponent {
            component_name: "Nose".into(),
        });
        assert!(result.success);
        assert_eq!(v.components.len(), 1);
        assert_eq!(v.components[0].name, "Body");
    }

    #[test]
    fn test_reorder_components() {
        let mut v = make_test_vehicle();
        let result = apply_patch(&mut v, &Patch::ReorderComponents {
            from_index: 0, to_index: 1,
        });
        assert!(result.success);
        assert_eq!(v.components[0].name, "Body");
        assert_eq!(v.components[1].name, "Nose");
    }

    #[test]
    fn test_set_parameter_number_and_expression() {
        let mut v = make_test_vehicle();
        let r1 = apply_patch(&mut v, &Patch::SetParameter { name: "body_od".into(), value: "98.0".into() });
        assert!(r1.success);
        assert!(apply_patch(&mut v, &Patch::SetParameter { name: "wall".into(), value: "2.0".into() }).success);
        let r2 = apply_patch(&mut v, &Patch::SetParameter { name: "body_id".into(), value: "body_od - 2 * wall".into() });
        assert!(r2.success);
        let ps = v.parameters.as_ref().unwrap();
        assert_eq!(ps.len(), 3);
        assert_eq!(ps[0].value.as_number(), Some(98.0));
        assert_eq!(ps[2].value.as_expr(), Some("body_od - 2 * wall"));
        // The document round-trips with the parameters intact.
        let ron_out = r2.vehicle_ron.as_ref().unwrap();
        let back: Vehicle = ron::from_str(ron_out).unwrap();
        assert_eq!(back.parameters.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn test_set_color_and_roundtrip() {
        let mut v = make_test_vehicle();
        let r1 = apply_patch(&mut v, &Patch::SetProperty {
            component_name: "Nose".into(), key: "color".into(), value: "#ff8800".into(),
        });
        assert!(r1.success);
        assert_eq!(v.components[0].color.as_deref(), Some("#ff8800"));
        // "auto"/empty clears back to None
        let r2 = apply_patch(&mut v, &Patch::SetProperty {
            component_name: "Nose".into(), key: "color".into(), value: "auto".into(),
        });
        assert!(r2.success);
        assert_eq!(v.components[0].color, None);
        // The explicit color survives a RON round-trip.
        let r3 = apply_patch(&mut v, &Patch::SetProperty {
            component_name: "Nose".into(), key: "color".into(), value: "red".into(),
        });
        let restored: Vehicle = ron::from_str(&r3.vehicle_ron.unwrap()).unwrap();
        assert_eq!(restored.components[0].color.as_deref(), Some("red"));
        // A vehicle without colors still parses (serde default).
        let plain = ron::from_str::<Vehicle>(
            r#"Vehicle(name: "p", units: Millimeters, components: [])"#
        ).unwrap();
        assert!(plain.components.is_empty());
    }

    #[test]
    fn test_set_parameter_cycle_rejected() {
        let mut v = make_test_vehicle();
        assert!(apply_patch(&mut v, &Patch::SetParameter { name: "a".into(), value: "1.0".into() }).success);
        assert!(apply_patch(&mut v, &Patch::SetParameter { name: "b".into(), value: "a + 1".into() }).success);
        let result = apply_patch(&mut v, &Patch::SetParameter { name: "a".into(), value: "b + 1".into() });
        assert!(!result.success, "cycle must be rejected");
        assert!(result.issues.iter().any(|i| i.message.contains("circular")));
    }

    /// Phase 3: AI patches arrive as RON text (local GBNF) or JSON converted
    /// to RON. All four variants must roundtrip through ron and apply cleanly.
    #[test]
    fn test_patch_ron_roundtrip_all_variants() {
        let cases: Vec<(&str, fn(&mut Vehicle))> = vec![
            (
                r#"SetProperty(component_name: "Nose", key: "length", value: "300.0")"#,
                |v: &mut Vehicle| match &v.components[0].kind {
                    ComponentKind::NoseCone(p) => assert!((p.length - 300.0).abs() < 1e-6),
                    _ => panic!(),
                },
            ),
            (
                r#"AddComponent(after_component: Some("Body"), component: Component(
                        name: "Tail", material: "G10-FR4", visible: true,
                        transform: Transform(position: (0.0, 0.0, 700.0), rotation: (0.0, 0.0, 0.0)),
                        kind: Transition(TransitionParams(length: 100.0, start_radius: 50.0, end_radius: 30.0, wall: 2.0, material: "G10-FR4")),
                    ))"#,
                |v: &mut Vehicle| {
                    assert_eq!(v.components.len(), 3);
                    assert_eq!(v.components[2].name, "Tail");
                },
            ),
            (
                r#"RemoveComponent(component_name: "Nose")"#,
                |v: &mut Vehicle| {
                    assert_eq!(v.components.len(), 1);
                    assert_eq!(v.components[0].name, "Body");
                },
            ),
            (
                r#"ReorderComponents(from_index: 0, to_index: 1)"#,
                |v: &mut Vehicle| assert_eq!(v.components[0].name, "Body"),
            ),
            (
                r#"Noop"#,
                |v: &mut Vehicle| {
                    assert_eq!(v.components.len(), 2);
                    assert_eq!(v.components[0].name, "Nose");
                },
            ),
            (
                r#"PatchList(patches: [
                    SetProperty(component_name: "Nose", key: "length", value: "320.0"),
                    SetProperty(component_name: "Nose", key: "wall", value: "4.0"),
                    SetProperty(component_name: "Body", key: "length", value: "600.0"),
                ])"#,
                |v: &mut Vehicle| {
                    match &v.components[0].kind {
                        ComponentKind::NoseCone(p) => {
                            assert!((p.length - 320.0).abs() < 1e-6);
                            assert!((p.wall - 4.0).abs() < 1e-6);
                        }
                        _ => panic!(),
                    }
                    match &v.components[1].kind {
                        ComponentKind::BodyTube(p) => assert!((p.length - 600.0).abs() < 1e-6),
                        _ => panic!(),
                    }
                },
            ),
        ];
        for (ron_str, check) in cases {
            let patch: Patch = ron::from_str(ron_str).expect("patch RON must parse");
            // JSON -> typed -> RON (the cloud path) must also work.
            let json = serde_json::to_string(&patch).unwrap();
            let from_json: Patch = serde_json::from_str(&json).unwrap();
            let reparsed: Patch = ron::from_str(&ron::to_string(&from_json).unwrap()).unwrap();
            assert_eq!(patch, reparsed);
            let mut v = make_test_vehicle();
            let result = apply_patch(&mut v, &reparsed);
            assert!(result.success, "patch {ron_str} failed: {:?}", result.issues);
            check(&mut v);
        }
    }

    /// PatchList must be all-or-nothing: a failing sub-patch leaves the vehicle untouched.
    #[test]
    fn test_patch_list_atomic_failure() {
        let patch: Patch = ron::from_str(
            r#"PatchList(patches: [
                SetProperty(component_name: "Nose", key: "length", value: "320.0"),
                SetProperty(component_name: "Ghost", key: "length", value: "1.0"),
            ])"#,
        ).unwrap();
        let mut v = make_test_vehicle();
        let result = apply_patch(&mut v, &patch);
        assert!(!result.success);
        assert!(!result.applied);
        assert!(result.vehicle_ron.is_none());
        // Nose untouched: length must still be 200.0.
        match &v.components[0].kind {
            ComponentKind::NoseCone(p) => assert!((p.length - 200.0).abs() < 1e-6),
            _ => panic!(),
        }
    }

    /// A NoseCone whose base_radius does not mate with the body must be rejected.
    #[test]
    fn test_mating_rule_rejects_mismatched_nose() {
        let mut v = make_test_vehicle();
        let result = apply_patch(&mut v, &Patch::SetProperty {
            component_name: "Nose".into(),
            key: "base_radius".into(),
            value: "100.0".into(),
        });
        assert!(!result.success);
        assert!(!result.applied);
        let msg = result.issues.iter().map(|i| i.message.as_str()).collect::<Vec<_>>().join("; ");
        assert!(msg.contains("does not mate"), "got: {msg}");
    }

    /// Tank that fits inside the body must pass with only warnings allowed.
    #[test]
    fn test_mating_tank_fit_warning_only() {
        let mut v = make_test_vehicle();
        let result = apply_patch(&mut v, &Patch::AddComponent {
            after_component: Some("Body".into()),
            component: Component {
                name: "Tank-1".into(),
                material: "Al-6061-T6".into(),
                visible: true,
                transform: Transform::default(),
                color: None,
                kind: ComponentKind::Tank(TankParams {
                    radius: 40.0, cylindrical_length: 100.0, dome: DomeKind::Hemispherical,
                    wall: 2.0, material: "Al-6061-T6".into(),
                }),
            },
        });
        assert!(result.success, "tank fit failed: {:?}", result.issues);
    }

    /// Unknown component name must fail cleanly via the RON path too.
    #[test]
    fn test_patch_ron_unknown_component() {
        let patch: Patch = ron::from_str(
            r#"SetProperty(component_name: "Ghost", key: "length", value: "100.0")"#,
        ).unwrap();
        let mut v = make_test_vehicle();
        let result = apply_patch(&mut v, &patch);
        assert!(!result.success);
        assert!(!result.applied);
        assert!(result.vehicle_ron.is_none());
    }
}
