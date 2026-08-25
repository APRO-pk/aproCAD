//! Builtin corpus: programmatically generated, schema-valid component
//! entries (guaranteed parseable because they are built from the same
//! structs the parser uses).

use apro_document::vehicle::*;

pub struct BuiltinDef {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub component: Component,
}

fn mat(al: &str) -> String { al.to_string() }

fn nose(profile: NoseConeProfile, base_r: f64, len: f64, wall: f64, material: &str, name: &str, desc: &str, tags: &[&str]) -> BuiltinDef {
    BuiltinDef {
        name: name.into(),
        description: desc.into(),
        tags: tags.iter().map(|s| s.to_string()).collect(),
        component: Component {
            name: name.into(),
            material: String::new(),
            visible: true,
            transform: Transform::default(),
            color: None,
            kind: ComponentKind::NoseCone(NoseConeParams {
                profile,
                length: len,
                base_radius: base_r,
                wall,
                material: mat(material),
            }),
        },
    }
}

fn tube(r: f64, len: f64, wall: f64, material: &str, name: &str, desc: &str, tags: &[&str]) -> BuiltinDef {
    BuiltinDef {
        name: name.into(),
        description: desc.into(),
        tags: tags.iter().map(|s| s.to_string()).collect(),
        component: Component {
            name: name.into(),
            material: String::new(),
            visible: true,
            transform: Transform::default(),
            color: None,
            kind: ComponentKind::BodyTube(BodyTubeParams {
                length: len,
                radius: r,
                wall,
                material: mat(material),
            }),
        },
    }
}

fn nozzle(throat_r: f64, er: f64, bell: f64, chamber_r: f64, wall: f64, material: &str, name: &str, desc: &str, tags: &[&str]) -> BuiltinDef {
    BuiltinDef {
        name: name.into(),
        description: desc.into(),
        tags: tags.iter().map(|s| s.to_string()).collect(),
        component: Component {
            name: name.into(),
            material: String::new(),
            visible: true,
            transform: Transform::default(),
            color: None,
            kind: ComponentKind::Nozzle(NozzleParams {
                kind: if bell < 0.5 { NozzleKind::Conical } else { NozzleKind::Bell },
                throat_radius: throat_r,
                expansion_ratio: er,
                percent_bell: bell,
                chamber_radius: chamber_r,
                wall,
                material: mat(material),
            }),
        },
    }
}

fn tank(r: f64, cyl_len: f64, dome: DomeKind, wall: f64, material: &str, name: &str, desc: &str, tags: &[&str]) -> BuiltinDef {
    BuiltinDef {
        name: name.into(),
        description: desc.into(),
        tags: tags.iter().map(|s| s.to_string()).collect(),
        component: Component {
            name: name.into(),
            material: String::new(),
            visible: true,
            transform: Transform::default(),
            color: None,
            kind: ComponentKind::Tank(TankParams {
                radius: r,
                cylindrical_length: cyl_len,
                dome,
                wall,
                material: mat(material),
            }),
        },
    }
}

fn transition(r1: f64, r2: f64, len: f64, wall: f64, material: &str, name: &str, desc: &str, tags: &[&str]) -> BuiltinDef {
    BuiltinDef {
        name: name.into(),
        description: desc.into(),
        tags: tags.iter().map(|s| s.to_string()).collect(),
        component: Component {
            name: name.into(),
            material: String::new(),
            visible: true,
            transform: Transform::default(),
            color: None,
            kind: ComponentKind::Transition(TransitionParams {
                length: len,
                start_radius: r1,
                end_radius: r2,
                wall,
                material: mat(material),
            }),
        },
    }
}

fn fins(count: u32, root: f64, tip: f64, span: f64, sweep: f64, thickness: f64, material: &str, name: &str, desc: &str, tags: &[&str]) -> BuiltinDef {
    BuiltinDef {
        name: name.into(),
        description: desc.into(),
        tags: tags.iter().map(|s| s.to_string()).collect(),
        component: Component {
            name: name.into(),
            material: String::new(),
            visible: true,
            transform: Transform::default(),
            color: None,
            kind: ComponentKind::FinSet(FinSetParams {
                count,
                root_chord: root,
                tip_chord: tip,
                span,
                sweep,
                airfoil: AirfoilParams { family: AirfoilFamily::NACA { digits: "0012".into() } },
                thickness,
                material: mat(material),
            }),
        },
    }
}

fn solid(name: &str, desc: &str, tags: &[&str], ops: Vec<SolidOp>) -> BuiltinDef {
    BuiltinDef {
        name: name.into(),
        description: desc.into(),
        tags: tags.iter().map(|s| s.to_string()).collect(),
        component: Component {
            name: name.into(),
            material: String::new(),
            visible: true,
            transform: Transform::default(),
            color: None,
            kind: ComponentKind::Solid(ops),
        },
    }
}

fn revolve_pts(pts: Vec<[f64; 2]>) -> SolidOp {
    SolidOp::Revolve { profile: Profile::Points(pts), angle: 360.0, axis: None }
}

pub fn builtin_defs() -> Vec<BuiltinDef> {
    let mut out = Vec::new();

    // --- Nose cones: 38 / 54 / 75 / 98 / 150 mm bodies ---
    out.push(nose(NoseConeProfile::VonKarman, 19.0, 120.0, 2.0, "Fiberglass", "VonKarman NC 38mm", "Von Karman nose cone, 38mm body diameter", &["nose cone", "von karman", "38mm", "fiberglass"]));
    out.push(nose(NoseConeProfile::Ogive, 19.0, 110.0, 2.0, "Fiberglass", "Ogive NC 38mm", "Tangent ogive nose cone, 38mm body diameter", &["nose cone", "ogive", "38mm", "fiberglass"]));
    out.push(nose(NoseConeProfile::Conical, 19.0, 90.0, 2.0, "Al-6061-T6", "Conical NC 38mm", "Simple conical nose cone, 38mm body diameter", &["nose cone", "conical", "38mm", "aluminum"]));
    out.push(nose(NoseConeProfile::VonKarman, 27.0, 180.0, 2.0, "Fiberglass", "VonKarman NC 54mm", "Von Karman nose cone, 54mm body diameter", &["nose cone", "von karman", "54mm", "fiberglass"]));
    out.push(nose(NoseConeProfile::Ogive, 27.0, 160.0, 2.0, "Fiberglass", "Ogive NC 54mm", "Tangent ogive nose cone, 54mm body diameter", &["nose cone", "ogive", "54mm", "fiberglass"]));
    out.push(nose(NoseConeProfile::Power { n: 0.75 }, 27.0, 150.0, 2.0, "Fiberglass", "Power NC 54mm", "Power series nose cone n=0.75, 54mm body", &["nose cone", "power series", "54mm"]));
    out.push(nose(NoseConeProfile::VonKarman, 37.5, 250.0, 3.0, "Fiberglass", "VonKarman NC 75mm", "Von Karman nose cone, 75mm body diameter", &["nose cone", "von karman", "75mm", "fiberglass"]));
    out.push(nose(NoseConeProfile::Ogive, 37.5, 230.0, 3.0, "Fiberglass", "Ogive NC 75mm", "Tangent ogive nose cone, 75mm body diameter", &["nose cone", "ogive", "75mm", "fiberglass"]));
    out.push(nose(NoseConeProfile::Haack { c: 0.333 }, 37.5, 240.0, 3.0, "Al-6061-T6", "Haack NC 75mm", "Sears-Haack nose cone, 75mm body diameter", &["nose cone", "haack", "75mm", "aluminum"]));
    out.push(nose(NoseConeProfile::VonKarman, 49.0, 320.0, 3.0, "Fiberglass", "VonKarman NC 98mm", "Von Karman nose cone, 98mm body diameter", &["nose cone", "von karman", "98mm", "fiberglass"]));
    out.push(nose(NoseConeProfile::Ogive, 49.0, 300.0, 3.0, "Fiberglass", "Ogive NC 98mm", "Tangent ogive nose cone, 98mm body diameter", &["nose cone", "ogive", "98mm", "fiberglass"]));
    out.push(nose(NoseConeProfile::VonKarman, 75.0, 500.0, 4.0, "Fiberglass", "VonKarman NC 150mm", "Large Von Karman nose cone, 150mm body diameter", &["nose cone", "von karman", "150mm", "fiberglass"]));
    out.push(nose(NoseConeProfile::Ogive, 75.0, 480.0, 4.0, "Carbon", "Ogive NC 150mm", "Large tangent ogive nose cone, 150mm body diameter", &["nose cone", "ogive", "150mm", "carbon"]));
    out.push(nose(NoseConeProfile::Conical, 100.0, 450.0, 5.0, "Al-6061-T6", "Conical NC 200mm", "Large conical nose cone, 200mm body diameter", &["nose cone", "conical", "200mm", "aluminum"]));

    // --- Body tubes: common airframe diameters ---
    out.push(tube(19.0, 600.0, 1.5, "Fiberglass", "Body Tube 38mm x 600", "Fiberglass airframe tube, 38mm OD", &["body tube", "airframe", "38mm", "fiberglass"]));
    out.push(tube(19.0, 900.0, 1.5, "Fiberglass", "Body Tube 38mm x 900", "Fiberglass airframe tube, 38mm OD, 900mm long", &["body tube", "airframe", "38mm", "fiberglass"]));
    out.push(tube(27.0, 600.0, 1.5, "Fiberglass", "Body Tube 54mm x 600", "Fiberglass airframe tube, 54mm OD", &["body tube", "airframe", "54mm", "fiberglass"]));
    out.push(tube(27.0, 1000.0, 1.5, "Fiberglass", "Body Tube 54mm x 1000", "Fiberglass airframe tube, 54mm OD, 1m long", &["body tube", "airframe", "54mm", "fiberglass"]));
    out.push(tube(37.5, 700.0, 2.0, "Fiberglass", "Body Tube 75mm x 700", "Fiberglass airframe tube, 75mm OD", &["body tube", "airframe", "75mm", "fiberglass"]));
    out.push(tube(37.5, 1200.0, 2.0, "Fiberglass", "Body Tube 75mm x 1200", "Fiberglass airframe tube, 75mm OD, 1.2m long", &["body tube", "airframe", "75mm", "fiberglass"]));
    out.push(tube(49.0, 800.0, 2.5, "Fiberglass", "Body Tube 98mm x 800", "Fiberglass airframe tube, 98mm OD", &["body tube", "airframe", "98mm", "fiberglass"]));
    out.push(tube(49.0, 1500.0, 2.5, "Fiberglass", "Body Tube 98mm x 1500", "Fiberglass airframe tube, 98mm OD, 1.5m long", &["body tube", "airframe", "98mm", "fiberglass"]));
    out.push(tube(75.0, 1000.0, 3.0, "Carbon", "Body Tube 150mm x 1000", "Carbon fiber airframe tube, 150mm OD", &["body tube", "airframe", "150mm", "carbon"]));
    out.push(tube(100.0, 1200.0, 3.0, "Carbon", "Body Tube 200mm x 1200", "Carbon fiber airframe tube, 200mm OD", &["body tube", "airframe", "200mm", "carbon"]));
    out.push(tube(14.0, 500.0, 1.2, "Fiberglass", "Motor Tube 28mm", "Motor mount tube, 28mm OD", &["motor tube", "28mm", "fiberglass"]));
    out.push(tube(27.0, 500.0, 1.5, "Fiberglass", "Motor Tube 54mm", "Motor mount tube, 54mm OD", &["motor tube", "54mm", "fiberglass"]));

    // --- Nozzles ---
    out.push(nozzle(5.0, 6.0, 0.8, 25.0, 3.0, "Graphite", "Graphite Nozzle 10mm throat", "Graphite bell nozzle, 10mm throat, ER 6", &["nozzle", "bell", "graphite", "10mm throat"]));
    out.push(nozzle(7.5, 8.0, 0.8, 35.0, 4.0, "Graphite", "Graphite Nozzle 15mm throat", "Graphite bell nozzle, 15mm throat, ER 8", &["nozzle", "bell", "graphite", "15mm throat"]));
    out.push(nozzle(10.0, 10.0, 0.85, 50.0, 5.0, "Graphite", "Graphite Nozzle 20mm throat", "Large graphite bell nozzle, 20mm throat, ER 10", &["nozzle", "bell", "graphite", "20mm throat"]));
    out.push(nozzle(12.5, 12.0, 0.85, 60.0, 6.0, "Inconel-718", "Inconel Nozzle 25mm throat", "Inconel bell nozzle, 25mm throat, ER 12", &["nozzle", "bell", "inconel", "25mm throat"]));
    out.push(nozzle(5.0, 6.0, 0.0, 25.0, 3.0, "Graphite", "Conical Nozzle 10mm throat", "Simple conical nozzle, 10mm throat, ER 6", &["nozzle", "conical", "graphite", "10mm throat"]));
    out.push(nozzle(10.0, 8.0, 0.0, 40.0, 4.0, "Steel-4130", "Conical Nozzle 20mm throat", "Steel conical nozzle, 20mm throat, ER 8", &["nozzle", "conical", "steel", "20mm throat"]));
    out.push(nozzle(15.0, 15.0, 0.9, 80.0, 8.0, "Inconel-718", "Inconel Nozzle 30mm throat", "High-area-ratio inconel bell nozzle, 30mm throat, ER 15", &["nozzle", "bell", "inconel", "30mm throat", "vacuum"]));

    // --- Tanks ---
    out.push(tank(27.0, 300.0, DomeKind::Ellipsoidal { ratio: 2.0 }, 1.5, "Al-6061-T6", "Lox Tank 54mm", "LOX tank, 54mm OD, 2:1 ellipsoidal domes", &["tank", "lox", "54mm", "aluminum"]));
    out.push(tank(27.0, 300.0, DomeKind::Ellipsoidal { ratio: 2.0 }, 1.5, "Al-6061-T6", "Fuel Tank 54mm", "Fuel tank, 54mm OD, 2:1 ellipsoidal domes", &["tank", "fuel", "54mm", "aluminum"]));
    out.push(tank(37.5, 400.0, DomeKind::Ellipsoidal { ratio: 2.0 }, 2.0, "Al-6061-T6", "Lox Tank 75mm", "LOX tank, 75mm OD, 2:1 ellipsoidal domes", &["tank", "lox", "75mm", "aluminum"]));
    out.push(tank(37.5, 400.0, DomeKind::Ellipsoidal { ratio: 2.0 }, 2.0, "Al-6061-T6", "Fuel Tank 75mm", "Fuel tank, 75mm OD, 2:1 ellipsoidal domes", &["tank", "fuel", "75mm", "aluminum"]));
    out.push(tank(49.0, 500.0, DomeKind::Hemispherical, 2.5, "Al-6061-T6", "Pressurant Tank 98mm", "Helium pressurant tank, 98mm OD, hemispherical domes", &["tank", "pressurant", "helium", "98mm", "aluminum"]));
    out.push(tank(49.0, 600.0, DomeKind::Ellipsoidal { ratio: 2.0 }, 2.5, "Al-6061-T6", "Lox Tank 98mm", "LOX tank, 98mm OD, 2:1 ellipsoidal domes", &["tank", "lox", "98mm", "aluminum"]));
    out.push(tank(49.0, 600.0, DomeKind::Ellipsoidal { ratio: 2.0 }, 2.5, "Al-6061-T6", "Fuel Tank 98mm", "Fuel tank, 98mm OD, 2:1 ellipsoidal domes", &["tank", "fuel", "98mm", "aluminum"]));
    out.push(tank(75.0, 800.0, DomeKind::Ellipsoidal { ratio: 2.0 }, 3.0, "Al-6061-T6", "Lox Tank 150mm", "LOX tank, 150mm OD, 2:1 ellipsoidal domes", &["tank", "lox", "150mm", "aluminum"]));
    out.push(tank(75.0, 800.0, DomeKind::Ellipsoidal { ratio: 2.0 }, 3.0, "Al-6061-T6", "Fuel Tank 150mm", "Fuel tank, 150mm OD, 2:1 ellipsoidal domes", &["tank", "fuel", "150mm", "aluminum"]));

    // --- Transitions ---
    out.push(transition(19.0, 27.0, 60.0, 2.0, "Fiberglass", "Transition 38 to 54", "Fiberglass boat tail transition, 38mm to 54mm", &["transition", "boat tail", "38mm", "54mm", "fiberglass"]));
    out.push(transition(27.0, 37.5, 80.0, 2.0, "Fiberglass", "Transition 54 to 75", "Fiberglass transition, 54mm to 75mm", &["transition", "54mm", "75mm", "fiberglass"]));
    out.push(transition(37.5, 49.0, 100.0, 2.5, "Fiberglass", "Transition 75 to 98", "Fiberglass transition, 75mm to 98mm", &["transition", "75mm", "98mm", "fiberglass"]));
    out.push(transition(49.0, 75.0, 150.0, 3.0, "Carbon", "Transition 98 to 150", "Carbon transition, 98mm to 150mm", &["transition", "98mm", "150mm", "carbon"]));
    out.push(transition(27.0, 19.0, 50.0, 2.0, "Fiberglass", "Boat Tail 54 to 38", "Boat tail, 54mm to 38mm", &["transition", "boat tail", "54mm", "38mm", "fiberglass"]));
    out.push(transition(37.5, 27.0, 60.0, 2.0, "Fiberglass", "Boat Tail 75 to 54", "Boat tail, 75mm to 54mm", &["transition", "boat tail", "75mm", "54mm", "fiberglass"]));

    // --- Fin sets ---
    out.push(fins(3, 90.0, 60.0, 60.0, 25.0, 3.0, "Balsa", "Balsa Fins 3", "Three balsa fins for low-power models", &["fins", "balsa", "3 fin", "low power"]));
    out.push(fins(4, 120.0, 80.0, 75.0, 30.0, 4.0, "Fiberglass", "Glass Fins 4", "Four fiberglass fins, 75mm span", &["fins", "fiberglass", "4 fin", "mid power"]));
    out.push(fins(3, 150.0, 100.0, 100.0, 35.0, 5.0, "Carbon", "Carbon Fins 3", "Three carbon fins, 100mm span, high power", &["fins", "carbon", "3 fin", "high power"]));
    out.push(fins(4, 180.0, 120.0, 120.0, 40.0, 6.0, "Carbon", "Carbon Fins 4", "Four carbon fins, 120mm span, high power", &["fins", "carbon", "4 fin", "high power"]));

    // --- Solids: bell nozzle, tank dome, fairing, combustion chamber ---
    out.push(solid("Bell Nozzle 15mm throat", "Solid-op bell nozzle, 15mm throat, ER 8", &["solid", "nozzle", "bell", "revolve"], vec![
        revolve_pts(vec![
            [0.0, 42.0], [20.0, 42.0], [30.0, 15.0], [60.0, 7.5],
            [160.0, 21.2], [180.0, 24.0],
        ]),
    ]));
    out.push(solid("Tank Dome 150mm", "Ellipsoidal tank dome, 150mm OD, as solid", &["solid", "tank", "dome", "revolve"], vec![
        revolve_pts(vec![
            [0.0, 0.0], [30.0, 37.5], [60.0, 66.0], [75.0, 75.0], [80.0, 75.0],
        ]),
    ]));
    out.push(solid("Fairing Section 75mm", "Aerodynamic fairing section, 75mm base", &["solid", "fairing", "revolve"], vec![
        revolve_pts(vec![
            [0.0, 0.0], [100.0, 28.0], [200.0, 37.5], [220.0, 37.5],
        ]),
    ]));
    out.push(solid("Combustion Chamber 30mm", "Combustion chamber as extruded+revolved solid, 30mm core", &["solid", "chamber", "combustion", "revolve"], vec![
        revolve_pts(vec![
            [0.0, 35.0], [40.0, 35.0], [60.0, 30.0], [60.0, 15.0], [0.0, 15.0],
        ]),
    ]));

    out
}
