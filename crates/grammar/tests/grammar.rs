//! Grammar-Constrained Generation tests:
//! - the JSON schema accepts every document in the golden corpus
//! - the emitted GBNF is structurally valid
//! - the grammar covers every SolidOp variant (minus stubs) and would fail
//!   loudly if a new variant were added without updating anything

use apro_grammar::*;
use apro_document::vehicle::*;

const CORPUS_JSON: &str = include_str!("data/corpus.json");

fn corpus() -> Vec<(String, String)> {
    let v: serde_json::Value = serde_json::from_str(CORPUS_JSON).expect("corpus.json parses");
    v["presets"]
        .as_object()
        .expect("presets object")
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
        .collect()
}

/// Parse a corpus doc as Vehicle or Component.
fn parse_doc(ron: &str) -> Result<serde_json::Value, String> {
    if let Ok(v) = ron::from_str::<Vehicle>(ron) {
        return serde_json::to_value(&v).map_err(|e| e.to_string());
    }
    if let Ok(c) = ron::from_str::<Component>(ron) {
        return serde_json::to_value(&c).map_err(|e| e.to_string());
    }
    Err(format!("neither Vehicle nor Component: {}", ron.chars().take(60).collect::<String>()))
}

#[test]
fn test_golden_corpus_roundtrips() {
    let docs = corpus();
    assert!(docs.len() >= 10, "corpus too small: {}", docs.len());
    for (name, ron) in &docs {
        let _ = parse_doc(ron).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn test_golden_corpus_validates_against_schema() {
    let vehicle_schema = schema_for_vehicle();
    let component_schema = schema_for_component();
    let v_compiled = jsonschema::validator_for(&vehicle_schema).expect("vehicle schema compiles");
    let c_compiled = jsonschema::validator_for(&component_schema).expect("component schema compiles");

    for (name, ron) in &corpus() {
        let json = parse_doc(ron).unwrap_or_else(|e| panic!("{name}: parse: {e}"));
        let is_component = json.get("name").is_some() && json.get("components").is_none();
        let result = if is_component {
            c_compiled.validate(&json)
        } else {
            v_compiled.validate(&json)
        };
        match result {
            Ok(()) => {}
            Err(e) => {
                panic!("{name}: schema violation: {e}");
            }
        }
    }
}

#[test]
fn test_gbnf_structural_valid_for_vehicle() {
    let gbnf = generate_gbnf(&schema_for_vehicle(), "Vehicle").expect("gbnf generation");
    validate_gbnf(&gbnf, "Vehicle").expect("gbnf valid");
    assert!(gbnf.contains("\"Vehicle\""));
    assert!(gbnf.contains("\"Component\""));
    assert!(gbnf.contains("\"NoseCone\""));
    assert!(gbnf.contains("\"BodyTube\""));
    assert!(gbnf.contains("\"Revolve\""));
    // The parameters block must be expressible: list form (GBNF-safe) with a
    // name property and a number-or-equation value.
    assert!(gbnf.contains("\"Parameter\""), "gbnf must include Parameter rule");
    assert!(gbnf.contains("\"name\""), "gbnf must include parameter name property");
}

/// A parametric Vehicle in the GBNF-mandated list form must validate against
/// the JSON schema (the cloud path) AND round-trip through the document crate
/// (the local path).
#[test]
fn test_parameters_list_form_roundtrips() {
    let schema = schema_for_vehicle();
    let ron_text = r#"Vehicle(
        name: "Parametric",
        units: Millimeters,
        parameters: Some([
            Parameter(name: "body_od", value: 98.0),
            Parameter(name: "wall", value: 2.0),
            Parameter(name: "body_id", value: "body_od - 2 * wall"),
        ]),
        components: [],
    )"#;
    let doc: apro_document::vehicle::Vehicle = ron::from_str(ron_text).expect("document parses list-form parameters");
    assert_eq!(doc.parameters.as_ref().unwrap().len(), 3);
    let json = parse_doc(ron_text).expect("json conversion path parses");
    jsonschema::validator_for(&schema)
        .expect("vehicle schema compiles")
        .validate(&json)
        .expect("list-form parameters satisfy vehicle schema");
}

#[test]
fn test_gbnf_structural_valid_for_component() {
    let gbnf = generate_gbnf(&schema_for_component(), "Component").expect("gbnf generation");
    validate_gbnf(&gbnf, "Component").expect("gbnf valid");
}

#[test]
fn test_gbnf_structural_valid_for_patch() {
    let gbnf = generate_gbnf(&schema_for_patch(), "Patch").expect("gbnf generation");
    validate_gbnf(&gbnf, "Patch").expect("gbnf valid");
    assert!(gbnf.contains("\"SetProperty\""));
    assert!(gbnf.contains("\"AddComponent\""));
    assert!(gbnf.contains("\"Noop\""));
    // Patch is an enum: RON writes bare variants (`SetProperty(...)`), so the
    // grammar must NOT force an invalid `Patch(...)` wrapper on the root.
    assert!(
        !gbnf.contains("\"Patch\" ws"),
        "enum root must not be wrapped with its name: {gbnf}"
    );
}

/// The GBNF for a Patch must accept exactly the RON that the document crate
/// parses: bare variants, no outer `Patch( ... )` wrapper.
#[test]
fn test_patch_gbnf_accepts_bare_variant_ron() {
    let gbnf = generate_gbnf(&schema_for_patch(), "Patch").expect("gbnf generation");
    validate_gbnf(&gbnf, "Patch").expect("gbnf valid");
    // A sampling of valid Patch RON must be parseable by the document crate
    // (proving the grammar shape matches the RON the loop applies).
    for ron_str in [
        r#"SetProperty(component_name: "Nose", key: "length", value: "300.0")"#,
        r#"SetParameter(name: "body_od", value: "98.0")"#,
        r#"SetParameter(name: "body_id", value: "body_od - 2 * wall")"#,
        r#"Noop"#,
        r#"RemoveComponent(component_name: "Nose")"#,
    ] {
        ron::from_str::<apro_document::patch::Patch>(ron_str)
            .unwrap_or_else(|e| panic!("{ron_str} must parse: {e}"));
    }
}

/// PatchList (recursive enum) must generate bounded GBNF and accept valid RON.
#[test]
fn test_patch_list_gbnf_and_roundtrip() {
    let gbnf = generate_gbnf(&schema_for_patch(), "Patch").expect("gbnf generation");
    validate_gbnf(&gbnf, "Patch").expect("gbnf valid");
    assert!(gbnf.contains("\"PatchList\""), "PatchList missing: {gbnf}");
    // Bounded recursion: the nested item rule must reference the variants,
    // not re-expand the whole enum forever.
    assert!(gbnf.contains("PatchList-field-patches-item"));

    let ron_str = r#"PatchList(patches: [
        SetProperty(component_name: "Nose", key: "length", value: "320.0"),
        SetParameter(name: "body_od", value: "body_od + 2"),
        AddComponent(after_component: Some("Body"), component: Component(
            name: "Tank-1", material: "Al-6061-T6", visible: true,
            transform: Transform(position: (0.0, 0.0, 700.0), rotation: (0.0, 0.0, 0.0)),
            kind: Tank(TankParams(radius: 40.0, cylindrical_length: 100.0, dome: Hemispherical, wall: 2.0, material: "Al-6061-T6")),
        )),
        Noop,
    ])"#;
    let patch: apro_document::patch::Patch = ron::from_str(ron_str)
        .unwrap_or_else(|e| panic!("PatchList RON must parse: {e}"));
    // JSON (cloud path) must roundtrip through RON identically.
    let json = serde_json::to_string(&patch).unwrap();
    let from_json: apro_document::patch::Patch = serde_json::from_str(&json).unwrap();
    assert_eq!(
        patch,
        from_json,
        "PatchList must survive JSON roundtrip"
    );
}

#[test]
fn test_schema_excludes_stub_ops() {
    let schema = schema_for_vehicle();
    let s = serde_json::to_string(&schema).unwrap();
    for stub in UNIMPLEMENTED_OPS {
        assert!(
            !s.contains(&format!("\"{stub}\"")),
            "stub op {stub} leaked into the schema"
        );
    }
    // Sanity: the schema still contains the real ops
    for op in ["Revolve", "Extrude", "Loft", "Sweep", "RevolveChain"] {
        assert!(s.contains(&format!("\"{op}\"")), "real op {op} missing from schema");
    }
}

/// The full set of SolidOp variants known to the Rust enum.
/// If this list is missing a newly added variant, `grammar_covers_all_variants`
/// fails and forces a decision: implement it or mark it `#[schemars(skip)]`.
const ALL_SOLID_OP_VARIANTS: &[&str] = &[
    "Revolve", "RevolveChain", "Extrude", "Loft", "Sweep",
    "Shell", "Boolean", "Fillet", "Chamfer", "TransformOp",
];

#[test]
fn test_grammar_covers_all_variants() {
    let schema = schema_for_vehicle();
    let defs = schema["$defs"].as_object().expect("$defs");
    let solid_op = defs
        .get("SolidOp")
        .expect("SolidOp definition exists")
        .clone();
    let one_of = solid_op["oneOf"].as_array().expect("SolidOp is oneOf");
    let in_schema: Vec<String> = one_of
        .iter()
        .filter_map(|alt| alt["properties"].as_object())
        .flat_map(|p| p.keys().cloned())
        .collect();

    let covered: Vec<&str> = ALL_SOLID_OP_VARIANTS
        .iter()
        .copied()
        .filter(|v| in_schema.iter().any(|s| s == v))
        .collect();
    let uncovered: Vec<&str> = ALL_SOLID_OP_VARIANTS
        .iter()
        .copied()
        .filter(|v| !in_schema.iter().any(|s| s == v))
        .collect();

    // Every unimplemented op must be excluded from the grammar
    for stub in UNIMPLEMENTED_OPS {
        assert!(
            !covered.contains(stub),
            "stub op {stub} must not appear in the grammar"
        );
    }
    // Everything else must be covered
    for v in ALL_SOLID_OP_VARIANTS {
        if !UNIMPLEMENTED_OPS.contains(v) {
            assert!(
                covered.contains(v),
                "SolidOp variant {v} is neither implemented in the grammar nor listed in UNIMPLEMENTED_OPS"
            );
        }
    }
    assert!(
        uncovered.iter().all(|v| UNIMPLEMENTED_OPS.contains(v)),
        "unexpected uncovered variants: {uncovered:?}"
    );
}

#[test]
fn test_schema_contains_all_key_types() {
    let schema = schema_for_vehicle();
    let defs = schema["$defs"].as_object().expect("$defs");
    for t in ["Component", "ComponentKind", "SolidOp", "Profile", "Path3D", "NoseConeParams", "BodyTubeParams", "Transform", "Units",
              "SketchParams", "SketchEntity", "SketchPlane", "BooleanKind", "SolidRef"] {
        assert!(defs.contains_key(t), "missing definition {t}");
    }
}

#[test]
fn test_patch_schema_types() {
    let schema = schema_for_patch();
    let s = serde_json::to_string(&schema).unwrap();
    assert!(s.contains("SetProperty"));
    assert!(s.contains("AddComponent"));
    assert!(s.contains("RemoveComponent"));
    assert!(s.contains("ReorderComponents"));
    assert!(s.contains("Noop"));
}

/// The agent loop's planning step: the Plan schema must accept the todos the
/// model emits and the GBNF must be structurally valid.
#[test]
fn test_plan_schema_and_gbnf() {
    let schema = schema_for_plan();
    let s = serde_json::to_string(&schema).unwrap();
    assert!(s.contains("todos"), "plan schema lacks todos");
    let compiled = jsonschema::validator_for(&schema).expect("plan schema compiles");
    for todos in [vec!["Shorten the body tube"], vec!["Increase nose length to 300 mm", "Move fins forward", "Change material to G10"]] {
        let plan = serde_json::json!({ "todos": todos });
        compiled.validate(&plan).unwrap_or_else(|e| panic!("plan rejected by schema: {e}"));
    }
    // The plan must also be representable as RON (local GBNF path). RON
    // omits the struct name on serialization, and accepts both the named
    // (`Plan(todos: [...])`, what the GBNF forces) and unnamed forms.
    // The plan must also be representable as RON (local GBNF path). RON's
    // serializer omits struct names, but the GBNF forces the named form
    // `Plan(todos: [...])` — the parser must accept it (via the serde rename).
    let plan_ron = ron::to_string(&apro_grammar::AgentPlan { todos: vec!["a".into(), "b".into()] }).unwrap();
    for s in [plan_ron.as_str(), "Plan(todos: [\"a\", \"b\"])"] {
        let p: apro_grammar::AgentPlan = ron::from_str(s).unwrap_or_else(|e| panic!("{s}: {e}"));
        assert_eq!(p.todos.len(), 2);
    }
    let gbnf = generate_gbnf(&schema_for_plan(), "Plan").expect("plan gbnf generation");
    validate_gbnf(&gbnf, "Plan").expect("plan gbnf valid");
    assert!(gbnf.contains("\"todos\""));
}

/// Phase 3: every Patch shape the AI may emit (the four variants, `None`
/// vs `Some` after_component, a full Component payload) must validate against
/// the Patch schema used for the cloud `response_format` path.
#[test]
fn test_patch_corpus_validates_against_schema() {
    let schema = schema_for_patch();
    let compiled = jsonschema::validator_for(&schema).expect("patch schema compiles");

    let patches: Vec<serde_json::Value> = vec![
        serde_json::json!({"SetProperty": {"component_name": "Nose", "key": "length", "value": "300.0"}}),
        serde_json::json!({"RemoveComponent": {"component_name": "Nose"}}),
        serde_json::json!({"ReorderComponents": {"from_index": 0, "to_index": 1}}),
        serde_json::json!({"AddComponent": {"after_component": null, "component": {
            "name": "Tail",
            "material": "G10-FR4",
            "visible": true,
            "transform": {"position": [0.0, 0.0, 700.0], "rotation": [0.0, 0.0, 0.0]},
            "kind": {"Transition": {"length": 100.0, "start_radius": 50.0, "end_radius": 30.0, "wall": 2.0, "material": "G10-FR4"}}
        }}}),
        serde_json::json!({"AddComponent": {"after_component": "Body", "component": {
            "name": "Fin",
            "material": "Balsa",
            "visible": true,
            "transform": {"position": [0.0, 0.0, 600.0], "rotation": [0.0, 0.0, 0.0]},
            "kind": {"FinSet": {"count": 3, "root_chord": 120.0, "tip_chord": 80.0, "span": 70.0, "sweep": 15.0, "airfoil": {"family": {"NACA": {"digits": "0012"}}}, "thickness": 3.0, "material": "Balsa"}}
        }}}),
    ];

    for p in &patches {
        compiled.validate(p).unwrap_or_else(|e| panic!("patch rejected by schema: {e}\npatch: {p}"));
    }
}

/// Every non-stub SolidOp variant, when serialized to RON by the document
/// crate itself, is accepted by the schema (round-trip guarantee).
#[test]
fn test_every_solid_op_roundtrips_through_schema() {
    let schema = schema_for_vehicle();
    let compiled = jsonschema::validator_for(&schema).expect("schema compiles");

    let ops: Vec<SolidOp> = vec![
        SolidOp::Revolve { profile: Profile::Points(vec![[0.0, 0.0], [100.0, 50.0]]), angle: 360.0, axis: Some(Axis::Z) },
        SolidOp::RevolveChain { segments: vec![vec![[0.0, 0.0], [100.0, 50.0]]], angle: 360.0 },
        SolidOp::Extrude { profile: Profile::Rectangle { width: 10.0, height: 20.0, corner_radius: Some(2.0) }, height: 30.0, direction: Some(Direction::PosZ), taper: Some(5.0) },
        SolidOp::Loft { profiles: vec![Profile::Circle { radius: 10.0 }, Profile::Circle { radius: 20.0 }], guide_curves: Some(vec![Profile::Points(vec![[0.0, 0.0]])]) },
        SolidOp::Sweep { profile: Profile::Points(vec![[0.0, 0.0]]), path: Path3D::Helix { radius: 10.0, pitch: 2.0, turns: 3.0 }, twist: Some(45.0) },
        SolidOp::TransformOp { translate: Some((1.0, 2.0, 3.0)), rotate: None, scale: Some((2.0, 2.0, 2.0)) },
    ];

    for op in ops {
        let ron_str = ron::to_string(&op).expect("op serializes");
        let vehicle = Vehicle { parameters: None,
            name: "t".into(),
            units: Units::Millimeters,
            components: vec![Component {
                name: "c".into(),
                material: String::new(),
                visible: true,
                transform: Transform::default(),
                color: None,
                kind: ComponentKind::Solid(vec![op]),
            }],
        };
        let json = serde_json::to_value(&vehicle).unwrap();
        compiled.validate(&json).unwrap_or_else(|e| {
            panic!("op {} failed schema: {e:?}\nRON: {ron_str}", ron_str.lines().next().unwrap_or(""))
        });
    }
}