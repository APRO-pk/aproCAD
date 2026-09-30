use std::sync::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};
use apro_document::vehicle::{Component, ComponentKind, Issue, IssueSeverity, Vehicle,
    NoseConeProfile, DomeKind, Transform, SolidOp, Profile, SketchPlane, SketchEntity};
use apro_document::patch::{Patch, PatchResult, apply_patch};
use apro_document::validation::{validate_component, validate_vehicle};
use apro_massprops::{combine, compute_mass_properties, MassProperties};
use apro_document::material::density_kg_per_mm3;
use apro_features::eval::evaluate_solid_ops;
use apro_features::shorthands::{nosecone_to_ops, bodytube_to_ops, transition_to_ops, tank_to_ops, nozzle_to_ops};
use apro_features::fin::mesh_finset;
use apro_recompute::RecomputeEngine;
use apro_kernel::{write_step, write_stl, MeshData};
use std::path::PathBuf;
use ron::from_str;
use apro_library::{Library, RetrievalQuery, Source};


pub mod needle;

pub struct AppState {
    pub engine: Mutex<RecomputeEngine>,
    pub library: Mutex<Option<Library>>,
    pub needle: Mutex<Option<crate::needle::NeedleWorker>>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            engine: Mutex::new(RecomputeEngine::new()),
            library: Mutex::new(None),
            needle: Mutex::new(None),
        }
    }
}

/// Lazily open the library (default dir) and seed builtins on first use.
fn library<'a>(state: &'a tauri::State<'_, AppState>) -> Result<std::sync::MutexGuard<'a, Option<Library>>, String> {
    let mut guard = state.library.lock().map_err(|e| format!("Lock error: {}", e))?;
    if guard.is_none() {
        let lib = Library::open(None, None)?;
        if let Err(e) = lib.seed_builtins() {
            eprintln!("[library] builtin seeding failed: {e}");
        }
        *guard = Some(lib);
    }
    Ok(guard)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EvaluateResult {
    pub mesh: Option<MeshData>,
    /// One mesh per evaluated component (assembly space) so the frontend can
    /// render each in its own color. Empty for single-component evaluations.
    pub components: Vec<ComponentMesh>,
    /// Pairs of components whose meshes physically overlap.
    pub interferences: Vec<Interference>,
    pub issues: Vec<Issue>,
    pub success: bool,
    pub component_count: u32,
    pub mass_props: Option<MassProperties>,
    pub cache_hits: u32,
    pub cache_misses: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ComponentMesh {
    pub name: String,
    pub material: String,
    pub color: Option<String>,
    pub visible: bool,
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
}

impl ComponentMesh {
    fn to_mesh(&self) -> MeshData {
        MeshData {
            positions: self.positions.clone(),
            normals: self.normals.clone(),
            indices: self.indices.clone(),
        }
    }
}

/// Assemble the whole vehicle's mass properties from its parts.
///
/// Each part is measured at **its own** material density and then combined with the
/// parallel axis theorem. Measuring the union mesh with one density would weigh a carbon
/// nose cone as aluminium — a plausible-looking wrong number, which is the worst kind.
///
/// Returns `None` when no component produced a mesh, so the caller can fall back rather
/// than reporting a confident zero.
fn combine_component_mass_props(components: &[ComponentMesh]) -> Option<MassProperties> {
    if components.is_empty() {
        return None;
    }
    let per_part: Vec<MassProperties> = components
        .iter()
        .map(|c| compute_mass_properties(&c.to_mesh(), density_kg_per_mm3(&c.material)))
        .collect();
    Some(combine(&per_part))
}

/// The axis aproCAD bodies are modelled along.
///
/// The app's own presets build a rocket up +Z, so the longitudinal axis is Z. This is a
/// stated convention, not something read out of the document — a vehicle modelled along
/// another axis would need to say so.
const LONGITUDINAL_AXIS: usize = 2;

/// Axis-aligned bounding-box extent of the whole assembly, in document units.
///
/// Returns `None` for an assembly with no vertices, rather than a zero-sized box that
/// would produce a zero reference area.
fn assembly_extent(components: &[ComponentMesh]) -> Option<[f64; 3]> {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    let mut seen = false;

    for component in components {
        for point in component.positions.chunks_exact(3) {
            seen = true;
            for (axis, value) in point.iter().enumerate() {
                let value = *value as f64;
                if value < min[axis] {
                    min[axis] = value;
                }
                if value > max[axis] {
                    max[axis] = value;
                }
            }
        }
    }

    if !seen {
        return None;
    }
    Some([max[0] - min[0], max[1] - min[1], max[2] - min[2]])
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Interference {
    pub a: String,
    pub b: String,
    pub volume: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PropertyRow {
    pub key: String,
    pub value: String,
    pub field_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ComponentTable {
    pub name: String,
    pub kind: String,
    pub material: String,
    pub color: Option<String>,
    pub transform: Transform,
    pub rows: Vec<PropertyRow>,
}

fn float_row(key: &str, val: f64) -> PropertyRow {
    PropertyRow { key: key.into(), value: format!("{:.1}", val), field_type: "float".into() }
}
fn string_row(key: &str, val: &str) -> PropertyRow {
    PropertyRow { key: key.into(), value: val.into(), field_type: "string".into() }
}
fn int_row(key: &str, val: u32) -> PropertyRow {
    PropertyRow { key: key.into(), value: format!("{}", val), field_type: "int".into() }
}
fn points_row(key: &str, pts: &[[f64; 2]]) -> PropertyRow {
    let s = pts.iter().map(|[x, y]| format!("({:.1},{:.1})", x, y)).collect::<Vec<_>>().join(",");
    PropertyRow { key: key.into(), value: format!("[{}]", s), field_type: "points".into() }
}


/// Route any ComponentKind through the shorthands + evaluator pipeline.
fn mesh_component(kind: &ComponentKind) -> Option<MeshData> {
    let ops = match kind {
        ComponentKind::NoseCone(p) => Some(nosecone_to_ops(p)),
        ComponentKind::BodyTube(p) => Some(bodytube_to_ops(p)),
        ComponentKind::Transition(p) => Some(transition_to_ops(p)),
        ComponentKind::Tank(p) => Some(tank_to_ops(p)),
        ComponentKind::Nozzle(p) => Some(nozzle_to_ops(p)),
        ComponentKind::FinSet(p) => return Some(mesh_finset(p)),
        ComponentKind::Solid(ops) => return evaluate_solid_ops(ops).ok(),
        // Sketches are construction geometry; only the vehicle builder, which
        // can see sibling sketches, turns them into solids.
        ComponentKind::Sketch(_) => return None,
    };
    match ops {
        Some(op_list) => evaluate_solid_ops(&op_list).ok(),
        None => None,
    }
}

/// Add transform position/rotation/scale rows to any property table.
fn transform_rows(tf: &Transform) -> Vec<PropertyRow> {
    let (sx, sy, sz) = tf.scale.unwrap_or((1.0, 1.0, 1.0));
    vec![
        float_row("pos_x", tf.position.0),
        float_row("pos_y", tf.position.1),
        float_row("pos_z", tf.position.2),
        float_row("rot_x_deg", tf.rotation.0),
        float_row("rot_y_deg", tf.rotation.1),
        float_row("rot_z_deg", tf.rotation.2),
        float_row("scale_x", sx),
        float_row("scale_y", sy),
        float_row("scale_z", sz),
    ]
}

fn component_type_name(kind: &ComponentKind) -> &str {
    match kind {
        ComponentKind::NoseCone(_) => "NoseCone",
        ComponentKind::BodyTube(_) => "BodyTube",
        ComponentKind::Transition(_) => "Transition",
        ComponentKind::Tank(_) => "Tank",
        ComponentKind::Nozzle(_) => "Nozzle",
        ComponentKind::FinSet(_) => "FinSet",
        ComponentKind::Solid(_) => "Solid",
        ComponentKind::Sketch(_) => "Sketch",
    }
}

fn describe_kind(kind: &ComponentKind) -> Vec<PropertyRow> {
    match kind {
        ComponentKind::NoseCone(p) => {
            let mut r = vec![
                string_row("profile", &format!("{:?}", p.profile)),
                float_row("length", p.length),
                float_row("base_radius", p.base_radius),
                float_row("wall", p.wall),
                string_row("material", &p.material),
            ];
            if let NoseConeProfile::Haack { c } = &p.profile {
                r.push(float_row("c", *c));
            }
            if let NoseConeProfile::Power { n } = &p.profile {
                r.push(float_row("n", *n));
            }
            if let NoseConeProfile::Parabolic { k } = &p.profile {
                r.push(float_row("k", *k));
            }
            r
        }
        ComponentKind::BodyTube(p) => vec![
            float_row("length", p.length),
            float_row("radius", p.radius),
            float_row("wall", p.wall),
            string_row("material", &p.material),
        ],
        ComponentKind::Transition(p) => vec![
            float_row("length", p.length),
            float_row("start_radius", p.start_radius),
            float_row("end_radius", p.end_radius),
            float_row("wall", p.wall),
            string_row("material", &p.material),
        ],
        ComponentKind::Tank(p) => {
            let mut r = vec![
                float_row("radius", p.radius),
                float_row("cylindrical_length", p.cylindrical_length),
                string_row("dome", &format!("{:?}", p.dome)),
                float_row("wall", p.wall),
                string_row("material", &p.material),
            ];
            if let DomeKind::Ellipsoidal { ratio } = &p.dome {
                r.push(float_row("ratio", *ratio));
            }
            r
        }
        ComponentKind::Nozzle(p) => vec![
            string_row("kind", &format!("{:?}", p.kind)),
            float_row("throat_radius", p.throat_radius),
            float_row("expansion_ratio", p.expansion_ratio),
            float_row("percent_bell", p.percent_bell),
            float_row("chamber_radius", p.chamber_radius),
            float_row("wall", p.wall),
            string_row("material", &p.material),
        ],
        ComponentKind::FinSet(p) => vec![
            int_row("count", p.count),
            float_row("root_chord", p.root_chord),
            float_row("tip_chord", p.tip_chord),
            float_row("span", p.span),
            float_row("sweep", p.sweep),
            string_row("airfoil", &format!("{:?}", p.airfoil)),
            float_row("thickness", p.thickness),
            string_row("material", &p.material),
        ],
        ComponentKind::Solid(ops) => {
            let mut r = vec![
                int_row("op_count", ops.len() as u32),
            ];
            for (i, op) in ops.iter().enumerate() {
                match op {
                    SolidOp::Revolve { profile, angle, axis } => {
                        if let Profile::Points(pts) = profile {
                            r.push(points_row(&format!("op_{}_profile", i), pts));
                        } else {
                            r.push(string_row(&format!("op_{}_profile", i), &format!("{:?}", profile)));
                        }
                        r.push(float_row(&format!("op_{}_angle", i), *angle));
                        r.push(string_row(&format!("op_{}_axis", i), &format!("{:?}", axis)));
                    }
                    SolidOp::Extrude { profile, height, direction, taper } => {
                        if let Profile::Points(pts) = profile {
                            r.push(points_row(&format!("op_{}_profile", i), pts));
                        } else {
                            r.push(string_row(&format!("op_{}_profile", i), &format!("{:?}", profile)));
                        }
                        r.push(float_row(&format!("op_{}_height", i), *height));
                        r.push(string_row(&format!("op_{}_direction", i), &format!("{:?}", direction)));
                        r.push(string_row(&format!("op_{}_taper", i), &format!("{:?}", taper)));
                    }
                    SolidOp::Loft { profiles, guide_curves } => {
                        for (j, prof) in profiles.iter().enumerate() {
                            if let Profile::Points(pts) = prof {
                                r.push(points_row(&format!("op_{}_profile_{}", i, j), pts));
                            } else {
                                r.push(string_row(&format!("op_{}_profile_{}", i, j), &format!("{:?}", prof)));
                            }
                        }
                        r.push(string_row(&format!("op_{}_guide_curves", i), &format!("{:?}", guide_curves)));
                    }
                    SolidOp::Sweep { profile, path, twist } => {
                        if let Profile::Points(pts) = profile {
                            r.push(points_row(&format!("op_{}_profile", i), pts));
                        } else {
                            r.push(string_row(&format!("op_{}_profile", i), &format!("{:?}", profile)));
                        }
                        r.push(string_row(&format!("op_{}_path", i), &format!("{:?}", path)));
                        r.push(string_row(&format!("op_{}_twist", i), &format!("{:?}", twist)));
                    }
                    SolidOp::Shell { thickness, faces } => {
                        r.push(float_row(&format!("op_{}_thickness", i), *thickness));
                        r.push(string_row(&format!("op_{}_faces", i), &format!("{:?}", faces)));
                    }
                    SolidOp::Boolean { kind, target } => {
                        r.push(string_row(&format!("op_{}_kind", i), &format!("{:?}", kind)));
                        r.push(string_row(&format!("op_{}_target", i), &format!("{:?}", target)));
                    }
                    SolidOp::Fillet { radius, edges } => {
                        r.push(float_row(&format!("op_{}_radius", i), *radius));
                        r.push(string_row(&format!("op_{}_edges", i), &format!("{:?}", edges)));
                    }
                    SolidOp::Chamfer { distance, edges } => {
                        r.push(float_row(&format!("op_{}_distance", i), *distance));
                        r.push(string_row(&format!("op_{}_edges", i), &format!("{:?}", edges)));
                    }
                    SolidOp::TransformOp { translate, rotate, scale } => {
                        r.push(string_row(&format!("op_{}_translate", i), &format!("{:?}", translate)));
                        r.push(string_row(&format!("op_{}_rotate", i), &format!("{:?}", rotate)));
                        r.push(string_row(&format!("op_{}_scale", i), &format!("{:?}", scale)));
                    }
                    SolidOp::RevolveChain { segments, angle } => {
                        r.push(int_row(&format!("op_{}_segments", i), segments.len() as u32));
                        r.push(float_row(&format!("op_{}_angle", i), *angle));
                        for (j, seg) in segments.iter().enumerate() {
                            r.push(points_row(&format!("op_{}_seg_{}", i, j), seg));
                        }
                    }
                    SolidOp::Hole { diameter, depth, axis } => {
                        r.push(float_row(&format!("op_{}_diameter", i), *diameter));
                        r.push(float_row(&format!("op_{}_depth", i), *depth));
                        r.push(string_row(&format!("op_{}_axis", i), &format!("{:?}", axis)));
                    }
                    SolidOp::BoltCircle { count, pitch_diameter, hole_diameter, depth } => {
                        r.push(int_row(&format!("op_{}_count", i), *count));
                        r.push(float_row(&format!("op_{}_pitch_diameter", i), *pitch_diameter));
                        r.push(float_row(&format!("op_{}_hole_diameter", i), *hole_diameter));
                        r.push(float_row(&format!("op_{}_depth", i), *depth));
                    }
                    SolidOp::RectPattern { x_count, y_count, spacing_x, spacing_y, hole_diameter, depth } => {
                        r.push(int_row(&format!("op_{}_x_count", i), *x_count));
                        r.push(int_row(&format!("op_{}_y_count", i), *y_count));
                        r.push(float_row(&format!("op_{}_spacing_x", i), *spacing_x));
                        r.push(float_row(&format!("op_{}_spacing_y", i), *spacing_y));
                        r.push(float_row(&format!("op_{}_hole_diameter", i), *hole_diameter));
                        r.push(float_row(&format!("op_{}_depth", i), *depth));
                    }
                }
            }
            r
        }
        ComponentKind::Sketch(s) => {
            let mut r = vec![
                string_row("plane", &format!("{:?}", s.plane)),
                float_row("offset", s.offset),
                int_row("entity_count", s.entities.len() as u32),
            ];
            for (i, e) in s.entities.iter().enumerate() {
                let label = match e {
                    SketchEntity::Line { .. } => "line",
                    SketchEntity::Rectangle { .. } => "rectangle",
                    SketchEntity::Circle { .. } => "circle",
                    SketchEntity::Arc { .. } => "arc",
                    SketchEntity::Spline { .. } => "spline",
                };
                r.push(string_row(&format!("entity_{}", i), label));
            }
            r
        }
    }
}

#[tauri::command]
async fn describe_vehicle(vehicle_ron: String) -> Result<Vec<ComponentTable>, String> {
    let vehicle: Vehicle = ron::from_str(&vehicle_ron)
        .map_err(|e| format!("Failed to parse: {}", e))?;
    let tables = vehicle.components.iter().map(|c| {
        let mut rows = describe_kind(&c.kind);
        rows.extend(transform_rows(&c.transform));
        ComponentTable {
            name: c.name.clone(),
            kind: component_type_name(&c.kind).into(),
            material: c.material.clone(),
            color: c.color.clone(),
            transform: c.transform.clone(),
            rows,
        }
    }).collect();
    Ok(tables)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ParameterRow {
    pub name: String,
    /// Literal number text or the raw equation string, as authored.
    pub value: String,
    /// Resolved numeric value when the parameter block is consistent.
    pub computed: Option<f64>,
}

#[tauri::command]
async fn describe_parameters(vehicle_ron: String) -> Result<Vec<ParameterRow>, String> {
    let vehicle: Vehicle = ron::from_str(&vehicle_ron)
        .map_err(|e| format!("Failed to parse: {}", e))?;
    let resolved = match &vehicle.parameters {
        Some(ps) => apro_document::params::resolve_parameters(ps).ok(),
        None => None,
    };
    let mut rows = Vec::new();
    if let Some(ps) = &vehicle.parameters {
        for p in ps {
            let (text, computed) = match &p.value {
                apro_document::params::Expr::Number(n) => (n.to_string(), Some(*n)),
                apro_document::params::Expr::Expression(e) => (
                    e.clone(),
                    resolved.as_ref()
                        .and_then(|v| v.iter().find(|(n, _)| n == &p.name).map(|(_, val)| *val)),
                ),
            };
            rows.push(ParameterRow { name: p.name.clone(), value: text, computed });
        }
    }
    Ok(rows)
}

fn parse_f64(s: &str) -> Result<f64, String> {
    s.parse::<f64>().map_err(|_| format!("invalid float: {}", s))
}

fn parse_u32(s: &str) -> Result<u32, String> {
    s.parse::<u32>().map_err(|_| format!("invalid int: {}", s))
}

fn modify_component_kind(kind: &mut ComponentKind, key: &str, value: &str) -> Result<(), String> {
    match kind {
        ComponentKind::NoseCone(p) => match key {
            "length" => { p.length = parse_f64(value)?; Ok(()) }
            "base_radius" => { p.base_radius = parse_f64(value)?; Ok(()) }
            "wall" => { p.wall = parse_f64(value)?; Ok(()) }
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
            "c" => { p.profile = NoseConeProfile::Haack { c: parse_f64(value)? }; Ok(()) }
            "n" => { p.profile = NoseConeProfile::Power { n: parse_f64(value)? }; Ok(()) }
            "k" => { p.profile = NoseConeProfile::Parabolic { k: parse_f64(value)? }; Ok(()) }
            _ => Err(format!("unknown key: {}", key)),
        },
        ComponentKind::BodyTube(p) => match key {
            "length" => { p.length = parse_f64(value)?; Ok(()) }
            "radius" => { p.radius = parse_f64(value)?; Ok(()) }
            "wall" => { p.wall = parse_f64(value)?; Ok(()) }
            "material" => { p.material = value.into(); Ok(()) }
            _ => Err(format!("unknown key: {}", key)),
        },
        ComponentKind::Transition(p) => match key {
            "length" => { p.length = parse_f64(value)?; Ok(()) }
            "start_radius" => { p.start_radius = parse_f64(value)?; Ok(()) }
            "end_radius" => { p.end_radius = parse_f64(value)?; Ok(()) }
            "wall" => { p.wall = parse_f64(value)?; Ok(()) }
            "material" => { p.material = value.into(); Ok(()) }
            _ => Err(format!("unknown key: {}", key)),
        },
        ComponentKind::Tank(p) => match key {
            "radius" => { p.radius = parse_f64(value)?; Ok(()) }
            "cylindrical_length" => { p.cylindrical_length = parse_f64(value)?; Ok(()) }
            "wall" => { p.wall = parse_f64(value)?; Ok(()) }
            "material" => { p.material = value.into(); Ok(()) }
            "ratio" => { p.dome = DomeKind::Ellipsoidal { ratio: parse_f64(value)? }; Ok(()) }
            _ => Err(format!("unknown key: {}", key)),
        },
        ComponentKind::Nozzle(p) => match key {
            "throat_radius" => { p.throat_radius = parse_f64(value)?; Ok(()) }
            "expansion_ratio" => { p.expansion_ratio = parse_f64(value)?; Ok(()) }
            "percent_bell" => { p.percent_bell = parse_f64(value)?; Ok(()) }
            "chamber_radius" => { p.chamber_radius = parse_f64(value)?; Ok(()) }
            "wall" => { p.wall = parse_f64(value)?; Ok(()) }
            "material" => { p.material = value.into(); Ok(()) }
            _ => Err(format!("unknown key: {}", key)),
        },
        ComponentKind::FinSet(p) => match key {
            "count" => { p.count = parse_u32(value)?; Ok(()) }
            "root_chord" => { p.root_chord = parse_f64(value)?; Ok(()) }
            "tip_chord" => { p.tip_chord = parse_f64(value)?; Ok(()) }
            "span" => { p.span = parse_f64(value)?; Ok(()) }
            "sweep" => { p.sweep = parse_f64(value)?; Ok(()) }
            "thickness" => { p.thickness = parse_f64(value)?; Ok(()) }
            "material" => { p.material = value.into(); Ok(()) }
            _ => Err(format!("unknown key: {}", key)),
        },
        ComponentKind::Solid(ops) => {
            // Key format: op_{index}_{field}
            let parts: Vec<&str> = key.splitn(3, '_').collect();
            if parts.len() < 3 || parts[0] != "op" {
                return Err(format!("invalid Solid op key format: {}", key));
            }
            let idx: usize = parts[1].parse().map_err(|_| format!("invalid op index: {}", parts[1]))?;
            if idx >= ops.len() {
                return Err(format!("op index {} out of range (len {})", idx, ops.len()));
            }
            let field = parts[2];
            // Parse [[x,y],...] point list
            let parse_pts = |s: &str| -> Result<Vec<[f64; 2]>, String> {
                let s = s.trim();
                if !s.starts_with('[') || !s.ends_with(']') {
                    return Err(format!("expected point array [(x,y),...], got: {}", s));
                }
                let inner = &s[1..s.len()-1];
                if inner.is_empty() { return Ok(Vec::new()); }
                // Accept both [(x,y),(x,y)] and [[x,y],[x,y]] formats
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
            match &mut ops[idx] {
                SolidOp::Revolve { profile, angle, axis: _ } => match field {
                    "profile" => { *profile = Profile::Points(parse_pts(value)?); Ok(()) }
                    "angle" => { *angle = parse_f64(value)?; Ok(()) }
                    _ => Err(format!("unknown Revolve field: {}", field)),
                },
                SolidOp::Extrude { profile, height, direction: _, taper: _ } => match field {
                    "profile" => { *profile = Profile::Points(parse_pts(value)?); Ok(()) }
                    "height" => { *height = parse_f64(value)?; Ok(()) }
                    _ => Err(format!("unknown Extrude field: {}", field)),
                },
                SolidOp::Loft { profiles: _, guide_curves: _ } => {
                    // field format: op_{i}_profile_{j}
                    if field == "profile" || field.starts_with("profile_") {
                        return Err("Loft profile editing not supported via property table".into());
                    }
                    Err(format!("unknown Loft field: {}", field))
                }
                SolidOp::Sweep { profile: _, path: _, twist: _ } => {
                    Err("Sweep field editing not supported via property table".into())
                }
                SolidOp::RevolveChain { segments: _, angle } => match field {
                    "angle" => { *angle = parse_f64(value)?; Ok(()) }
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
            "offset" => { s.offset = parse_f64(value)?; Ok(()) }
            _ => Err("Sketch entities are edited in the RON editor, not the property table".into()),
        },
    }
}

fn to_named_ron<T: serde::Serialize>(value: &T) -> Result<String, ron::Error> {
    ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::new().struct_names(true))
}

#[tauri::command]
fn modify_property(vehicle_ron: String, component_name: String, key: String, value: String) -> Result<String, String> {
    let mut vehicle: Vehicle = ron::from_str(&vehicle_ron)
        .map_err(|e| format!("Failed to parse: {}", e))?;
    let comp = vehicle.components.iter_mut()
        .find(|c| c.name == component_name)
        .ok_or_else(|| format!("component '{}' not found", component_name))?;
    // Handle component-level properties first (transform fields, material)
    match key.as_str() {
        "pos_x" => { comp.transform.position.0 = parse_f64(&value)?; return Ok(to_named_ron(&vehicle).map_err(|e| format!("Failed to serialize: {}", e))?); }
        "pos_y" => { comp.transform.position.1 = parse_f64(&value)?; return Ok(to_named_ron(&vehicle).map_err(|e| format!("Failed to serialize: {}", e))?); }
        "pos_z" => { comp.transform.position.2 = parse_f64(&value)?; return Ok(to_named_ron(&vehicle).map_err(|e| format!("Failed to serialize: {}", e))?); }
        "rot_x_deg" => { comp.transform.rotation.0 = parse_f64(&value)?; return Ok(to_named_ron(&vehicle).map_err(|e| format!("Failed to serialize: {}", e))?); }
        "rot_y_deg" => { comp.transform.rotation.1 = parse_f64(&value)?; return Ok(to_named_ron(&vehicle).map_err(|e| format!("Failed to serialize: {}", e))?); }
        "rot_z_deg" => { comp.transform.rotation.2 = parse_f64(&value)?; return Ok(to_named_ron(&vehicle).map_err(|e| format!("Failed to serialize: {}", e))?); }
        "scale_x" | "scale_y" | "scale_z" => {
            let s = comp.transform.scale.unwrap_or((1.0, 1.0, 1.0));
            let v = parse_f64(&value)?;
            comp.transform.scale = Some(match key.as_str() {
                "scale_x" => (v, s.1, s.2),
                "scale_y" => (s.0, v, s.2),
                _ => (s.0, s.1, v),
            });
            return Ok(to_named_ron(&vehicle).map_err(|e| format!("Failed to serialize: {}", e))?);
        }
        "material" => { comp.material = value.into(); return Ok(to_named_ron(&vehicle).map_err(|e| format!("Failed to serialize: {}", e))?); }
        "color" => {
            let trimmed = value.trim();
            comp.color = if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") { None } else { Some(trimmed.to_string()) };
            return Ok(to_named_ron(&vehicle).map_err(|e| format!("Failed to serialize: {}", e))?);
        }
        "visible" => { comp.visible = value.parse::<bool>().map_err(|_| format!("invalid bool: {}", value))?; return Ok(to_named_ron(&vehicle).map_err(|e| format!("Failed to serialize: {}", e))?); }
        _ => {}
    }
    modify_component_kind(&mut comp.kind, &key, &value)?;
    to_named_ron(&vehicle).map_err(|e| format!("Failed to serialize: {}", e))
}

#[tauri::command]
async fn evaluate(component_ron: String) -> Result<EvaluateResult, String> {
    let component: Component = ron::from_str(&component_ron)
        .map_err(|e| format!("Failed to parse RON: {}", e))?;

    let issues = validate_component(&component);

    let mesh = mesh_component(&component.kind);

    let mass_props = mesh.as_ref().map(|m| {
        compute_mass_properties(m, density_kg_per_mm3(&component.material_name()))
    });

    Ok(EvaluateResult {
        success: mesh.is_some() && issues.iter().all(|i| i.severity != IssueSeverity::Error),
        mesh,
        components: vec![],
        interferences: vec![],
        issues,
        component_count: 1,
        mass_props,
        cache_hits: 0,
        cache_misses: 0,
    })
}

/// NOTE: every heavy command here is `async` on purpose. Tauri runs a
/// synchronous command on the main thread, which freezes the window AND blocks
/// delivery of the `eval-progress` events this command emits — so the bar could
/// never animate. An `async` command is dispatched onto the async runtime, so
/// the UI stays responsive and progress arrives while the work is happening.
#[tauri::command]
async fn evaluate_vehicle(
    vehicle_ron: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<EvaluateResult, String> {
    let vehicle: Vehicle = ron::from_str(&vehicle_ron)
        .map_err(|e| format!("Failed to parse vehicle RON: {}", e))?;

    let mut issues = Vec::new();
    issues.extend(validate_vehicle(&vehicle));

    let mut engine = state.engine.lock().map_err(|e| format!("Lock error: {}", e))?;
    let emitter = app.clone();
    let (mesh, hits, misses) = engine.evaluate_vehicle_with_progress(&vehicle, move |phase, done, total, name| {
        let _ = emitter.emit(
            "eval-progress",
            serde_json::json!({ "phase": phase, "done": done, "total": total, "current": name }),
        );
    });

    let components: Vec<ComponentMesh> = engine.component_meshes(&vehicle).into_iter().map(|(name, material, color, m)| {
        let visible = vehicle.components.iter().find(|c| c.name == name).map(|c| c.visible).unwrap_or(true);
        ComponentMesh { name, material, color, visible, positions: m.positions, normals: m.normals, indices: m.indices }
    }).collect();

    // Measure each part at its OWN density, then combine.
    //
    // The union mesh has no material: it is one triangle soup drawn from every
    // component. Measuring it with a single density silently weighs a carbon nose cone
    // as aluminium, which is exactly the kind of error that survives a spot-check
    // because the number it produces is still plausible.
    let mass_props = Some(match combine_component_mass_props(&components) {
        Some(combined) => combined,
        // Nothing was cached, so fall back to the union mesh with the default density.
        None => compute_mass_properties(&mesh, density_kg_per_mm3("")),
    });

    Ok(EvaluateResult {
        success: mesh.positions.len() > 0 && issues.iter().all(|i| i.severity != IssueSeverity::Error),
        mesh: Some(mesh),
        components,
        interferences: vec![],
        issues,
        component_count: vehicle.components.len() as u32,
        mass_props,
        cache_hits: hits,
        cache_misses: misses,
    })
}

/// Pairwise assembly-space overlap check over every component mesh.
///
/// The CSG cost grows with the PRODUCT of the two parts' triangle counts, and
/// measured on the bundled presets it is far worse than it looks: the
/// full-rocket preset spends ~27 s in five pairs alone (NoseCone × Fins, product
/// ~5M, is ~8.8 s), while a *small* pair can still be slow when the two shapes
/// genuinely interpenetrate.
///
/// So this is bounded three ways — a per-pair triangle-product cap, a
/// wall-clock budget, and a per-pair predicted-cost guard (the time check can
/// only run *between* pairs, so one expensive pair must be refused up front).
/// Skipping a pair can miss a real interference; that is the deliberate
/// trade-off, because a check the user never sees finish is worse than no check.
/// The number of skipped pairs is reported so the UI can say so.
fn detect_interferences(comps: &[ComponentMesh]) -> (Vec<Interference>, usize) {
    const MAX_PAIRS: usize = 32;
    const MAX_TRIS: usize = 200_000;       // whole-assembly budget
    const MAX_PAIR_PRODUCT: u64 = 150_000; // ~15 ms .. ~1 s depending on shape
    const TIME_BUDGET_MS: f64 = 1_200.0;   // hard wall-clock ceiling

    let started = std::time::Instant::now();
    let elapsed_ms = || started.elapsed().as_secs_f64() * 1000.0;
    let tot_tris: usize = comps.iter().map(|c| c.indices.len() / 3).sum();
    if comps.len() < 2 || tot_tris > MAX_TRIS {
        return (Vec::new(), 0);
    }

    // Bounds only — no mesh clone, no CSG.
    let boxes: Vec<Option<[f32; 6]>> = comps.iter().map(|c| {
        if c.positions.len() < 9 { None } else { apro_kernel::aabb(&c.to_mesh()) }
    }).collect();

    let mut out = Vec::new();
    let mut pairs_tested = 0usize;
    let mut skipped = 0usize;
    for i in 0..comps.len() {
        let Some(bi) = boxes[i] else { continue; };
        let ta = (comps[i].indices.len() / 3) as u64;
        for j in (i + 1)..comps.len() {
            if pairs_tested >= MAX_PAIRS { return (out, skipped); }
            let used = elapsed_ms();
            if used > TIME_BUDGET_MS {
                return (out, skipped + 1); // ran out of time: report as incomplete
            }
            let Some(bj) = boxes[j] else { continue; };
            if bi[0] > bj[3] || bj[0] > bi[3] || bi[1] > bj[4] || bj[1] > bi[4] || bi[2] > bj[5] || bj[2] > bi[5] {
                continue; // disjoint bounds: cannot intersect
            }
            let tb = (comps[j].indices.len() / 3) as u64;
            let prod = ta * tb;
            if prod > MAX_PAIR_PRODUCT {
                skipped += 1;
                continue;
            }
            // The wall-clock check only runs BETWEEN pairs, so refuse a pair that
            // would not finish inside what is left of the budget. Measured
            // throughput is roughly 30k..100k triangle-pairs per ms depending on
            // how much the shapes actually overlap, so this is a conservative
            // estimate (it assumes the slow end).
            let remaining = TIME_BUDGET_MS - used;
            if (prod as f64) > remaining * 30_000.0 {
                skipped += 1;
                continue;
            }
            pairs_tested += 1;
            if let Some(vol) = apro_kernel::meshes_overlap_volume(&comps[i].to_mesh(), &comps[j].to_mesh()) {
                out.push(Interference { a: comps[i].name.clone(), b: comps[j].name.clone(), volume: vol });
            }
        }
    }
    (out, skipped)
}

/// Result of an interference sweep. Carries `skipped` so the UI can be honest:
/// pairs skipped for cost reasons are *unchecked*, not "clear".
#[derive(Debug, Serialize, Deserialize)]
pub struct InterferenceReport {
    pub found: Vec<Interference>,
    /// Pairs that were not tested because they exceeded the cost budget.
    pub skipped: usize,
}

/// Invoked by the frontend AFTER an evaluate returns. Async so the work runs off
/// the main thread, and the engine lock is released before any CSG runs: the
/// expensive part below used to hold it for tens of seconds, so the *next*
/// evaluation blocked on the mutex and the UI sat at "Evaluating 0%".
#[tauri::command]
async fn check_interferences(
    vehicle_ron: String,
    state: tauri::State<'_, AppState>,
) -> Result<InterferenceReport, String> {
    let vehicle: Vehicle = ron::from_str(&vehicle_ron)
        .map_err(|e| format!("Failed to parse vehicle RON: {}", e))?;

    // Take what we need from the cache, then drop the guard immediately: the
    // CSG below must never run while the engine is locked.
    let comps: Vec<ComponentMesh> = {
        let engine = state.engine.lock().map_err(|e| format!("Lock error: {}", e))?;
        engine.component_meshes(&vehicle).into_iter().map(|(name, material, color, m)| {
            let visible = vehicle.components.iter().find(|c| c.name == name).map(|c| c.visible).unwrap_or(true);
            ComponentMesh { name, material, color, visible, positions: m.positions, normals: m.normals, indices: m.indices }
        }).collect()
    };
    let (found, skipped) = detect_interferences(&comps);
    Ok(InterferenceReport { found, skipped })
}

/// Needle Mode (spike): run one `complete()` turn against the warm in-process
/// tool-calling model. `tools` are JSON schemas (e.g. from `get_ai_schema`);
/// the host must execute any returned `function_calls` itself and feed the
/// result back via a follow-up `needle_run`.
#[tauri::command]
fn needle_run(
    query: String,
    tools: Vec<serde_json::Value>,
    weights: Option<String>,
    reset: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let req = crate::needle::NeedleRequest {
        query,
        tools,
        weights,
        reset: reset.unwrap_or(true),
    };
    crate::needle::needle_run_impl(&state.needle, &req)
}

/// The exact narrow tool schema the local Needle model was trained on. Must be
/// served byte-for-byte: the model memorised this tool block, so re-describing
/// the same tools degrades it sharply (measured 6/8 -> 1/8).
const NEEDLE_TOOLS_NARROW: &str = include_str!("../needle_tools_narrow.json");

#[tauri::command]
fn get_needle_schema() -> Result<Vec<serde_json::Value>, String> {
    serde_json::from_str(NEEDLE_TOOLS_NARROW)
        .map_err(|e| format!("embedded needle schema is invalid: {e}"))
}

/// Native open-file dialog; returns the chosen path (None when cancelled).
#[tauri::command]
fn pick_file_dialog(
    filter_name: String,
    extensions: Vec<String>,
) -> Result<Option<String>, String> {
    let mut dialog = rfd::FileDialog::new();
    let exts: Vec<&str> = extensions.iter().map(|s| s.as_str()).collect();
    if !exts.is_empty() {
        dialog = dialog.add_filter(&filter_name, &exts);
    }
    Ok(dialog.pick_file().map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
fn validate(component_ron: String) -> Result<Vec<Issue>, String> {
    let component: Component = ron::from_str(&component_ron)
        .map_err(|e| format!("Failed to parse RON: {}", e))?;
    Ok(validate_component(&component))
}

#[tauri::command]
async fn apply_patch_vehicle(vehicle_ron: String, patch: Patch) -> Result<PatchResult, String> {
    let mut vehicle: Vehicle = from_str(&vehicle_ron)
        .map_err(|e| format!("Failed to parse: {}", e))?;
    Ok(apply_patch(&mut vehicle, &patch))
}

/// Phase 3: apply a Patch emitted by the AI as RON (local GBNF path) or
/// converted from JSON (cloud json_schema path).
#[tauri::command]
async fn apply_patch_ron(vehicle_ron: String, patch_ron: String) -> Result<PatchResult, String> {
    let mut vehicle: Vehicle = from_str(&vehicle_ron)
        .map_err(|e| format!("Failed to parse vehicle: {}", e))?;
    let patch: Patch = from_str(&patch_ron)
        .map_err(|e| format!("Failed to parse patch: {}", e))?;
    Ok(apply_patch(&mut vehicle, &patch))
}

fn parse_vehicle_or_component(ron_str: &str) -> Result<Vehicle, String> {
    if let Ok(v) = from_str::<Vehicle>(ron_str) {
        return Ok(v);
    }
    let comp: Component = from_str(ron_str).map_err(|_| {
        "Expected Vehicle(...) or Component(...) RON".to_string()
    })?;
    Ok(Vehicle { uid: None, parameters: None,
        name: "Export".into(),
        units: apro_document::vehicle::Units::Millimeters,
        components: vec![comp],
    })
}

#[tauri::command]
async fn export_step(vehicle_ron: String, path: String) -> Result<String, String> {
    let vehicle = parse_vehicle_or_component(&vehicle_ron)?;
    if vehicle.components.is_empty() {
        return Err("No components to export".into());
    }
    let file_path = PathBuf::from(&path);
    // STEP requires B-Rep — use BuildSolid for types that support it
    let solid = match &vehicle.components[0].kind {
        ComponentKind::NoseCone(p) => apro_features::BuildSolid::build(p),
        ComponentKind::BodyTube(p) => apro_features::BuildSolid::build(p),
        ComponentKind::Transition(p) => apro_features::BuildSolid::build(p),
        ComponentKind::Tank(p) => apro_features::BuildSolid::build(p),
        ComponentKind::Nozzle(p) => apro_features::BuildSolid::build(p),
        ComponentKind::FinSet(_) => return Err("STEP export not yet supported for fin sets".into()),
        ComponentKind::Solid(_) => return Err("STEP export not yet supported for Solid kind (mesh-only pipeline)".into()),
        ComponentKind::Sketch(_) => return Err("STEP export not supported for sketches (construction geometry)".into()),
    };
    write_step(&solid, &file_path)?;
    Ok(format!("STEP exported to {}", file_path.display()))
}

#[tauri::command]
async fn export_stl(vehicle_ron: String, path: String) -> Result<String, String> {
    let vehicle = parse_vehicle_or_component(&vehicle_ron)?;
    let file_path = PathBuf::from(&path);
    let mesh = if vehicle.components.len() == 1 {
        mesh_component(&vehicle.components[0].kind)
            .ok_or_else(|| "No mesh generated for STL export".to_string())?
    } else {
        let mut engine = RecomputeEngine::new();
        let (mesh, _, _) = engine.evaluate_vehicle(&vehicle);
        if mesh.positions.is_empty() {
            return Err("No mesh generated".into());
        }
        mesh
    };
    write_stl(&mesh, &file_path)?;
    Ok(format!("STL exported to {}", file_path.display()))
}

// ---------------------------------------------------------------------------
// Component library commands (Phase 1: Component Library + Retrieval)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct LibraryRetrieveResponse {
    pub hits: Vec<apro_library::RetrievedHit>,
    pub dimension_mismatch: bool,
}

#[tauri::command]
fn library_save(
    vehicle_ron: String,
    component_name: String,
    description: String,
    tags: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let guard = library(&state)?;
    let lib = guard.as_ref().unwrap();
    lib.save_component(&apro_library::ingest::IngestInput {
        vehicle_ron,
        component_name,
        description,
        tags,
        source: Source::UserSaved,
    })
}

#[tauri::command]
fn library_save_ron(
    name: String,
    description: String,
    tags: Vec<String>,
    component_ron: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let guard = library(&state)?;
    let lib = guard.as_ref().unwrap();
    lib.save_component_ron(name, description, tags, &component_ron, Source::AIGenerated)
}

#[tauri::command]
fn library_retrieve(
    text: String,
    kind: Option<String>,
    od_mm: Option<f64>,
    length_mm: Option<f64>,
    length_max_mm: Option<f64>,
    fits_od_mm: Option<f64>,
    limit: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> Result<LibraryRetrieveResponse, String> {
    let guard = library(&state)?;
    let lib = guard.as_ref().unwrap();
    let query = RetrievalQuery {
        text,
        kind: kind.and_then(|k| match k.as_str() {
            "NoseCone" => Some(apro_library::EntryKind::NoseCone),
            "BodyTube" => Some(apro_library::EntryKind::BodyTube),
            "Tank" => Some(apro_library::EntryKind::Tank),
            "Nozzle" => Some(apro_library::EntryKind::Nozzle),
            "FinSet" => Some(apro_library::EntryKind::FinSet),
            "Transition" => Some(apro_library::EntryKind::Transition),
            "Solid" => Some(apro_library::EntryKind::Solid),
            "Sketch" => Some(apro_library::EntryKind::Sketch),
            _ => None,
        }),
        od_mm,
        length_mm,
        length_max_mm,
        fits_od_mm,
        ..Default::default()
    };
    let hits = lib.retrieval.retrieve(&query, limit.unwrap_or(5))?;
    let dimension_mismatch = hits.iter().any(|h| h.dimension_mismatch);
    Ok(LibraryRetrieveResponse { hits, dimension_mismatch })
}

#[tauri::command]
fn library_list(state: tauri::State<'_, AppState>) -> Result<Vec<apro_library::LibraryEntry>, String> {
    let guard = library(&state)?;
    let lib = guard.as_ref().unwrap();
    lib.store().list()
}

#[tauri::command]
fn library_delete(id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let guard = library(&state)?;
    let lib = guard.as_ref().unwrap();
    lib.store().delete(&id)
}

#[tauri::command]
fn library_update(
    id: String,
    name: String,
    description: String,
    tags: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let guard = library(&state)?;
    let lib = guard.as_ref().unwrap();
    lib.store().update_metadata(&id, &name, &description, &tags)
}

#[tauri::command]
fn library_bump_use(ids: Vec<String>, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let guard = library(&state)?;
    let lib = guard.as_ref().unwrap();
    lib.store().bump_use_counts(&ids)
}

#[tauri::command]
fn library_seed_builtins(state: tauri::State<'_, AppState>) -> Result<usize, String> {
    let guard = library(&state)?;
    let lib = guard.as_ref().unwrap();
    lib.seed_builtins()
}

// ---------------------------------------------------------------------------
// Phase 2: Grammar-Constrained Generation support
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct AiSchemaBundle {
    pub vehicle_schema: serde_json::Value,
    pub component_schema: serde_json::Value,
    pub patch_schema: serde_json::Value,
    pub plan_schema: serde_json::Value,
    pub gbnf_vehicle: String,
    pub gbnf_component: String,
    pub gbnf_patch: String,
    pub gbnf_plan: String,
    pub unimplemented_ops: Vec<String>,
}

#[tauri::command]
fn get_ai_schema() -> Result<AiSchemaBundle, String> {
    let g = |s: &serde_json::Value, root: &str| apro_grammar::generate_gbnf(s, root)
        .map_err(|e| format!("gbnf generation failed for {root}: {e}"));
    Ok(AiSchemaBundle {
        vehicle_schema: apro_grammar::schema_for_vehicle(),
        component_schema: apro_grammar::schema_for_component(),
        patch_schema: apro_grammar::schema_for_patch(),
        plan_schema: apro_grammar::schema_for_plan(),
        gbnf_vehicle: g(&apro_grammar::schema_for_vehicle(), "Vehicle")?,
        gbnf_component: g(&apro_grammar::schema_for_component(), "Component")?,
        gbnf_patch: g(&apro_grammar::schema_for_patch(), "Patch")?,
        gbnf_plan: g(&apro_grammar::schema_for_plan(), "Plan")?,
        unimplemented_ops: apro_grammar::UNIMPLEMENTED_OPS.iter().map(|s| s.to_string()).collect(),
    })
}

/// Deterministically convert model-produced JSON (constrained path) into RON.
/// Native Save-As dialog; writes the document and returns the chosen path
/// (None when the user cancels).
#[tauri::command]
fn save_document_dialog(
    default_name: String,
    contents: String,
) -> Result<Option<String>, String> {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("RON document", &["ron"])
        .set_file_name(&default_name)
        .save_file()
    else {
        return Ok(None);
    };
    std::fs::write(&path, contents).map_err(|e| format!("write failed: {e}"))?;
    Ok(Some(path.to_string_lossy().to_string()))
}

/// Native Save-As dialog that ONLY picks a path (caller does the writing).
#[tauri::command]
fn show_save_path_dialog(
    default_name: String,
    filter_name: String,
    extensions: Vec<String>,
) -> Result<Option<String>, String> {
    let mut dialog = rfd::FileDialog::new().set_file_name(&default_name);
    let exts: Vec<&str> = extensions.iter().map(|s| s.as_str()).collect();
    if !exts.is_empty() {
        dialog = dialog.add_filter(&filter_name, &exts);
    }
    Ok(dialog.save_file().map(|p| p.to_string_lossy().to_string()))
}

const B64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_decode(input: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0;
    for c in input.bytes() {
        if c == b'=' || c == b'\n' || c == b'\r' {
            if c == b'=' { break; }
            continue;
        }
        let v = B64_TABLE
            .iter()
            .position(|&t| t == c)
            .ok_or_else(|| "invalid base64".to_string())? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Ok(out)
}

/// Write a data-URL image (e.g. canvas screenshot) to disk.
/// Returns a status message with the written size.
#[tauri::command]
fn save_image_file(path: String, data_url: String) -> Result<String, String> {
    let Some(b64) = data_url.split(',').nth(1) else {
        return Err("invalid image data (no base64 payload)".into());
    };
    let bytes = b64_decode(b64)?;
    const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 8 || bytes[..8] != PNG_SIG[..] {
        // Never silently write non-image bytes under an image extension.
        return Err(format!(
            "payload is not a PNG image (got {} bytes, wrong magic)",
            bytes.len()
        ));
    }
    std::fs::write(&path, &bytes).map_err(|e| format!("write failed: {e}"))?;
    Ok(format!("{} ({} KB)", path, bytes.len() / 1024))
}

#[tauri::command]
fn json_to_ron(json_str: String) -> Result<String, String> {
    let trimmed = json_str.trim().trim_start_matches("```json").trim_end_matches("```").trim();
    // Vehicle, then Component, then Patch
    if let Ok(v) = serde_json::from_str::<apro_document::vehicle::Vehicle>(trimmed) {
        return to_named_ron(&v).map_err(|e| format!("json->ron failed: {e}"));
    }
    if let Ok(c) = serde_json::from_str::<apro_document::vehicle::Component>(trimmed) {
        return to_named_ron(&c).map_err(|e| format!("json->ron failed: {e}"));
    }
    if let Ok(p) = serde_json::from_str::<apro_document::patch::Patch>(trimmed) {
        return to_named_ron(&p).map_err(|e| format!("json->ron failed: {e}"));
    }
    Err("JSON did not match Vehicle, Component, or Patch".into())
}

/// The AI rulebook, baked into the binary at compile time. A release build has
/// no project tree next to it, so without this the assistant would silently
/// lose its schema reference (or, worse, pick up an unrelated `AI_INSTRUCTIONS.md`
/// found by walking up from the exe).
const AI_INSTRUCTIONS_EMBEDDED: &str = include_str!("../../AI_INSTRUCTIONS.md");

/// Read the AI rulebook. Prefers a file next to the executable (so a user can
/// edit it without a rebuild) and otherwise falls back to the embedded copy,
/// which is what makes the packaged .exe self-contained.
#[tauri::command]
fn read_ai_instructions(app_handle: tauri::AppHandle) -> Result<String, String> {
    if let Ok(dir) = app_handle.path().resource_dir() {
        // Only the executable's own directory is consulted. Walking up would
        // let a stray file in a parent folder silently replace the rulebook.
        let candidate = dir.join("AI_INSTRUCTIONS.md");
        if candidate.exists() {
            if let Ok(text) = std::fs::read_to_string(&candidate) {
                return Ok(text);
            }
        }
    }
    Ok(AI_INSTRUCTIONS_EMBEDDED.to_string())
}

// ---------------------------------------------------------------------------
// APRO Works platform bridge
// ---------------------------------------------------------------------------

/// What the UI needs to know to decide whether to offer a Publish button.
#[derive(Debug, Serialize)]
pub struct PlatformStatus {
    /// False when aproCAD was started on its own rather than by the hub. Not an error.
    pub connected: bool,
    pub endpoint: Option<String>,
    pub app_slug: String,
    /// A sentence to show the user verbatim.
    pub detail: String,
    /// The artifact type this app publishes.
    pub publishes: String,
}

/// Connect using the launch handshake, or explain why there is nothing to connect to.
///
/// The hub passes `--apro-product-slug`, `--apro-launch-token` and
/// `--apro-store-endpoint`. `from_launch_environment` returns `Ok(None)` when no
/// credential is present, which means "standalone" rather than "broken".
fn platform_client() -> Result<apro_cad_bridge::apro_client::HttpStoreClient, String> {
    match apro_cad_bridge::apro_client::HttpStoreClient::from_launch_environment() {
        Ok(Some(client)) => Ok(client),
        Ok(None) => Err(
            "aproCAD is not connected to APRO Works. Launch it from the hub to publish \
             mass properties."
                .into(),
        ),
        Err(err) => Err(format!("Could not reach the APRO Works store: {err}")),
    }
}

/// Report the connection state. Never fails, so the UI can always render something.
#[tauri::command]
async fn platform_status() -> Result<PlatformStatus, String> {
    use apro_cad_bridge::apro_client::{read_discovery, AproStoreClient, HttpStoreClient};

    let app_slug = apro_cad_bridge::APP_SLUG.to_string();
    let publishes = apro_cad_bridge::apro_contracts::MASS_PROPERTIES_TYPE.to_string();

    // The endpoint comes from the discovery file the hub writes, not from the health
    // payload: a store reports its data directory there, which is not where you reach it.
    let endpoint = read_discovery()
        .ok()
        .flatten()
        .map(|discovery| discovery.endpoint);

    match HttpStoreClient::from_launch_environment() {
        Ok(Some(client)) => {
            let detail = match client.health() {
                Ok(health) => format!(
                    "Connected to APRO Works (node {}, schema v{}).",
                    &health.node_id[..health.node_id.len().min(8)],
                    health.schema_version
                ),
                Err(err) => format!("Connected, but the store did not answer: {err}"),
            };
            Ok(PlatformStatus {
                connected: true,
                endpoint,
                app_slug,
                detail,
                publishes,
            })
        }
        Ok(None) => Ok(PlatformStatus {
            connected: false,
            endpoint,
            app_slug,
            publishes,
            detail: "Standalone. Launch aproCAD from APRO Works to publish and share \
                     this design."
                .into(),
        }),
        Err(err) => Ok(PlatformStatus {
            connected: false,
            endpoint,
            app_slug,
            publishes,
            detail: format!("Could not reach the APRO Works store: {err}"),
        }),
    }
}

/// Assign this design a stable identity, if it does not already have one.
///
/// Returns the updated document, because assigning an identity changes the saved file.
#[tauri::command]
async fn ensure_vehicle_uid(vehicle_ron: String) -> Result<String, String> {
    let mut vehicle: Vehicle =
        ron::from_str(&vehicle_ron).map_err(|e| format!("Failed to parse vehicle RON: {}", e))?;

    if vehicle.uid.is_some() {
        return Ok(vehicle_ron);
    }
    apro_cad_bridge::ensure_uid(&mut vehicle);
    to_named_ron(&vehicle).map_err(|e| format!("Failed to serialize: {}", e))
}

/// The result of a publish, in the shape the UI wants to render.
#[derive(Debug, Serialize)]
pub struct PublishReport {
    pub instance: String,
    pub type_id: String,
    pub revision_number: u32,
    pub content_hash: String,
    pub byte_size: u64,
    /// Publishing an unchanged design is a no-op rather than an error.
    pub unchanged: bool,
    /// Set when this publish had to mint an identity for the design. The UI must persist
    /// `updated_vehicle_ron` or the next publish would mint a *different* one.
    pub assigned_uid: Option<String>,
    pub updated_vehicle_ron: Option<String>,
    /// A sentence to show the user verbatim.
    pub summary: String,
}

/// Publish this design's mass properties to APRO Works.
///
/// The vehicle is re-evaluated here rather than trusting numbers the frontend is holding:
/// the engine is cache-warm straight after a UI evaluation, so this is cheap, and it
/// removes a whole class of "published what was on screen a minute ago" bugs.
///
/// `datum_offset_mm` is where the CAD origin sits in the consumer's body datum frame, in
/// the document's own units. Omitted means the two frames coincide — stated explicitly,
/// not assumed.
#[tauri::command]
async fn publish_mass_properties(
    vehicle_ron: String,
    datum_offset_mm: Option<[f64; 3]>,
    label: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<PublishReport, String> {
    let client = platform_client()?;

    let mut vehicle: Vehicle =
        ron::from_str(&vehicle_ron).map_err(|e| format!("Failed to parse vehicle RON: {}", e))?;

    // Mint an identity if the document has none, and hand the updated document back so
    // the caller can save it. Falling back to `name` instead would produce a different
    // artifact every time the design is renamed.
    let assigned_uid = if vehicle.uid.is_none() {
        Some(apro_cad_bridge::ensure_uid(&mut vehicle))
    } else {
        None
    };

    let components: Vec<ComponentMesh> = {
        let mut engine = state.engine.lock().map_err(|e| format!("Lock error: {}", e))?;
        engine.evaluate_vehicle_with_progress(&vehicle, |_, _, _, _| {});
        let vehicle = &vehicle;
        engine
            .component_meshes(vehicle)
            .into_iter()
            .map(|(name, material, color, m)| {
                let visible = vehicle
                    .components
                    .iter()
                    .find(|c| c.name == name)
                    .map(|c| c.visible)
                    .unwrap_or(true);
                ComponentMesh {
                    name,
                    material,
                    color,
                    visible,
                    positions: m.positions,
                    normals: m.normals,
                    indices: m.indices,
                }
            })
            .collect()
    };

    let mass = combine_component_mass_props(&components).ok_or_else(|| {
        "Nothing to publish: no component produced a mesh. Evaluate the design first."
            .to_string()
    })?;

    let datum = match datum_offset_mm {
        Some(offset) => apro_cad_bridge::DatumOffset::from_document_units(offset, &vehicle.units),
        None => apro_cad_bridge::DatumOffset::coincident(),
    };

    // Reference geometry is derived from the assembly's bounding box, and the derivation
    // records the convention it used so the consumer can disagree with it.
    let reference = assembly_extent(&components).map(|extent| {
        apro_cad_bridge::reference_geometry_from_extent(extent, LONGITUDINAL_AXIS, &vehicle.units)
    });

    let outcome = apro_cad_bridge::publish(
        &client,
        apro_cad_bridge::PublishRequest {
            vehicle: &vehicle,
            mass: &mass,
            datum,
            reference,
            label: label.as_deref(),
        },
    )
    .map_err(|err| err.to_string())?;

    let updated_vehicle_ron = match &assigned_uid {
        Some(_) => Some(to_named_ron(&vehicle).map_err(|e| format!("Failed to serialize: {}", e))?),
        None => None,
    };

    let mut summary = outcome.summary();
    if let Some(uid) = &assigned_uid {
        summary.push_str(&format!(
            ". Assigned this design the identity {uid} — save the document or the next \
             publish will mint a different one."
        ));
    }

    Ok(PublishReport {
        instance: outcome.instance,
        type_id: outcome.type_id,
        revision_number: outcome.revision_number,
        content_hash: outcome.content_hash,
        byte_size: outcome.byte_size,
        unchanged: outcome.unchanged,
        assigned_uid,
        updated_vehicle_ron,
        summary,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![evaluate, evaluate_vehicle, validate, describe_vehicle, describe_parameters, modify_property, export_step, export_stl, apply_patch_vehicle, apply_patch_ron, read_ai_instructions, library_save, library_save_ron, library_retrieve, library_list, library_delete, library_update, library_bump_use, library_seed_builtins, get_ai_schema, json_to_ron, save_document_dialog, show_save_path_dialog, save_image_file, check_interferences, needle_run, get_needle_schema, pick_file_dialog, platform_status, ensure_vehicle_uid, publish_mass_properties])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
