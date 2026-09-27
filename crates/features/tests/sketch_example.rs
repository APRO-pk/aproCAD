//! End-to-end check that the shipped sketch example evaluates to real geometry.
//!
//! The parser tests prove the RON is well formed; this proves the sketches
//! compile into closed loops and that the extrudes referencing them produce
//! solids in the expected places. It is also the regression guard for the
//! example file itself, so `examples/sketch-bracket.ron` cannot rot.

use apro_document::vehicle::{Component, ComponentKind, Units, Vehicle};
use apro_features::vehicle::build_vehicle_mesh;
use apro_kernel::{aabb, signed_volume};

fn load_example() -> Vehicle {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("sketch-bracket.ron");
    let ron = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    ron::from_str(&ron).expect("sketch-bracket.ron must parse")
}

fn single(component: &Component) -> Vehicle {
    Vehicle {
        parameters: None,
        name: "one".into(),
        units: Units::Millimeters,
        components: vec![component.clone()],
    }
}

/// The target component plus every sketch, so `Profile::Reference` resolves.
/// Sketches contribute no mesh, so the result is the target's geometry alone.
fn component_with_sketches(vehicle: &Vehicle, component: &Component) -> Vehicle {
    let mut components = vec![component.clone()];
    components.extend(
        vehicle
            .components
            .iter()
            .filter(|c| matches!(c.kind, ComponentKind::Sketch(_)))
            .cloned(),
    );
    Vehicle {
        parameters: None,
        name: "one".into(),
        units: Units::Millimeters,
        components,
    }
}

/// Bounding box of one named component's mesh, in assembly space.
fn component_bounds(vehicle: &Vehicle, name: &str) -> [f32; 6] {
    let comp = vehicle
        .components
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no component named {name}"));
    let mesh = build_vehicle_mesh(&component_with_sketches(vehicle, comp));
    assert!(
        !mesh.positions.is_empty(),
        "component {name} produced no geometry"
    );
    aabb(&mesh).expect("bounds")
}

#[test]
fn example_parses_and_every_sketch_renders_a_solid() {
    let v = load_example();
    assert_eq!(v.name, "SketchBracket");

    let sketches: Vec<&str> = v
        .components
        .iter()
        .filter(|c| matches!(c.kind, ComponentKind::Sketch(_)))
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(sketches, vec!["Sketch1", "Sketch2", "Sketch3"]);

    // Sketches are construction geometry and must contribute no mesh.
    for name in &sketches {
        let comp = v.components.iter().find(|c| c.name == *name).unwrap();
        assert!(
            build_vehicle_mesh(&single(comp)).positions.is_empty(),
            "sketch {name} must not render a mesh"
        );
    }

    // Every solid part evaluates to something, and the whole assembly is big
    // enough that no part collapsed to a sliver.
    let mesh = build_vehicle_mesh(&v);
    assert!(!mesh.positions.is_empty());
    let b = aabb(&mesh).expect("assembly bounds");
    assert!(b[3] - b[0] > 80.0, "assembly X span: {b:?}");
    assert!(b[5] - b[2] > 10.0, "assembly Z span: {b:?}");
}

#[test]
fn obround_plate_is_the_stitched_area_times_height() {
    // Sketch1 is two arcs + two lines: an obround (rect with semicircular
    // ends). Only correct end-to-end stitching closes it, so the volume is a
    // direct check on the compiler.
    let v = load_example();
    let b = component_bounds(&v, "Plate");
    assert!(b[0].abs() < 0.01, "x starts at 0: {b:?}");
    assert!((b[3] - 100.0).abs() < 0.5, "x ends at 100: {b:?}");
    assert!(b[1].abs() < 0.01 && (b[4] - 40.0).abs() < 0.5, "y span: {b:?}");
    assert!(b[2].abs() < 0.01 && (b[5] - 10.0).abs() < 0.01, "z span: {b:?}");
}

#[test]
fn spline_web_closes_against_its_chord() {
    // If the open spline failed to stitch with its closing line, the Web would
    // render nothing and `component_bounds` would panic.
    let v = load_example();
    let b = component_bounds(&v, "Web");
    // Sketch2 is on XZ (v runs up +Z) and extrudes 8 along the plane normal,
    // which is -Y for that plane.
    assert!((b[0] - 20.0).abs() < 0.01 && (b[3] - 80.0).abs() < 0.01, "web x span: {b:?}");
    assert!(b[2].abs() < 0.01, "web sits on the plane at z=0: {b:?}");
    // The spline interpolates its control points; a Catmull-Rom span between
    // equal-height points bulges slightly above them, so the crest is a little
    // over the 26 of the control points rather than exactly at it.
    assert!((26.0..30.0).contains(&b[5]), "spline crest near 26: {b:?}");
    assert!((b[1] + 8.0).abs() < 0.01 && b[4].abs() < 0.01, "web extrudes toward -Y: {b:?}");
}

#[test]
fn lifted_circle_sketch_starts_where_the_offset_says() {
    // Sketch3 sits at offset 10 on XY, so the boss starts at z=10 and rises 6.
    let v = load_example();
    let b = component_bounds(&v, "Boss");
    assert!((b[2] - 10.0).abs() < 0.01, "boss base at z=10: {b:?}");
    assert!((b[5] - 16.0).abs() < 0.01, "boss top at z=16: {b:?}");
    // radius 12 centred at (50, 20).
    assert!((b[0] - 38.0).abs() < 0.3 && (b[3] - 62.0).abs() < 0.3, "boss x: {b:?}");
}

#[test]
fn stitched_plate_volume_is_close_to_the_analytic_obround() {
    let v = load_example();
    let comp = v.components.iter().find(|c| c.name == "Plate").unwrap();
    let mesh = build_vehicle_mesh(&component_with_sketches(&v, comp));
    let vol = signed_volume(&mesh);

    // Obround = 60 x 40 rectangle + one r=20 disc; 10 thick; minus an 8mm hole.
    let obround = 60.0 * 40.0 + std::f64::consts::PI * 20.0 * 20.0;
    let hole = std::f64::consts::PI * 4.0 * 4.0;
    let expected = (obround - hole) * 10.0;
    assert!(
        (vol - expected).abs() / expected < 0.02,
        "plate volume {vol} vs analytic {expected}"
    );
}

// ---------------------------------------------------------------------------
// The interactive sketcher writes RON as text, not through serde. These
// fixtures are checked in copies of that emitter's real output, so the
// frontend's output shape cannot silently drift away from what the Rust parser
// accepts. To refresh them after changing the emitter in `src/sketcher.js`,
// regenerate with a document that has a bare Component plus a sketch using all
// five entity kinds, and update both files together.
// ---------------------------------------------------------------------------

fn emitted_fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn sketcher_emitted_document_parses_and_covers_every_entity_kind() {
    let ron = emitted_fixture("sketcher-emitted.ron");
    let v: Vehicle = ron::from_str(&ron).expect("sketcher output must parse as a Vehicle");

    // The original component survives untouched, and the sketch was appended.
    assert_eq!(v.components.len(), 2);
    assert_eq!(v.components[0].name, "Block");
    assert!(v.components[0].material == "Al-6061-T6");

    let sketch = match &v.components[1].kind {
        ComponentKind::Sketch(s) => s,
        other => panic!("expected a Sketch, got {other:?}"),
    };
    assert_eq!(v.components[1].name, "Sketch1");
    assert_eq!(sketch.plane, apro_document::vehicle::SketchPlane::XZ);
    assert!((sketch.offset - 5.0).abs() < 1e-9);
    // One of each entity kind the sketcher can draw.
    assert_eq!(sketch.entities.len(), 5);
    assert!(matches!(sketch.entities[0], apro_document::vehicle::SketchEntity::Line { .. }));
    assert!(matches!(sketch.entities[1], apro_document::vehicle::SketchEntity::Rectangle { .. }));
    assert!(matches!(sketch.entities[2], apro_document::vehicle::SketchEntity::Circle { .. }));
    assert!(matches!(sketch.entities[3], apro_document::vehicle::SketchEntity::Arc { .. }));
    assert!(matches!(sketch.entities[4], apro_document::vehicle::SketchEntity::Spline { .. }));

    // The untouched `Block` still renders — the frontend edit did not disturb
    // the rest of the document.
    let mesh = build_vehicle_mesh(&v);
    assert!(!mesh.positions.is_empty(), "the pre-existing Block must still evaluate");
}

#[test]
fn sketcher_replacement_keeps_exactly_one_sketch() {
    // Rewriting an existing sketch must swap it, not append a second copy.
    let ron = emitted_fixture("sketcher-emitted-replaced.ron");
    let v: Vehicle = ron::from_str(&ron).expect("replaced output must parse as a Vehicle");
    let sketches: Vec<&str> = v
        .components
        .iter()
        .filter(|c| matches!(c.kind, ComponentKind::Sketch(_)))
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(sketches, vec!["Sketch1"], "expected exactly one sketch after replace");

    // The sibling with a paren-heavy nested Points profile is still intact.
    let block = v.components.iter().find(|c| c.name == "Block").expect("Block kept");
    match &block.kind {
        ComponentKind::Solid(ops) => assert_eq!(ops.len(), 1),
        other => panic!("Block should still be a Solid, got {other:?}"),
    }
}
