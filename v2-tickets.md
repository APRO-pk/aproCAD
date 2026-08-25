# APRO CAD v2 — Implementation Tickets

## Phase 1: Core type system (document crate)
- [x] 1.1 Define `SolidOp` enum (Revolve, Extrude, Loft, Sweep, Shell, Boolean, Fillet, Chamfer, Transform)
- [x] 1.2 Define `Profile` enum (Points, Circle, Rectangle, Polygon, Reference, UserFunction)
- [x] 1.3 Define `Path3D` enum (Line, Arc, Spline, Helix)
- [x] 1.4 Define `Transform` struct (position: Vec3, rotation: Vec3), update `Component` to use it
- [x] 1.5 Add `Solid(Vec<SolidOp>)` variant to `ComponentKind`; demote existing variants to shorthands
- [x] 1.6 Define `SolidRef`, `FaceRef`, `EdgeRef` enums
- [x] 1.7 Update validation to accept `Solid` kind
- [x] 1.8 Update `describe_component` to flatten SolidOp tree into property table
- [x] 1.9 Update patch module `SetProperty` to handle nested SolidOp paths
- [x] 1.10 Ensure all types derive `Serialize, Deserialize, Debug, Clone, PartialEq`

## Phase 2: Kernel operations (kernel crate)
- [x] 2.1 `kernel::revolve()` — full angle support, dedup profile, proper vertex sharing
- [x] 2.2 `kernel::extrude()` — tsweep with closed planar profile face
- [x] 2.3 `kernel::loft_mesh()` — direct mesh generation (bypasses B-Rep stitching). B-Rep `loft()` remains blocked (Truck 0.6 homotopy + caps don't stitch)
- [x] 2.4 `kernel::sweep()` — works for single-segment path (multi-segment blocked by no boolean union)
- [ ] 2.5 `kernel::shell()` — not exposed in Truck 0.6 builder API; manual mesh offset possible
- [ ] 2.6 `kernel::boolean()` — no boolean API in Truck 0.6; mesh-based CSG possible
- [ ] 2.7 `kernel::fillet()` / `kernel::chamfer()` — not available in Truck 0.6
- [x] 2.8 `kernel::tessellate()` — alias for `solid_to_meshdata`
- [x] 2.9 `kernel::transform_mesh()` — Euler XYZ rotation + translation on vertex/normal buffers
- [x] 2.10 16 unit tests passing (revolve 2, extrude 2, loft_mesh 3, sweep 0, transform 2, error cases 2, not-yet 2, loft B-Rep 1 IGNORED)

## Phase 3: Operation stack evaluator (features crate)
- [x] 3.1 `evaluate_solid_ops(ops: &[SolidOp]) -> Result<MeshData>` — evaluates ops sequentially, tessellating each Solid-producing op to MeshData immediately (mesh-only pipeline)
- [x] 3.2 Profile expansion: `expand_profile(profile, samples) -> Vec<[f64; 2]>` — handles Points, Circle (sampled), Rectangle (4-point or rounded), Polygon (N-gon), Reference (error), UserFunction (error)
- [x] 3.3 Path3D conversion: `convert_path(path, samples) -> Vec<[f64; 3]>` — handles Line, Arc, Spline, Helix
- [ ] 3.4 Boolean reference resolution — requires boolean kernel support
- [x] 3.5 Wired into `build_component_mesh(ComponentKind::Solid(ops))` — errors print warning, return empty mesh
- [x] 3.6 19 unit tests covering: profile expansion (4), path conversion (4), revolve/extrude/loft/transform eval (6), error cases (4), benchmark (1)
- [x] 3.7 Axis/Direction handling: revolve supports X/Y/Z axis; extrude supports 6 directions + taper via loft_mesh
- [x] 3.8 `TransformOp` applies mesh-level translate/rotate (no scale yet) — redundant with Phase 5 transform_mesh

## Phase 4: Aerospace shorthand → SolidOp expansion (features crate)
- [x] 4.1 `nosecone_to_ops(...) -> Vec<SolidOp>` — expand any NoseCone kind
- [x] 4.2 `bodytube_to_ops(...) -> Vec<SolidOp>`
- [x] 4.3 `transition_to_ops(...) -> Vec<SolidOp>`
- [x] 4.4 `tank_to_ops(...) -> Vec<SolidOp>`
- [x] 4.5 `nozzle_to_ops(...) -> Vec<SolidOp>`
- [x] 4.6 `finset_to_ops(...) -> Vec<SolidOp>` (returns error — fins are mesh-only, not representable as revolve/extrude ops)
- [x] 4.7 Route all aerospace kinds through `build_component_mesh` via shorthands → evaluator pipeline (existing `BuildSolid` + `build()` kept for backward compat in tests)
- [x] 4.8 All 108 tests pass
- [x] 4.9 Remove `axial_offset` field entirely (now dead code)
- [x] 4.10 Remove old `kernel::revolve.rs` (all builders now use `kernel::ops::revolve`)

## Phase 5: Transform system (features + kernel + recompute crates)
- [x] 5.1 `transform_mesh()` in kernel (Phase 2.9)
- [x] 5.2 `build_vehicle_mesh` applies full 6-DOF transform (position xyz + rotation xyz) via `transform_mesh()`
- [x] 5.3 RecomputeEngine fingerprints include transform (was already done)
- [x] 5.4 `describe_vehicle` returns transform fields (was already done)
- [x] 5.5 Frontend property table for position/rotation editing (Phase 6.6 — transform fields editable via double-click)
- [x] 5.6 Presets include transforms (Phase 1)
- [x] 5.7 Tests: 3 transform tests pass (offset, rotation, multi-component)
- [x] 5.8 `evaluate` (single-component Tauri command) now routes through `mesh_component` — handles Solid kind
- [x] 5.9 `export_stl` now routes through `mesh_component` — handles Solid kind
- [x] 5.10 RecomputeEngine `build_mesh` now routes through shorthands + evaluator — handles Solid kind
- [x] 5.11 RecomputeEngine `evaluate_vehicle` applies full 6-DOF transform via `transform_mesh()`
- [x] 5.12 RecomputeEngine no longer panics on build failure (filter_map skips failed components)
- [x] 5.13 `describe_kind` now decomposes Solid ops into per-field rows (profile, angle, height, etc.) with editable numeric fields
- [x] 5.14 `modify_component_kind` now handles Solid op field keys (`op_{index}_{field}`) — supports profile (Points) + angle/height
- [ ] 5.15 `export_step` still uses `BuildSolid` for B-Rep preservation (Solid kind returns error)

## Phase 6: Frontend updates
- [x] 6.1 Detect `Solid([` in RON text — kind badge indicator in editor header shows "Solid op-stack"
- [x] 6.2 Show operation stack in property tree — `describe_kind` returns op_count + per-op rows (display-only, editing returns error)
- [x] 6.3 Operator add/remove via property table toolbar (Add Op dropdown + Remove last op) — RON text manipulation approach
- [x] 6.4 Profile point list inline editing — `field_type = "points"` triggers textarea; edit sends `SetProperty` patch; backend parses `[[x,y],...]` format
- [x] 6.5 SolidOp field decomposition in `describe_kind` — ops shown as per-field rows (profile, angle, height, etc.) with editable numeric fields and point lists
- [x] 6.5 Error propagation improved — recompute engine skips failed components (filter_map), `build_component_mesh` logs errors via eprintln
- [x] 6.6 Transform edit fields — `describe_kind` → `transform_rows` appends pos_x/y/z + rot_x/y/z_deg; `set_component_property` in patch.rs handles editing via SetProperty; frontend `renderPropertyTable` renders them generically with double-click editing
- [x] 6.7 Added 3 Solid-kind presets (extrude, revolve, loft) to frontend dropdown
- [x] 6.8 Initial auto-evaluate on page load

## Phase 7: Internal cleanup and hardening
- [x] 7.1 Remove `axial_offset` field entirely (already removed from Component struct)
- [x] 7.2 Add `#[serde(alias)]` for ComponentKind variants (Nosecone, Bodytube, Finset) for forgiving RON parsing
- [x] 7.3 Update all doc comments, error messages — 0 stale references remain
- [x] 7.4 Full `cargo test --workspace` pass — 116 tests pass, 0 warnings
- [x] 7.5 Manual smoke tests: rocket RON presets verified; `cargo build --workspace` succeeds; 3 Solid-kind presets added
- [x] 7.6 Fix 3 unused-variable warnings (xu, xl, throat_idx) for zero-warning build
- [x] 7.7 Benchmark: evaluate time vs mesh complexity — 40pt revolve ~44ms, 60pt ~54ms, 80pt ~71ms; extrude ~1.5ms; transform ~instant
