# APRO CAD — Aerospace Parametric B-Rep CAD

**Working name:** APRO CAD (rename to fit the APRO Hub family — e.g. *Chisel-Aero*, *Forge*).
**Status:** Design specification, v0 (pre-implementation).
**Lineage:** Evolution of [ChiselCAD](https://github.com/LTKMN/ChiselCAD), re-founded on an exact B-Rep geometry kernel and built specifically for aerospace design, simulation, and AI-assisted editing.

---

## 1. Vision

APRO CAD is a **code-first, AI-editable, parametric CAD application built specifically for aerospace geometry** — nose cones, rocket nozzles, combustion chambers, propellant tanks, fins, and airframes — with simulation baked in rather than bolted on.

It is not a general-purpose modeler with aerospace clip-art. It is a system where:

- A design is a **human-readable, diffable document** (the "base file") that both an engineer and an AI can read and edit.
- Geometry is **exact** (boundary representation with NURBS surfaces), not faceted mesh, so it exports clean STEP and meshes cleanly for CFD/FEA.
- Editing is **real-time** — parameter changes recompute only what changed and re-render instantly.
- The same design document is the **single source of truth** consumed by geometry, simulation, mass-properties, and AI.

APRO CAD is intended to be the **parametric geometry core of the APRO platform**, sitting underneath rocket-specific tooling (R-Design), propulsion tooling (Propulsor), and the structural/thermal analysis suites already developed for the LPDE work.

---

## 2. Design principles

1. **The design is data, not code.** The base file is a declarative parametric document (RON). Geometry is *derived* from it deterministically. This is what makes AI editing safe: the AI edits structured, validatable data, never free-form geometry code.
2. **Exact geometry only.** B-Rep + NURBS. No mesh-CSG for the master model. Meshes are an *output* (for rendering and simulation), never the master representation.
3. **The aerospace knowledge lives in the open.** All the surface math (nose-cone families, nozzle contours, airfoils) is a small set of pure, transparent functions. Anyone — including the AI — can read and alter them without touching the kernel.
4. **One operation, many parts.** Most axisymmetric aerospace components are *profile → revolve → shell*. This single abstraction keeps the codebase small and legible.
5. **The kernel is swappable.** Aerospace math never imports the kernel directly. If the kernel disappoints, it is replaced behind a stable interface without touching the domain logic.
6. **Recompute nothing you don't have to.** Real-time interactivity comes from incremental recomputation, not from a faster kernel.
7. **Simulation is a first-class consumer.** The document carries not just geometry but engineering metadata (materials, wall thickness, loads) so solvers read the same source of truth.

---

## 3. System architecture

### 3.1 The four-layer spine

```
┌──────────────────────────────────────────────────────────┐
│  document   RON "base file" — humans & AI edit this        │
├──────────────────────────────────────────────────────────┤
│  geometry   pure math: nose profiles, nozzle contours,     │
│             airfoils. NO kernel dependency. The readable    │
│             heart of the system.                            │
├──────────────────────────────────────────────────────────┤
│  features   profile → B-Rep solid: revolve, shell,          │
│             boolean, loft, pattern (calls the kernel)       │
├──────────────────────────────────────────────────────────┤
│  kernel     Truck B-Rep/NURBS. Tessellate → render,         │
│             STEP export, mass properties. SWAPPABLE.        │
└──────────────────────────────────────────────────────────┘
```

### 3.2 Data flow

```
edit (user or AI)
   │
   ▼
document (RON) ──► recompute engine (dirty-tracking DAG)
                        │  only changed features re-evaluate
                        ▼
                   geometry math ──► features ──► kernel (B-Rep solids)
                        │
                        ├─► tessellate (adaptive tolerance) ──► Three.js viewer
                        ├─► STEP export ──► downstream CAD / suppliers
                        ├─► mass properties (CG, inertia) ──► stability, trajectory
                        └─► simulation mesh (Gmsh) ──► FEA / CFD solvers
```

### 3.3 Crate & directory layout

```
apro-cad/
├── crates/
│   ├── geometry/    # pure math: nose profiles, nozzle contours, airfoils — no kernel deps
│   ├── features/    # profile → Truck B-Rep: revolve, shell, boolean, loft, pattern
│   ├── document/    # the IR: serde types, RON load/save, schema validation, migration
│   ├── recompute/   # feature dependency graph, dirty tracking, mesh cache
│   ├── massprops/   # volume, centroid, inertia tensor from B-Rep
│   └── kernel/      # thin swappable wrapper over truck (or monstertruck / occt)
├── src-tauri/       # Tauri commands: evaluate / apply_patch / export_step / mass_props
├── src/             # webview UI (editor + Three.js viewer, from ChiselCAD)
└── examples/        # sample vehicles (.ron) for tests and demos
```

The critical boundary is `geometry/` (pure math, no kernel) vs `kernel/` (swappable). This delivers both goals at once: the aerospace math stays readable and independent, and the kernel can be replaced without disturbing it.

### 3.4 Process model (Tauri)

- **Rust core (backend):** owns the document, the kernel, recompute, mass properties, STEP export. All heavy geometry lives here.
- **Webview (frontend):** the code/parameter editor and the Three.js viewer, largely inherited from ChiselCAD. It never does geometry — it sends edits and renders meshes.
- **IPC contract:** three primary commands (see §8). The backend streams back only vertex/normal/index buffers (mesh deltas), keeping IPC light.

---

## 4. The kernel decision

| Option | Language | B-Rep + NURBS | STEP I/O | Meshing | Maturity | Notes |
|---|---|---|---|---|---|---|
| **Truck** (`ricosjp/truck`) | Pure Rust | Yes | Yes | Yes | Young but usable | Default choice. WebGPU + WASM capable. |
| **monstertruck** (fork) | Pure Rust | Yes | Yes | Yes | Active (2026) | Refactored API, faster primitives. Evaluate for speed. |
| **Fornjot** (`hannobraun/Fornjot`) | Pure Rust | Partial | Limited | Yes | Young | Strong code-CAD philosophy; historically weaker booleans/NURBS. |
| **opencascade-rs** (OCCT FFI) | Rust → C++ | Yes (industrial) | Yes (industrial) | Yes | Battle-tested | Heavy, C++ FFI. The escape hatch. |

**Decision:** Start on **Truck** (evaluate **monstertruck** for performance). It is pure Rust, fits the Tauri stack, provides NURBS for smooth aero surfaces, exports STEP, meshes for simulation, and computes volume + centroid natively.

**Known risk:** pure-Rust B-Rep kernels are younger than OpenCascade; **fillets and complex booleans are the fragile area**. Mitigation: the isolated `kernel/` wrapper lets us fall back to `opencascade-rs` for a specific operation without rewriting domain logic. This fallback is a *decision record*, not a rewrite.

---

## 5. The base file (parametric IR)

Format: **RON** (serde-native, human-readable, diffable, maps 1:1 to Rust types). This document *is* the design and *is* the AI's action space.

### 5.1 Example

```ron
Vehicle(
    name: "APRO-1",
    units: Millimeters,
    components: [
        NoseCone(
            profile: VonKarman,     // Conical | Ogive | VonKarman | Haack(c) | Power(n) | Parabolic(k)
            length: 300.0,
            base_radius: 54.0,
            wall: 2.0,
            material: "Al-6061-T6",
        ),
        BodyTube( length: 700.0, radius: 54.0, wall: 2.0, material: "CFRP" ),
        Nozzle(
            kind: Bell,             // Conical | Bell | Moc
            throat_radius: 12.0,
            expansion_ratio: 8.0,   // Ae / At
            percent_bell: 80.0,     // Rao length fraction
            chamber_radius: 40.0,
            wall: 3.0,
            material: "Graphite",
        ),
        FinSet(
            count: 4,
            root_chord: 120.0,
            tip_chord: 60.0,
            span: 80.0,
            sweep: 30.0,            // degrees
            airfoil: NACA(digits: "0008"),
            thickness: 4.0,
            material: "Al-6061-T6",
        ),
    ],
)
```

### 5.2 Schema rules

- Every component is a typed variant with named, unit-carrying parameters.
- `material` references a shared material library (density, modulus, allowables) so mass properties and FEA read consistent values.
- The document is **validated** on every edit: ranges (e.g. `expansion_ratio > 1.0`), physical sanity (`throat_radius < chamber_radius`), and topological feasibility.
- The `document/` crate owns **schema migration**: each document records the schema version it was written against, and migrations upgrade older files forward.

---

## 6. Geometry math (the readable heart)

The domain knowledge lives here as pure functions — no kernel types, fully unit-testable.

### 6.1 Nose-cone families

```rust
/// Local radius y at axial station x ∈ [0, length], for base radius R.
pub enum NoseProfile {
    Conical,
    Ogive,
    VonKarman,          // Haack C=0: minimum drag for given L,R
    Haack { c: f64 },   // C=1/3: LV-Haack
    Power { n: f64 },
    Parabolic { k: f64 },
}

impl NoseProfile {
    pub fn radius(&self, x: f64, l: f64, r: f64) -> f64 {
        use std::f64::consts::PI;
        match *self {
            NoseProfile::Conical      => x * r / l,
            NoseProfile::Power { n }  => r * (x / l).powf(n),
            NoseProfile::Ogive => {
                let rho = (r * r + l * l) / (2.0 * r);          // ogive radius
                (rho * rho - (l - x).powi(2)).sqrt() - (rho - r)
            }
            NoseProfile::Parabolic { k } => {
                let t = x / l;
                r * (2.0 * t - k * t * t) / (2.0 - k)
            }
            NoseProfile::VonKarman    => haack(x, l, r, 0.0),
            NoseProfile::Haack { c }  => haack(x, l, r, c),
        }
    }
}

fn haack(x: f64, l: f64, r: f64, c: f64) -> f64 {
    use std::f64::consts::PI;
    let theta = (1.0 - 2.0 * x / l).clamp(-1.0, 1.0).acos();
    r * ((theta - (2.0*theta).sin()/2.0 + c*theta.sin().powi(3)) / PI).sqrt()
}
```

### 6.2 Nozzle contours (Rao thrust-optimized parabolic)

The bell nozzle is a converging cone + circular throat arc + parabolic bell, defined by initial/exit wall angles θn, θe drawn from Rao's design charts (functions of expansion ratio and percent-length).

```rust
/// (x, r) contour of a thrust-optimized parabolic (Rao) nozzle.
pub fn bell_contour(rt: f64, eps: f64, percent: f64, theta_n: f64, theta_e: f64) -> Vec<[f64; 2]> {
    let re = rt * eps.sqrt();                                   // exit radius (Ae/At = eps)
    let mut pts = throat_arc(rt, theta_n);                      // 0.382*Rt downstream arc
    let n  = *pts.last().unwrap();                              // parabola start (N)
    let ln = percent/100.0 * (re - rt) / (15f64).to_radians().tan(); // 15° conical ref length
    let e  = [ln, re];                                          // exit point (E)
    pts.extend(parabola(n, e, theta_n, theta_e));              // quadratic Bézier N→E
    pts
}
```

Conical and method-of-characteristics (MOC) contours are alternative functions with the same signature shape. All feed the same revolve.

### 6.3 Airfoils

NACA 4-digit (and later 5-digit / custom) sections for fins, returned as an ordered point loop ready to sketch and loft.

---

## 7. Features (profile → solid)

Every axisymmetric component routes through one generic operation:

```rust
pub fn revolve_shell(profile: &[[f64; 2]], wall: f64) -> Solid {
    let wire  = bspline_wire(profile);                          // fit NURBS through samples
    let face  = builder::try_attach_plane(&[wire]).unwrap();
    let solid = builder::rsweep(&face, ORIGIN, AXIS_Z, Rad(TAU)); // full revolution
    kernel::shell(&solid, wall)                                 // hollow to wall thickness
}
```

- **Nose cones, nozzles, tanks, chambers, transitions** → `revolve_shell`.
- **Fins** → sketch airfoil → loft root-to-tip with sweep/taper → mirror/pattern about the axis.
- **Assembly** → boolean union of components at their axial stations; couplers and mates align interfaces.

The bounded set of templates (revolve, loft, pattern, boolean) is what keeps the whole system legible.

---

## 8. Tauri IPC contract

```
evaluate(document: Ron)            -> Mesh            // full rebuild
apply_patch(patch: Patch)          -> MeshDelta       // incremental edit
export_step(document: Ron)         -> FilePath        // exact B-Rep out
mass_props(document: Ron)          -> MassProperties  // CG, mass, inertia tensor
validate(document: Ron)            -> Vec<Issue>      // schema + physical checks
```

The frontend never computes geometry. It sends patches and renders returned buffers.

---

## 9. Real-time recompute engine

Interactivity comes from three mechanisms, in order of impact:

1. **Incremental recompute over a feature DAG.** Each component caches its output solid keyed by a hash of its parameters. Editing one parameter dirties only that node and its downstream dependents; everything else serves from cache.
2. **Adaptive tessellation tolerance.** Coarse mesh while a slider is dragged (near-instant), refined on idle, full precision for STEP export. The B-Rep stays exact; only display resolution flexes.
3. **Parallelism + debounce.** Independent components tessellate in parallel (`rayon`); rapid slider events are debounced to recompute the final value only.

---

## 10. Rendering

- Rust tessellates the B-Rep and streams positions/normals/indices to the webview.
- **Three.js** renders (inherited from ChiselCAD) — keeps the existing viewer, camera, and interaction code.
- WebGPU-direct rendering via Truck is a possible later optimization but not required for v1.

---

## 11. AI integration (the defining feature)

The base file being structured data is what makes AI editing tractable and safe.

- **Action space = document patches.** The AI never writes geometry code. It emits validated edits to the RON document ("set expansion ratio to 12", "switch nosecone to von Kármán, fineness 5").
- **Validation gate.** Every AI patch passes through `validate()` before it is applied — schema, ranges, physical sanity.
- **Repair loop.** After applying, the system recompiles the geometry and reports back (did it build? new CG, CP, stability margin?). The AI iterates against real feedback rather than guessing.
- **Retrieval before training.** Near-term intelligence comes from tool-calling over the schema + retrieval over the APRO component library and design references (Barrowman equations, Sutton, past designs). Fine-tuning on (intent → patch) pairs is a later optimization once real usage data exists.

---

## 12. Simulation integration

The document carries geometry *and* engineering metadata, so solvers consume one source of truth.

- **Aerodynamic stability (analytical):** Barrowman method for CP/CG and stability margin — fast enough to run live as the design changes.
- **Trajectory (6-DOF):** integrate **RocketPy** — geometry + motor → apogee, trajectory, dispersion.
- **Structural FEA:** B-Rep → **Gmsh** mesh → the existing LPDE structural suite (axisymmetric FEM, thick-wall Lamé, shell dynamics, modal, fatigue, thermal).
- **Propulsion:** **Propulsor** as the motor/thrust-curve source; NASA CEA for thermochemistry.
- **CFD (later, server-side):** Gmsh + SU2/OpenFOAM. Heavy, out-of-process, and the export-control-sensitive end of the roadmap.

**Mass properties** (CG, mass, inertia tensor) come straight from the exact B-Rep and are the connective tissue between geometry and every dynamics simulation.

---

## 13. Interoperability & outputs

- **STEP / IGES export** — exact B-Rep for downstream CAD, CAM, and suppliers.
- **STL / 3MF** — for 3D-printed prototypes and quick prints.
- **Mesh export (Gmsh formats)** — for simulation.
- **The .ron document itself** — the portable, versionable master file; the true deliverable.

---

## 14. Relationship to the APRO ecosystem

APRO CAD is the **parametric geometry core**, not a standalone island:

- **R-Design** becomes the rocket-specific UI/workflow layer on top of this core.
- **Propulsor** provides propulsion definitions and consumes nozzle geometry.
- **The LPDE FEA suite** becomes the structural analysis module.
- **APRO Hub** hosts them as one platform reading one shared IR.

The moat is not the editor — it is the single parametric source of truth that geometry, simulation, and AI all speak.

---

## 15. Export-control note

As high-fidelity trajectory, guidance, and propulsion simulation are added, the tool moves toward the controlled end of the technology spectrum (MTCR/dual-use). This is a **classification-awareness** item to track as features mature — not a blocker for the CAD and analysis foundation, but something to flag before shipping guidance/6-DOF and CFD capabilities externally.

---

## 16. Technology stack

- **Backend:** Rust — Truck (kernel), rayon (parallelism), serde + RON (document), Gmsh bindings (meshing).
- **Shell:** Tauri.
- **Frontend:** existing ChiselCAD webview — editor + Three.js viewer.
- **Simulation:** RocketPy, Gmsh, the LPDE Python FEA suite, Propulsor, NASA CEA; SU2/OpenFOAM later.
- **AI:** structured tool-calling over the RON schema + retrieval; fine-tuning later.

---

## 17. Risks & mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Pure-Rust kernel fillet/boolean bugs | High | Isolated `kernel/` wrapper; `opencascade-rs` fallback per-operation. |
| Real-time performance at high tessellation | Medium | Incremental DAG recompute + adaptive tolerance + rayon. |
| AI produces invalid designs | Medium | Patch-only action space + validation gate + repair loop. |
| Scope sprawl vs R-Design / Propulsor | High | Treat this as the shared core; those become layers, not forks. |
| Export-control exposure | Medium | Classification review before shipping guidance/6-DOF/CFD. |

---

## 18. Version history (phased roadmap)

Versions are milestone-gated. Each ships something demonstrable. Phases group related versions.

### Phase 0 — Foundation (prove the loop)

| Version | Deliverable |
|---|---|
| **v0.1** | Tauri + Rust skeleton. Webview loads; `evaluate()` command round-trips a hardcoded solid to the Three.js viewer. |
| **v0.2** | `document/` crate: RON load/save of a `Vehicle`, one component type, schema versioning. |
| **v0.3** | `geometry/` nose-cone profile math (all families) + unit tests. No kernel yet — validated numerically. |
| **v0.4** | Kernel integration: `revolve_shell`. One von Kármán nose cone: RON → revolve → mesh → render. **End-to-end loop proven.** |

### Phase 1 — Aerospace geometry library

| Version | Deliverable |
|---|---|
| **v0.5** | Full nose-cone family selectable from the document; live re-render on parameter change (naive full rebuild). |
| **v0.6** | Body tubes, transitions, tanks (ellipsoidal/hemispherical domes) via the same revolve. |
| **v0.7** | Rao bell nozzle + conical nozzle contours. Chamber + throat + bell as one shelled solid. |
| **v0.8** | Fins: NACA airfoil sketch → loft → pattern. Sweep/taper parameters. |
| **v0.9** | Assembly: boolean union of components at axial stations; couplers/mates; material library. |

### Phase 2 — Real-time & outputs

| Version | Deliverable |
|---|---|
| **v0.10** | Recompute DAG + dirty tracking + mesh cache. Only changed features rebuild. |
| **v0.11** | Adaptive tessellation (coarse-on-drag, refine-on-idle) + rayon parallelism + debounce. **Real-time editing achieved.** |
| **v0.12** | STEP/IGES export. STL/3MF export. |
| **v0.13** | `massprops/`: volume, CG, inertia tensor from the B-Rep, displayed live. |
| **v1.0** | **First stable release.** Full aerospace primitive library, real-time parametric editing, exact exports, mass properties. Usable for real design work. |

### Phase 3 — AI-assisted design

| Version | Deliverable |
|---|---|
| **v1.1** | Patch protocol: structured edits to the document with full `validate()` gate. |
| **v1.2** | AI editing loop: natural-language intent → validated patch → rebuild → geometry feedback. |
| **v1.3** | Retrieval over the APRO component library + design references (Barrowman, Sutton). |
| **v1.4** | Repair loop hardened: AI reads build errors and mass/stability results and iterates. |
| **v1.5** | Design assistant: "make this stable", "optimize this nozzle for sea level" as guided operations. |

### Phase 4 — Simulation coupling

| Version | Deliverable |
|---|---|
| **v1.6** | Barrowman stability (CP/CG/margin) live in-app. |
| **v1.7** | Gmsh meshing bridge → LPDE structural FEA suite on generated geometry. |
| **v1.8** | RocketPy 6-DOF trajectory from geometry + Propulsor motor. |
| **v1.9** | Thermal + propulsion coupling (Propulsor / NASA CEA) into the nozzle/chamber workflow. |
| **v2.0** | **Integrated platform release.** CAD + stability + structural + trajectory + propulsion on one document. |

### Phase 5 — Platform & scale

| Version | Deliverable |
|---|---|
| **v2.1+** | R-Design and Propulsor re-based onto this core as UI layers within APRO Hub. |
| **v2.x** | Server-side CFD (Gmsh + SU2/OpenFOAM); design-of-experiments and optimization loops. |
| **v2.x** | Fine-tuned aerospace design model on real usage data; collaborative/versioned documents (Git-style). |
| **v3.0** | Full APRO design environment: geometry, simulation, AI, and collaboration unified. |

---

## 19. Immediate next step

Build **v0.1 → v0.4** as a single focused sprint: the end-to-end loop (RON document → nose-cone math → Truck revolve → mesh → Tauri → Three.js). Everything else is additive once that spine is alive. Resist adding a second primitive before the first one renders in real time — the loop is the product; the library is just volume on top of it.
