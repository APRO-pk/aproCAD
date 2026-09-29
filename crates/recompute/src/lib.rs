use std::collections::HashMap;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use apro_document::params::{ParamEnv, resolve_parameters};
use apro_document::vehicle::{Component, ComponentKind, SolidOp, SolidRef, Vehicle};
use apro_features::eval::evaluate_solid_ops;
use apro_features::shorthands::{nosecone_to_ops, bodytube_to_ops, transition_to_ops, tank_to_ops, nozzle_to_ops};
use apro_features::fin::mesh_finset;
use apro_features::vehicle::try_build_component_mesh;
use apro_kernel::{transform_mesh_srt, MeshData};

struct CacheEntry {
    fingerprint: u64,
    mesh: MeshData,
}

pub struct RecomputeEngine {
    cache: HashMap<String, CacheEntry>,
    hits: u32,
    misses: u32,
}

fn fingerprint_ron(ron_str: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    ron_str.hash(&mut hasher);
    hasher.finish()
}

fn component_fingerprint(comp: &Component) -> u64 {
    let full_ron = ron::to_string(&(
        comp.name.as_str(),
        &comp.material,
        &comp.transform,
        &comp.kind,
    )).unwrap_or_default();
    fingerprint_ron(&full_ron)
}

/// Names of sibling components this component references via Boolean ops.
fn boolean_deps(comp: &Component) -> Vec<String> {
    match &comp.kind {
        ComponentKind::Solid(ops) => ops.iter().filter_map(|op| match op {
            SolidOp::Boolean { target: SolidRef::Component(name), .. } => Some(name.clone()),
            _ => None,
        }).collect(),
        _ => Vec::new(),
    }
}

/// Kahn's algorithm: components that depend on siblings must be evaluated after
/// their dependencies. Errors on a circular dependency chain.
fn topo_order(vehicle: &Vehicle) -> Result<Vec<usize>, String> {
    let n = vehicle.components.len();
    let mut name_index: HashMap<&str, usize> = HashMap::new();
    for (i, c) in vehicle.components.iter().enumerate() {
        name_index.insert(c.name.as_str(), i);
    }
    let mut indeg = vec![0usize; n];
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, c) in vehicle.components.iter().enumerate() {
        for dep in boolean_deps(c) {
            if let Some(&j) = name_index.get(dep.as_str()) {
                adj[j].push(i);
                indeg[i] += 1;
            }
        }
    }
    let mut queue: VecDeque<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    let mut order = Vec::with_capacity(n);
    while let Some(i) = queue.pop_front() {
        order.push(i);
        for &j in &adj[i] {
            indeg[j] -= 1;
            if indeg[j] == 0 {
                queue.push_back(j);
            }
        }
    }
    if order.len() == n {
        Ok(order)
    } else {
        Err("circular boolean dependency between components".into())
    }
}

/// FNV-1a-style fold of dependency fingerprints, so renames and reorders both
/// change the result and identical deps never cancel each other out.
fn combine_dep_fp(dep_fp: &mut u64, dep_name: &str, dep_tfp: u64) {
    *dep_fp = dep_fp.wrapping_mul(0x100000001b3).wrapping_add(dep_tfp ^ fingerprint_ron(dep_name));
}

fn build_mesh(comp: &Component) -> Option<MeshData> {
    let ops = match &comp.kind {
        ComponentKind::NoseCone(p) => Some(nosecone_to_ops(p)),
        ComponentKind::BodyTube(p) => Some(bodytube_to_ops(p)),
        ComponentKind::Transition(p) => Some(transition_to_ops(p)),
        ComponentKind::Tank(p) => Some(tank_to_ops(p)),
        ComponentKind::Nozzle(p) => Some(nozzle_to_ops(p)),
        ComponentKind::FinSet(p) => return Some(mesh_finset(p)),
        ComponentKind::Solid(ops) => return evaluate_solid_ops(ops).ok(),
        // A sketch is construction geometry and produces no mesh. Solid
        // components that reference one are evaluated through the vehicle
        // builder, which has the sibling registry this path lacks.
        ComponentKind::Sketch(_) => return None,
    };
    match ops {
        Some(op_list) => evaluate_solid_ops(&op_list).ok(),
        None => None,
    }
}

impl RecomputeEngine {
    pub fn new() -> Self {
        RecomputeEngine { cache: HashMap::new(), hits: 0, misses: 0 }
    }

    pub fn evaluate_component(&mut self, comp: &Component) -> Option<&MeshData> {
        let fp = component_fingerprint(comp);
        let is_hit = self.cache.get(&comp.name).map_or(false, |e| e.fingerprint == fp);
        if is_hit {
            self.hits += 1;
            return self.cache.get(&comp.name).map(|e| &e.mesh);
        }
        let mesh = match build_mesh(comp) {
            Some(m) => m,
            None => return None,
        };
        self.misses += 1;
        self.cache.insert(comp.name.clone(), CacheEntry { fingerprint: fp, mesh });
        self.cache.get(&comp.name).map(|e| &e.mesh)
    }

    pub fn evaluate_vehicle(&mut self, vehicle: &Vehicle) -> (MeshData, u32, u32) {
        self.evaluate_vehicle_with_progress(vehicle, |_, _, _, _| {})
    }

    /// Same as [`evaluate_vehicle`] but reports two-phase progress through
    /// `on_progress(phase, done, total_steps, component_name)`:
    ///   phase "tessellate" — per-component mesh build (skipped on cache hits)
    ///   phase "assemble"   — per-component transform + merge (always runs)
    /// `total_steps` is 2× the component count so the frontend bar advances
    /// meaningfully even on fully-cached re-evaluations.
    pub fn evaluate_vehicle_with_progress(
        &mut self,
        vehicle: &Vehicle,
        mut on_progress: impl FnMut(&str, usize, usize, &str),
    ) -> (MeshData, u32, u32) {
        let order = match topo_order(vehicle) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("[warn] {e}");
                return (MeshData { positions: vec![], normals: vec![], indices: vec![] }, self.hits, self.misses);
            }
        };

        // Resolve the parameter block once; a parameter edit invalidates every
        // component because any profile/expression may reference it.
        let params = match &vehicle.parameters {
            Some(ps) => match resolve_parameters(ps) {
                Ok(resolved) => resolved,
                Err(e) => {
                    eprintln!("[warn] Parameters: {e}");
                    Vec::new()
                }
            },
            None => Vec::new(),
        };
        let param_fp = fingerprint_ron(&ron::to_string(&params).unwrap_or_default());
        let param_env: ParamEnv = params.into_iter().collect();

        let own_fps: Vec<u64> = vehicle.components.iter().map(component_fingerprint).collect();
        let n = vehicle.components.len();

        // Transitive fingerprints in topological order: a component's fingerprint
        // folds in the fingerprints of everything it depends on plus the resolved
        // parameter values, so a parameter or dependency change invalidates the
        // dependent's cache entry too.
        let mut tfps: Vec<u64> = vec![0; n];
        let mut tfp_index: HashMap<String, u64> = HashMap::new();
        for &i in &order {
            let mut dep_fp = 0x9E3779B97F4A7C15u64;
            combine_dep_fp(&mut dep_fp, "_params", param_fp);
            for dep in boolean_deps(&vehicle.components[i]) {
                let d = tfp_index.get(&dep).copied().unwrap_or_else(|| fingerprint_ron(&dep));
                combine_dep_fp(&mut dep_fp, &dep, d);
            }
            let tfp = own_fps[i] ^ dep_fp;
            tfps[i] = tfp;
            tfp_index.insert(vehicle.components[i].name.clone(), tfp);
        }

        let total_steps = order.len() * 2;
        for (ord_idx, &i) in order.iter().enumerate() {
            let comp = &vehicle.components[i];
            let is_hit = self.cache.get(&comp.name).map_or(false, |e| e.fingerprint == tfps[i]);
            if is_hit {
                self.hits += 1;
                on_progress("tessellate", ord_idx + 1, total_steps, &comp.name);
                continue;
            }
            let mesh = match self.build_vehicle_component(vehicle, comp, &param_env) {
                Some(m) => m,
                None => {
                    on_progress("tessellate", ord_idx + 1, total_steps, &comp.name);
                    continue;
                }
            };
            self.misses += 1;
            self.cache.insert(comp.name.clone(), CacheEntry { fingerprint: tfps[i], mesh });
            on_progress("tessellate", ord_idx + 1, total_steps, &comp.name);
        }

        let mut all_positions = Vec::new();
        let mut all_normals = Vec::new();
        let mut all_indices = Vec::new();
        for (mi, comp) in vehicle.components.iter().enumerate() {
            if let Some(entry) = self.cache.get(&comp.name) {
                let s = comp.transform.scale.unwrap_or((1.0, 1.0, 1.0));
                let mesh_with_transform = transform_mesh_srt(
                    &entry.mesh,
                    &[comp.transform.position.0, comp.transform.position.1, comp.transform.position.2],
                    &[comp.transform.rotation.0, comp.transform.rotation.1, comp.transform.rotation.2],
                    &[s.0, s.1, s.2],
                );
                let base = all_positions.len() as u32 / 3;
                all_positions.extend_from_slice(&mesh_with_transform.positions);
                all_normals.extend_from_slice(&mesh_with_transform.normals);
                for &idx in &mesh_with_transform.indices {
                    all_indices.push(base + idx);
                }
                on_progress("assemble", order.len() + mi + 1, total_steps, &comp.name);
            }
        }

        (MeshData { positions: all_positions, normals: all_normals, indices: all_indices }, self.hits, self.misses)
    }

    /// Build one component's local mesh with Boolean targets resolved from the
    /// engine cache (dependencies are guaranteed built first by topo order).
    fn build_vehicle_component(
        &mut self,
        vehicle: &Vehicle,
        comp: &Component,
        params: &ParamEnv,
    ) -> Option<MeshData> {
        let comp_name = comp.name.clone();
        let mut resolver = |name: &str| -> Result<MeshData, String> {
            if name == comp_name {
                return Err(format!("circular boolean reference: component '{name}' depends on itself"));
            }
            match self.cache.get(name) {
                Some(e) => Ok(e.mesh.clone()),
                None => Err(format!("boolean target component '{name}' not yet available")),
            }
        };
        let mesh = try_build_component_mesh(vehicle, comp, params, &mut resolver).ok()?;
        if mesh.positions.is_empty() {
            return None;
        }
        Some(mesh)
    }

    /// Per-component meshes (already transformed to assembly space) taken from
    /// the engine cache, in component order. Used by the frontend to render
    /// each component in its own color. Components absent from the cache are
    /// skipped (a failed evaluation yields no mesh for them).
    pub fn component_meshes(&self, vehicle: &Vehicle) -> Vec<(String, String, Option<String>, MeshData)> {
        let mut out = Vec::with_capacity(vehicle.components.len());
        for comp in &vehicle.components {
            let Some(entry) = self.cache.get(&comp.name) else {
                continue;
            };
            if entry.mesh.positions.is_empty() {
                continue;
            }
            let s = comp.transform.scale.unwrap_or((1.0, 1.0, 1.0));
            let transformed = transform_mesh_srt(
                &entry.mesh,
                &[comp.transform.position.0, comp.transform.position.1, comp.transform.position.2],
                &[comp.transform.rotation.0, comp.transform.rotation.1, comp.transform.rotation.2],
                &[s.0, s.1, s.2],
            );
            out.push((comp.name.clone(), comp.material_name(), comp.color.clone(), transformed));
        }
        out
    }

    pub fn invalidate(&mut self, name: &str) {
        self.cache.remove(name);
    }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.hits = 0;
        self.misses = 0;
    }

    pub fn stats(&self) -> (u32, u32) {
        (self.hits, self.misses)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apro_document::vehicle::*;

    fn make_nosecone() -> Component {
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
        }
    }

    #[test]
    fn test_cache_hit_on_repeat() {
        let mut engine = RecomputeEngine::new();
        let comp = make_nosecone();
        let _ = engine.evaluate_component(&comp);
        let (hits1, misses1) = engine.stats();
        assert_eq!(hits1, 0);
        assert_eq!(misses1, 1);
        let _ = engine.evaluate_component(&comp);
        let (hits2, misses2) = engine.stats();
        assert_eq!(hits2, 1);
        assert_eq!(misses2, 1);
    }

    #[test]
    fn test_cache_miss_on_change() {
        let mut engine = RecomputeEngine::new();
        let comp = make_nosecone();
        let _ = engine.evaluate_component(&comp);
        let mut comp2 = make_nosecone();
        if let ComponentKind::NoseCone(ref mut p) = comp2.kind {
            p.length = 999.0;
        }
        let _ = engine.evaluate_component(&comp2);
        let (hits, misses) = engine.stats();
        assert_eq!(hits, 0);
        assert_eq!(misses, 2);
    }

    #[test]
    fn test_vehicle_assembly() {
        let mut engine = RecomputeEngine::new();
        let vehicle = Vehicle { parameters: None,
            name: "Test".into(),
            units: Units::Millimeters,
            components: vec![make_nosecone()],
        };
        let (mesh, hits, misses) = engine.evaluate_vehicle(&vehicle);
        assert!(mesh.positions.len() >= 9);
        assert_eq!(hits, 0);
        assert_eq!(misses, 1);
        let (_, hits2, misses2) = engine.evaluate_vehicle(&vehicle);
        assert_eq!(hits2, 1);
        assert_eq!(misses2, 1);
    }

    #[test]
    fn test_invalidate() {
        let mut engine = RecomputeEngine::new();
        let comp = make_nosecone();
        let _ = engine.evaluate_component(&comp);
        engine.invalidate("Nose");
        let _ = engine.evaluate_component(&comp);
        let (hits, misses) = engine.stats();
        assert_eq!(hits, 0);
        assert_eq!(misses, 2);
    }

    /// A cylindrical bore, used as the Difference target for a block so the
    /// result is a block with an open hole (non-empty geometry).
    fn make_bore(name: &str, height: f64) -> Component {
        Component {
            name: name.into(),
            material: "Al-6061-T6".into(),
            visible: true,
            transform: Transform::default(),
            color: None,
            kind: ComponentKind::Solid(vec![
                SolidOp::Extrude {
                    profile: Profile::Circle { radius: 10.0 },
                    height,
                    direction: None,
                    taper: None,
                },
            ]),
        }
    }

    fn make_block_with_boolean(name: &str, target: &str, kind: BooleanKind) -> Component {
        Component {
            name: name.into(),
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
                    kind,
                    target: SolidRef::Component(target.into()),
                },
            ]),
        }
    }

    #[test]
    fn test_vehicle_boolean_dependency_order() {
        // "Cut" must be built before "Base" because Base differences it out.
        let vehicle = Vehicle { parameters: None,
            name: "Dep".into(),
            units: Units::Millimeters,
            components: vec![
                make_block_with_boolean("Base", "Cut", BooleanKind::Difference),
                make_bore("Cut", 20.0),
            ],
        };
        let mut engine = RecomputeEngine::new();
        let (mesh, hits, misses) = engine.evaluate_vehicle(&vehicle);
        assert_eq!(hits, 0);
        assert_eq!(misses, 2, "both components must build");
        assert!(mesh.positions.len() >= 9, "expected base + cutter geometry");
        // The base top face hole (r=10) must be open.
        let center_filled = mesh.positions.chunks(3).any(|p| {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            (p[2] - 20.0).abs() < 1.0 && r < 9.0
        });
        assert!(!center_filled, "base top face hole must be open (r<9 removed)");
        // Second pass: everything cached.
        let (_, hits2, misses2) = engine.evaluate_vehicle(&vehicle);
        assert_eq!(hits2, 2);
        assert_eq!(misses2, 2);
    }

    #[test]
    fn test_transitive_fingerprint_invalidates_dependent() {
        // Base differences out Cut. Changing Cut's bore height must invalidate
        // Base's cache entry even though Base's own RON is unchanged.
        let vehicle = |cut_height: f64| Vehicle { parameters: None,
            name: "Dep".into(),
            units: Units::Millimeters,
            components: vec![
                make_block_with_boolean("Base", "Cut", BooleanKind::Difference),
                make_bore("Cut", cut_height),
            ],
        };
        let mut engine = RecomputeEngine::new();
        let _ = engine.evaluate_vehicle(&vehicle(20.0));
        let (hits, _) = engine.stats();
        assert_eq!(hits, 0);
        // Same vehicle again: both cache hits (cumulative hits = 2).
        let (_, hits2, _) = engine.evaluate_vehicle(&vehicle(20.0));
        assert_eq!(hits2, 2);
        // Cut height changes: both Cut and Base miss (hits stay flat at 2),
        // and both rebuild (cumulative misses rise from 2 to 4).
        let (_, hits3, misses3) = engine.evaluate_vehicle(&vehicle(30.0));
        assert_eq!(hits3, 2, "dependent Base must be invalidated by Cut change");
        assert_eq!(misses3, 4);
    }

    #[test]
    fn test_circular_boolean_dependency_returns_empty() {
        let vehicle = Vehicle { parameters: None,
            name: "Circ".into(),
            units: Units::Millimeters,
            components: vec![
                make_block_with_boolean("A", "B", BooleanKind::Difference),
                make_block_with_boolean("B", "A", BooleanKind::Union),
            ],
        };
        let mut engine = RecomputeEngine::new();
        let (mesh, _, _) = engine.evaluate_vehicle(&vehicle);
        assert!(mesh.positions.is_empty(), "circular dependency must yield no geometry");
    }
}
