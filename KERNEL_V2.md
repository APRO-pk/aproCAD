# Kernel v2 — OpenCascade (OCCT) Backend Plan

Status: PHASE 0 COMPLETE — Phase 1 (OCCT hello-solid) next
Owner: kernel crate (`crates/kernel`)
Goal: replace the Truck-0.6-limited geometry backend with OpenCascade-backed
operations, unlocking Fillet, Chamfer, Shell, true splines/curved sweeps and
industrial-grade booleans + STEP fidelity.

---

## Why
Truck 0.6 hard-stops on fillet/chamfer/shell (long-standing upstream limitation)
and has no NURBS. Every real mechanical part needs edge breaks; the AI agent is
currently forced to avoid those ops. OCCT solves all of these in one dependency,
and its STEP importer/exporter is the industry reference.

## Architecture — ShapeKernel trait
The whole upper stack (document model, parameters, recompute engine, patches,
AI contracts, UI) already compiles down to `MeshData` + `SolidOp` descriptions.
We formalize that boundary as a trait so backends are swappable:

```rust
// crates/kernel/src/backend.rs
pub trait ShapeKernel {
    type Error;
    type Solid; // opaque B-rep handle per backend

    fn extrude(&self, profile: &[[f64; 2]], height: f64) -> Result<Self::Solid, Self::Error>;
    fn revolve(&self, profile: &[[f64; 2]], angle_deg: f64) -> Result<Self::Solid, Self::Error>;
    fn loft(&self, profiles: &[Vec<[f64; 2]>]) -> Result<Self::Solid, Self::Error>;
    fn sweep(&self, profile: &[[f64; 2]], path: &[[f64; 3]]) -> Result<Self::Solid, Self::Error>;
    fn boolean(&self, base: &Self::Solid, tool: &Self::Solid, kind: BooleanKind) -> Result<Self::Solid, Self::Error>;
    fn fillet(&self, solid: &Self::Solid, radius: f64, edges: EdgeSel) -> Result<Self::Solid, Self::Error>;
    fn chamfer(&self, solid: &Self::Solid, distance: f64, edges: EdgeSel) -> Result<Self::Solid, Self::Error>;
    fn shell(&self, solid: &Self::Solid, thickness: f64) -> Result<Self::Solid, Self::Error>;
    fn transform(&self, s: &Self::Solid, t: [f64; 3], r_deg: [f64; 3], s_xyz: Option<[f64; 3]>) -> Self::Solid;
    fn tessellate(&self, s: &Self::Solid, tolerance: f64) -> MeshData;
}
```

`features::eval` stops calling free functions and drives a boxed
`Box<dyn ShapeKernel>` instead. `MeshData`, `Profile`, `Path3D`, document model:
**unchanged**.

### Backends
| Backend | Module | Status |
|---|---|---|
| Truck 0.6 (current behavior) | `backend/truck.rs` | default; keeps app working |
| OpenCascade | `backend/occt.rs` | feature-gated `occ`, Phase 1 spike |

Selection at runtime: `OnceLock<Box<dyn ShapeKernel>>`, chosen by env var
(`APRO_KERNEL=truck|occt`) until the OCC path proves out.

## OCCT integration notes (Windows)
- Rust wrapper: [`opencascade` crate](https://crates.io/crates/opencascade)
  (safe bindings over occt-rs/cxx). Requires OCCT C++ libs.
- Recommended install: **vcpkg**: `vcpkg install opencascade:x64-windows-static-md`
  then set env `RUSTOCCT_VCPKG=<vcpkg-root>` before build (the crate's
  documented lookup). Alternative: system OCCT via `OCCT_INCLUDE`/`OCCT_LIB`.
- Cargo: `[dependencies] opencascade = { version = "0.7", optional = true }`
  and kernel feature `occ = ["dep:opencascade"]`.
- Meshing: `BRepMesh_IncrementalMesh(shape, deflection)` → triangulation
  extraction into our existing `MeshData` (positions/normals/indices).
- STEP I/O stays on Truck for now; migrate to OCC IGES/STEP later if fidelity
  issues appear.

## Op mapping sketch (OCC APIs)
| APRO op | OCCT call chain |
|---|---|
| extrude | `BRepPrimAPI_MakePrism(wire_face, vec)` |
| revolve | `BRepPrimAPI_MakeRevol(face, gp_Ax1, angle)` |
| loft | `BRepOffsetAPI_ThruSections(is_solid, ruled)` |
| sweep | `BRepOffsetAPI_MakePipe(spine_wire, profile)` (pipe → `MakePipeShell` later) |
| fillet | `BRepFilletAPI_MakeFillet` + edge selection from `TopExp_Explorer(TopAbs_EDGE)` |
| chamfer | `BRepFilletAPI_MakeChamfer` |
| shell | `BRepOffsetAPI_MakeThickSolid` |
| boolean | `BRepAlgoAPI_Fuse/Cut/Common` |
| transform | `gp_Trsf` (+ scale factor) via `BRepBuilderAPI_Transform` |
| tessellate | `BRepMesh_IncrementalMesh` → face triangulations |

Edge selection (`EdgeSel`) for v1: All | ByAngle(threshold) — matching how the
UI will expose it; explicit indices come with the feature-tree work.

## Phases
0. **Spike (this PR set)** — trait extracted, TruckBackend wired through
   features/eval, `occ` feature skeleton compiles-with-dep-present, plan doc.
   Exit criteria: full test suite green on default features; OCCT link attempt
   documented (success or exact blocker).
1. **OCCT hello-solid** — behind `--features occ`: extrude+revolve+tessellate
   producing identical-ish MeshData vs Truck golden tests (loose tolerance).
2. **Parity ops** — loft/sweep/boolean/transform through OCC.
3. **The unlocks** — fillet/chamfer/shell live in schema + eval + AI docs
   (remove from UNIMPLEMENTED_OPS), plus `Path3D::Arc/Spline` sampling for sweeps.
4. **Edge selection UX** — property-panel edge picker, then AI contract update.

## Risks
- OCCT build weight (~1 GB toolchain; mitigated by vcpkg caching).
- Version pinning pain between `opencascade` crate ↔ installed OCCT release.
- Tessellation differences change vertex counts → any tests asserting exact
  counts must assert ranges instead.
- Windows static-md CRT mixing with WebView2 runtime: use
  `x64-windows-static-md` triplet exactly.

## Non-goals (for now)
Sketcher/constraints, NURBS *authoring* surfaces, multi-document assemblies.
