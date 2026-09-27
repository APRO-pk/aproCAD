use crate::vehicle::*;

pub fn validate_component(comp: &Component) -> Vec<Issue> {
    let mut issues = Vec::new();
    match &comp.kind {
        ComponentKind::NoseCone(p) => {
            if p.length <= 0.0 {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Nose cone length must be positive".into(), component: Some(comp.name.clone()) });
            }
            if p.base_radius <= 0.0 {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Nose cone base radius must be positive".into(), component: Some(comp.name.clone()) });
            }
            if p.wall <= 0.0 {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Nose cone wall thickness must be positive".into(), component: Some(comp.name.clone()) });
            }
            if p.wall >= p.base_radius {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Wall thickness must be less than base radius".into(), component: Some(comp.name.clone()) });
            }
        }
        ComponentKind::BodyTube(p) => {
            if p.length <= 0.0 {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Body tube length must be positive".into(), component: Some(comp.name.clone()) });
            }
            if p.radius <= 0.0 {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Body tube radius must be positive".into(), component: Some(comp.name.clone()) });
            }
            if p.wall <= 0.0 || p.wall >= p.radius {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Invalid wall thickness".into(), component: Some(comp.name.clone()) });
            }
        }
        ComponentKind::Nozzle(p) => {
            if p.throat_radius <= 0.0 {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Throat radius must be positive".into(), component: Some(comp.name.clone()) });
            }
            if p.expansion_ratio <= 1.0 {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Expansion ratio must be > 1".into(), component: Some(comp.name.clone()) });
            }
            if p.throat_radius >= p.chamber_radius {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Throat radius must be less than chamber radius".into(), component: Some(comp.name.clone()) });
            }
        }
        ComponentKind::FinSet(p) => {
            if p.count < 2 {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Fin count must be >= 2".into(), component: Some(comp.name.clone()) });
            }
            if p.span <= 0.0 {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Fin span must be positive".into(), component: Some(comp.name.clone()) });
            }
            if p.root_chord <= 0.0 {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Fin root chord must be positive".into(), component: Some(comp.name.clone()) });
            }
            if p.tip_chord <= 0.0 {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Fin tip chord must be positive".into(), component: Some(comp.name.clone()) });
            }
        }
        ComponentKind::Transition(p) => {
            if p.length <= 0.0 {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Transition length must be positive".into(), component: Some(comp.name.clone()) });
            }
            if p.start_radius <= 0.0 || p.end_radius <= 0.0 {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Transition radii must be positive".into(), component: Some(comp.name.clone()) });
            }
            if p.wall <= 0.0 || p.wall >= p.start_radius.max(p.end_radius) {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Invalid transition wall thickness".into(), component: Some(comp.name.clone()) });
            }
        }
        ComponentKind::Tank(p) => {
            if p.radius <= 0.0 {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Tank radius must be positive".into(), component: Some(comp.name.clone()) });
            }
            if p.cylindrical_length < 0.0 {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Tank cylindrical length must be non-negative".into(), component: Some(comp.name.clone()) });
            }
            if p.wall <= 0.0 || p.wall >= p.radius {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Invalid tank wall thickness".into(), component: Some(comp.name.clone()) });
            }
            if let DomeKind::Ellipsoidal { ratio } = &p.dome {
                if *ratio <= 0.0 {
                    issues.push(Issue { severity: IssueSeverity::Error, message: "Ellipsoidal dome ratio must be positive".into(), component: Some(comp.name.clone()) });
                }
            }
        }
        ComponentKind::Solid(ops) => {
            if ops.is_empty() {
                issues.push(Issue { severity: IssueSeverity::Error, message: "Solid must have at least one operation".into(), component: Some(comp.name.clone()) });
            }
            for (i, op) in ops.iter().enumerate() {
                match op {
                    SolidOp::Boolean { target, .. } => {
                        if let SolidRef::Component(name) = target {
                            if name.is_empty() {
                                issues.push(Issue { severity: IssueSeverity::Error, message: format!("Boolean op #{} references empty component name", i), component: Some(comp.name.clone()) });
                            }
                        }
                        if i == 0 {
                            issues.push(Issue { severity: IssueSeverity::Error, message: "Boolean op cannot be the first op (it must follow a solid-producing op)".into(), component: Some(comp.name.clone()) });
                        }
                    }
                    _ => {}
                }
            }
        }
        ComponentKind::Sketch(s) => {
            if s.entities.is_empty() {
                issues.push(Issue { severity: IssueSeverity::Warning, message: "Sketch has no entities".into(), component: Some(comp.name.clone()) });
            }
            for (i, e) in s.entities.iter().enumerate() {
                match e {
                    SketchEntity::Circle { radius, .. } if *radius <= 0.0 => {
                        issues.push(Issue { severity: IssueSeverity::Error, message: format!("Sketch circle #{} radius must be positive", i), component: Some(comp.name.clone()) });
                    }
                    SketchEntity::Arc { radius, .. } if *radius <= 0.0 => {
                        issues.push(Issue { severity: IssueSeverity::Error, message: format!("Sketch arc #{} radius must be positive", i), component: Some(comp.name.clone()) });
                    }
                    SketchEntity::Spline { points, .. } if points.len() < 2 => {
                        issues.push(Issue { severity: IssueSeverity::Error, message: format!("Sketch spline #{} needs at least 2 points", i), component: Some(comp.name.clone()) });
                    }
                    _ => {}
                }
            }
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_comp(ops: Vec<SolidOp>) -> Component {
        Component {
            name: "S".into(),
            material: "Al-6061-T6".into(),
            visible: true,
            transform: Transform::default(),
            color: None,
            kind: ComponentKind::Solid(ops),
        }
    }

    fn block_op() -> SolidOp {
        SolidOp::Extrude {
            profile: Profile::Rectangle { width: 60.0, height: 60.0, corner_radius: None },
            height: 20.0,
            direction: None,
            taper: None,
        }
    }

    fn bool_op() -> SolidOp {
        SolidOp::Boolean { kind: BooleanKind::Difference, target: SolidRef::This }
    }

    #[test]
    fn boolean_first_op_is_error() {
        let issues = validate_component(&solid_comp(vec![bool_op()]));
        assert!(issues.iter().any(|i| i.message.contains("cannot be the first op")));
    }

    #[test]
    fn boolean_after_solid_is_ok() {
        let issues = validate_component(&solid_comp(vec![block_op(), bool_op()]));
        assert!(!issues.iter().any(|i| i.message.contains("cannot be the first op")));
    }

    #[test]
    fn boolean_empty_target_is_error() {
        let issues = validate_component(&solid_comp(vec![block_op(), SolidOp::Boolean {
            kind: BooleanKind::Union,
            target: SolidRef::Component(String::new()),
        }]));
        assert!(issues.iter().any(|i| i.message.contains("empty component name")));
    }

    #[test]
    fn vehicle_parameter_cycle_is_error() {
        let v = crate::vehicle::Vehicle {
            name: "Cyc".into(),
            units: crate::vehicle::Units::Millimeters,
            parameters: Some(vec![
                crate::params::Parameter { name: "a".into(), value: crate::params::Expr::Expression("b + 1".into()) },
                crate::params::Parameter { name: "b".into(), value: crate::params::Expr::Expression("a + 1".into()) },
            ]),
            components: vec![],
        };
        let issues = validate_vehicle(&v);
        assert!(issues.iter().any(|i| i.message.contains("Parameters: circular")));
    }

    #[test]
    fn vehicle_parameter_block_ok() {
        let v = crate::vehicle::Vehicle {
            name: "Ok".into(),
            units: crate::vehicle::Units::Millimeters,
            parameters: Some(vec![
                crate::params::Parameter { name: "od".into(), value: crate::params::Expr::Number(98.0) },
                crate::params::Parameter { name: "id".into(), value: crate::params::Expr::Expression("od - 4".into()) },
            ]),
            components: vec![],
        };
        let issues = validate_vehicle(&v);
        assert!(!issues.iter().any(|i| i.severity == IssueSeverity::Error));
    }
}

/// Mating tolerance (mm) between parts that physically join.
const MATE_TOLERANCE: f64 = 2.0;

/// Whole-document checks: mating radii between parts, tank fit inside body,
/// and parameter block resolution.
pub fn validate_vehicle(vehicle: &Vehicle) -> Vec<Issue> {
    let mut issues = Vec::new();
    if let Some(parameters) = &vehicle.parameters {
        if let Err(e) = crate::params::resolve_parameters(parameters) {
            issues.push(Issue {
                severity: IssueSeverity::Error,
                message: format!("Parameters: {e}"),
                component: None,
            });
        }
    }
    let body = vehicle.components.iter().find_map(|c| match &c.kind {
        ComponentKind::BodyTube(p) => Some((c.name.clone(), p.radius, p.wall)),
        _ => None,
    });
    let Some((body_name, body_radius, body_wall)) = body else {
        return issues;
    };
    for c in &vehicle.components {
        let comp_name = c.name.clone();
        match &c.kind {
            ComponentKind::NoseCone(p) => {
                if (p.base_radius - body_radius).abs() > MATE_TOLERANCE {
                    issues.push(Issue {
                        severity: IssueSeverity::Error,
                        message: format!(
                            "Nose '{}' base_radius {} does not mate with body '{}' radius {}",
                            comp_name, p.base_radius, body_name, body_radius
                        ),
                        component: Some(comp_name),
                    });
                }
            }
            ComponentKind::Nozzle(p) => {
                if (p.chamber_radius - body_radius).abs() > MATE_TOLERANCE {
                    issues.push(Issue {
                        severity: IssueSeverity::Error,
                        message: format!(
                            "Nozzle '{}' chamber_radius {} does not mate with body '{}' radius {}",
                            comp_name, p.chamber_radius, body_name, body_radius
                        ),
                        component: Some(comp_name),
                    });
                }
            }
            ComponentKind::Tank(p) => {
                let body_inner = body_radius - body_wall;
                if p.radius + p.wall > body_inner + MATE_TOLERANCE {
                    issues.push(Issue {
                        severity: IssueSeverity::Warning,
                        message: format!(
                            "Tank '{}' (radius {} + wall {}) may not fit inside body '{}' (inner radius {})",
                            comp_name, p.radius, p.wall, body_name, body_inner
                        ),
                        component: Some(comp_name),
                    });
                }
            }
            ComponentKind::Transition(p) => {
                if (p.start_radius - body_radius).abs() > MATE_TOLERANCE && (p.end_radius - body_radius).abs() > MATE_TOLERANCE {
                    issues.push(Issue {
                        severity: IssueSeverity::Warning,
                        message: format!(
                            "Transition '{}' ({} -> {}) does not mate with body '{}' radius {}",
                            comp_name, p.start_radius, p.end_radius, body_name, body_radius
                        ),
                        component: Some(comp_name),
                    });
                }
            }
            _ => {}
        }
    }
    issues
}
