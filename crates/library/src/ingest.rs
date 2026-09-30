//! Ingestion: save a component from the current design into the library,
//! extracting numeric params + mass props, embedding, and inserting.

use crate::db::LibraryStore;
use crate::types::{EntryKind, LibraryEntry, ParamVector, Source};
use apro_document::material::density_kg_per_mm3;
use apro_document::vehicle::{Component, ComponentKind, Profile, SolidOp, Vehicle};
use apro_embed::Embedder;
use apro_features::vehicle::build_vehicle_mesh;
use apro_massprops::compute_mass_properties;
use ulid::Ulid;

pub struct IngestInput {
    pub vehicle_ron: String,
    pub component_name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub source: Source,
}

/// Serialize a single component as standalone RON.
pub fn component_ron(comp: &Component) -> Result<String, String> {
    ron::to_string(comp).map_err(|e| format!("cannot serialize component: {e}"))
}

/// Compute mass properties for a single component via a single-component vehicle.
pub fn component_mass_props(comp: &Component) -> Result<apro_massprops::MassProperties, String> {
    let vehicle = Vehicle { uid: None, parameters: None,
        name: "library".into(),
        units: apro_document::vehicle::Units::Millimeters,
        components: vec![comp.clone()],
    };
    let mesh = build_vehicle_mesh(&vehicle);
    if mesh.positions.is_empty() {
        return Err("component produced no mesh".into());
    }
    let density = density_kg_per_mm3(&comp.material_name());
    Ok(compute_mass_properties(&mesh, density))
}

/// Extract the numeric parameter vector for an entry from its kind.
pub fn extract_params(comp: &Component, mass: Option<f64>) -> ParamVector {
    let kind = match &comp.kind {
        ComponentKind::NoseCone(p) => {
            let mut v = ParamVector {
                od_mm: Some(p.base_radius * 2.0),
                length_mm: Some(p.length),
                material: Some(comp.material_name()),
                ..Default::default()
            };
            v.custom.insert("profile".into(), profile_code(&p.profile));
            v
        }
        ComponentKind::BodyTube(p) => ParamVector {
            od_mm: Some(p.radius * 2.0),
            id_mm: Some((p.radius - p.wall) * 2.0),
            length_mm: Some(p.length),
            material: Some(comp.material_name()),
            ..Default::default()
        },
        ComponentKind::Transition(p) => {
            let max_r = p.start_radius.max(p.end_radius);
            ParamVector {
                od_mm: Some(max_r * 2.0),
                length_mm: Some(p.length),
                material: Some(comp.material_name()),
                ..Default::default()
            }
        }
        ComponentKind::Tank(p) => ParamVector {
            od_mm: Some(p.radius * 2.0),
            length_mm: Some(p.cylindrical_length + p.radius),
            material: Some(comp.material_name()),
            ..Default::default()
        },
        ComponentKind::Nozzle(p) => ParamVector {
            od_mm: Some(p.chamber_radius * 2.0),
            throat_mm: Some(p.throat_radius * 2.0),
            expansion_ratio: Some(p.expansion_ratio),
            length_mm: Some(nozzle_length_est(p.expansion_ratio, p.throat_radius)),
            material: Some(comp.material_name()),
            ..Default::default()
        },
        ComponentKind::FinSet(p) => ParamVector {
            length_mm: Some(p.root_chord),
            material: Some(comp.material_name()),
            ..Default::default()
        },
        ComponentKind::Solid(ops) => {
            let (od, len) = solid_extents(ops);
            ParamVector {
                od_mm: od,
                length_mm: len,
                material: Some(comp.material_name()),
                ..Default::default()
            }
        }
        // Sketches carry no solid dimensions; their entities are described by
        // the sketch itself rather than by an OD/length pair.
        ComponentKind::Sketch(_) => ParamVector {
            material: Some(comp.material_name()),
            ..Default::default()
        },
    };
    let mut v = kind;
    v.mass_g = mass;
    v
}

fn profile_code(p: &apro_document::vehicle::NoseConeProfile) -> f64 {
    match p {
        apro_document::vehicle::NoseConeProfile::Conical => 0.0,
        apro_document::vehicle::NoseConeProfile::Ogive => 1.0,
        apro_document::vehicle::NoseConeProfile::VonKarman => 2.0,
        apro_document::vehicle::NoseConeProfile::Haack { c } => 3.0 + *c,
        apro_document::vehicle::NoseConeProfile::Power { n } => 4.0 + *n,
        apro_document::vehicle::NoseConeProfile::Parabolic { k } => 5.0 + *k,
    }
}

fn nozzle_length_est(er: f64, throat_r: f64) -> f64 {
    // Conical nozzle length ≈ throat_r * (sqrt(er) - 1) / tan(15deg)
    let exit_r = throat_r * er.sqrt();
    ((exit_r - throat_r) / 0.268).max(10.0)
}

fn solid_extents(ops: &[SolidOp]) -> (Option<f64>, Option<f64>) {
    let mut max_r = 0.0f64;
    let mut max_x = 0.0f64;
    for op in ops {
        match op {
            SolidOp::Revolve { profile, .. } | SolidOp::Extrude { profile, .. } => {
                if let Profile::Points(pts) = profile {
                    for [x, r] in pts {
                        max_r = max_r.max(*r);
                        max_x = max_x.max(*x);
                    }
                }
            }
            SolidOp::RevolveChain { segments, .. } => {
                for seg in segments {
                    for [x, r] in seg {
                        max_r = max_r.max(*r);
                        max_x = max_x.max(*x);
                    }
                }
            }
            SolidOp::Loft { profiles, .. } => {
                for prof in profiles {
                    if let Profile::Points(pts) = prof {
                        for [x, r] in pts {
                            max_r = max_r.max(*r);
                            max_x = max_x.max(*x);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (
        if max_r > 0.0 { Some(max_r * 2.0) } else { None },
        if max_x > 0.0 { Some(max_x) } else { None },
    )
}

pub fn kind_of(comp: &Component) -> EntryKind {
    match &comp.kind {
        ComponentKind::NoseCone(_) => EntryKind::NoseCone,
        ComponentKind::BodyTube(_) => EntryKind::BodyTube,
        ComponentKind::Transition(_) => EntryKind::Transition,
        ComponentKind::Tank(_) => EntryKind::Tank,
        ComponentKind::Nozzle(_) => EntryKind::Nozzle,
        ComponentKind::FinSet(_) => EntryKind::FinSet,
        ComponentKind::Solid(_) => EntryKind::Solid,
        ComponentKind::Sketch(_) => EntryKind::Sketch,
    }
}

/// Save a component from a vehicle RON into the library. Returns the entry id.
pub fn save_component(
    store: &LibraryStore,
    embedder: &dyn Embedder,
    input: &IngestInput,
) -> Result<String, String> {
    let vehicle: Vehicle = ron::from_str(&input.vehicle_ron)
        .map_err(|e| format!("failed to parse vehicle RON: {e}"))?;
    let comp = vehicle
        .components
        .iter()
        .find(|c| c.name == input.component_name)
        .ok_or_else(|| format!("component '{}' not found", input.component_name))?;

    let ron = component_ron(comp)?;
    let mass_props = component_mass_props(comp)?;
    let mut params = extract_params(comp, Some(mass_props.mass));
    if params.material.is_none() {
        params.material = Some(comp.material_name());
    }

    let id = Ulid::generate().to_string();
    let entry = LibraryEntry {
        id: id.clone(),
        name: comp.name.clone(),
        description: input.description.clone(),
        tags: input.tags.clone(),
        kind: kind_of(comp),
        ron,
        params,
        embedding: None,
        mass_props: Some(mass_props),
        source: input.source,
        created: chrono::Utc::now().timestamp(),
        use_count: 0,
    };

    let emb = embedder.embed(&entry.embed_text());
    store.insert(&entry)?;
    if !emb.is_empty() {
        store.update_embedding(&id, &emb)?;
    }
    Ok(id)
}

/// Save a raw single-component RON (e.g. from the AI or builtins).
pub fn save_component_ron(
    store: &LibraryStore,
    embedder: &dyn Embedder,
    name: String,
    description: String,
    tags: Vec<String>,
    component_ron: &str,
    source: Source,
) -> Result<String, String> {
    let comp: Component = ron::from_str(component_ron)
        .map_err(|e| format!("failed to parse component RON: {e}"))?;
    let mass_props = component_mass_props(&comp)?;
    let params = extract_params(&comp, Some(mass_props.mass));

    let id = Ulid::generate().to_string();
    let entry = LibraryEntry {
        id: id.clone(),
        name: if name.is_empty() { comp.name.clone() } else { name },
        description,
        tags,
        kind: kind_of(&comp),
        ron: component_ron.to_string(),
        params,
        embedding: None,
        mass_props: Some(mass_props),
        source,
        created: chrono::Utc::now().timestamp(),
        use_count: 0,
    };
    let emb = embedder.embed(&entry.embed_text());
    store.insert(&entry)?;
    if !emb.is_empty() {
        store.update_embedding(&id, &emb)?;
    }
    Ok(id)
}
