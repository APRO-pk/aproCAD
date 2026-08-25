use apro_document::vehicle::*;

fn main() {
    let v = Vehicle { parameters: None,
        name: "patch".into(),
        units: Units::Millimeters,
        components: vec![Component {
            name: "Tank-1".into(), material: "Steel".into(), visible: true,
            transform: Transform::default(),
            color: None,
            kind: ComponentKind::Tank(TankParams { radius: 15.0, cylindrical_length: 50.0, dome: DomeKind::Hemispherical, wall: 2.0, material: "Steel".into() }),
        }],
    };
    let named = ron::ser::to_string_pretty(&v, ron::ser::PrettyConfig::new().struct_names(true)).unwrap();
    println!("=== struct_names(true) ===\n{named}\n===");
    let parsed: Vehicle = ron::from_str(&named).unwrap();
    println!("PARSE OK, eq={}", parsed == v);
}
