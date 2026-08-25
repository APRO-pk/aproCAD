# APRO CAD v2 — General-Purpose Parametric CAD with Aerospace Primitives

## 1. Philosophy

APRO CAD is a **code-first parametric CAD system** where the design document is a human-readable RON file that both a person and an AI can edit. It is not an aerospace clip-art library — it is a **general-purpose solid modeling kernel** with **aerospace-aware shortcuts** that compile down to the same primitives as everything else.

**Core principle:** There is no special geometry pipeline. Aerospace components (nose cones, nozzles, tanks) are convenience functions that emit the same solid-operation graph as a custom revolve or extrude. A von Kármán nose cone and a custom vase are both `Revolve { profile: [...], angle: 360 } → Shell { thickness: ... }` under the hood.

This means you can model a rocket *or* a butterfly. The aerospace templates just save you keystrokes.

---

## 2. The Solid Operation Stack (the heart of the system)

Every solid in APRO CAD is produced by a **stack of operations** applied in sequence. Each operation takes the output of the previous one and transforms it. The first operation creates a raw solid from a profile or sketch; subsequent operations modify it.

### 2.1 Operation enum

```rust
pub enum SolidOp {
    /// Spin a 2D profile around the Z axis.
    Revolve {
        profile: Profile,
        angle: f64,              // degrees, default 360
        axis: Option<Axis>,      // default Z
    },
    /// Pull a 2D profile along a direction.
    Extrude {
        profile: Profile,
        height: f64,
        direction: Option<Direction>, // default +Z
        taper: Option<f64>,          // draft angle in degrees
    },
    /// Loft (blend) between two or more profiles.
    Loft {
        profiles: Vec<Profile>,
        guide_curves: Option<Vec<Profile>>, // optional rails
    },
    /// Sweep a profile along a path.
    Sweep {
        profile: Profile,
        path: Path3D,
        twist: Option<f64>,  // degrees of twist along path
    },
    /// Hollow the solid to a wall thickness.
    Shell {
        thickness: f64,
        faces: Option<Vec<FaceRef>>, // which faces to remove (open)
    },
    /// Boolean operation with another solid (by component reference).
    Boolean {
        kind: BooleanKind,        // Union, Difference, Intersection
        target: SolidRef,         // reference to another component's solid
    },
    /// Add a fillet (rounded edge).
    Fillet {
        radius: f64,
        edges: EdgeRef,          // which edges to fillet
    },
    /// Add a chamfer.
    Chamfer {
        distance: f64,
        edges: EdgeRef,
    },
    /// Transform (translate / rotate / scale) the solid.
    Transform {
        translate: Option<Vec3>,
        rotate: Option<Vec3>,    // Euler angles (roll, pitch, yaw)
        scale: Option<Vec3>,     // uniform or per-axis
    },
}
```

### 2.2 Profiles

A `Profile` defines a closed 2D shape. It can be constructed in several ways:

```rust
pub enum Profile {
    /// Raw point list (sample an arbitrary curve).
    Points(Vec<[f64; 2]>),

    /// A circle centered at origin.
    Circle { radius: f64 },

    /// A rectangle centered at origin.
    Rectangle { width: f64, height: f64, corner_radius: Option<f64> },

    /// A regular polygon.
    Polygon { sides: u32, circumradius: f64 },

    /// Reference to another component's profile (for boolean ops).
    Reference(String),

    /// A parametric curve function (future: user-defined math).
    UserFunction { expr: String, variable: String, range: [f64; 2], samples: u32 },
}
```

### 2.3 Path3D

For sweep operations:

```rust
pub enum Path3D {
    /// Line segment.
    Line { start: Vec3, end: Vec3 },
    /// Arc.
    Arc { center: Vec3, radius: f64, start_angle: f64, end_angle: f64 },
    /// Spline through points.
    Spline(Vec<Vec3>),
    /// Helix (for threads, springs).
    Helix { radius: f64, pitch: f64, turns: f64 },
}
```

### 2.4 SolidRef and FaceRef

References to existing geometry:

```rust
pub enum SolidRef {
    /// By component name in the same vehicle.
    Component(String),
    /// The output of the current operation stack itself.
    This,
}

pub enum FaceRef {
    Index(usize),          // face index in the solid
    Normal(Vec3),          // face with closest normal
    Plane { point: Vec3, normal: Vec3 },
}

pub enum EdgeRef {
    All,
    Indices(Vec<usize>),
}
```

---

## 3. Component model

### 3.1 Component

Every part in a design is a `Component`. Components are the atomic unit of the design document.

```ron
Component(
    name: "NoseCone-1",
    material: "Al-6061-T6",
    visible: true,
    transform: (
        position: (0.0, 0.0, 0.0),
        rotation: (0.0, 0.0, 0.0),   // roll, pitch, yaw in degrees
    ),
    kind: Solid(              // <-- changed from aerotype enum to unified system
        ops: [
            Revolve(
                profile: Points([
                    (0.0, 0.0),
                    (100.0, 50.0),
                    (300.0, 54.0),
                ]),
                angle: 360.0,
            ),
            Shell(thickness: 2.0),
        ]
    ),
)
```

### 3.2 Aerospace shorthands

Aerospace components are *syntactic sugar* — they get expanded into `Solid { ops: [...] }` at parse time or during compilation. They exist so engineers don't need to type nose-cone profile math by hand.

```ron
// Shortcut — expands to the Solid { ops: [...] } above
Component(
    name: "NoseCone-1",
    material: "Al-6061-T6",
    transform: (position: (0, 0, 0), rotation: (0, 0, 0)),
    kind: NoseCone(
        profile: VonKarman,
        length: 300.0,
        base_radius: 54.0,
        wall: 2.0,
    ),
)
```

Current aerospace shorthand types and their expansion:

| Shorthand | Expands to |
|---|---|
| `NoseCone { profile, length, base_radius, wall }` | `Revolve(profile: <nosecone math points>) + Shell(thickness: wall)` |
| `BodyTube { length, radius, wall }` | `Revolve(profile: rectangle profile) + Shell(thickness: wall)` |
| `Transition { length, start_radius, end_radius, wall }` | `Revolve(profile: linear taper) + Shell(thickness: wall)` |
| `Tank { radius, cylindrical_length, dome, wall }` | `Revolve(profile: dome+cylinder profile) + Shell(thickness: wall)` |
| `Nozzle { kind, throat_radius, ... }` | `Revolve(profile: bell/conical/MOC contour) + Shell(thickness: wall)` |
| `FinSet { count, root_chord, tip_chord, span, ... }` | `Loft(profiles: [root airfoil, tip airfoil]) + Pattern(count)` |

### 3.3 Transform system

Every component has a full 6-DOF transform applied when its mesh is assembled into the vehicle:

```rust
pub struct Transform {
    pub position: Vec3,
    pub rotation: Vec3,  // Euler angles in degrees (roll, pitch, yaw)
}
```

The transform is applied as a 4×4 matrix during mesh assembly. This replaces the single `axial_offset: f64` with proper 3D positioning.

The mesh assembly for a vehicle:
1. Evaluate each component's solid operation stack independently
2. Tessellate to mesh
3. Apply the component's transform matrix to all vertices
4. Concatenate all meshes into a single buffer

---

## 4. Vehicle model

A `Vehicle` is a named collection of components:

```ron
Vehicle(
    name: "Butterfly",
    units: Millimeters,
    author: "Engineer Name",
    notes: "A parametric butterfly with lofted wings",
    components: [
        Component(
            name: "Body",
            material: "Al-6061-T6",
            transform: (position: (0, 0, 0), rotation: (0, 0, 0)),
            kind: Solid(ops: [
                Revolve(profile: Points([(0,0), (2,8), (4,10), (6,8), (8,0)]), angle: 360),
            ]),
        ),
        Component(
            name: "LeftWing",
            material: "CFRP",
            transform: (position: (0, 4, 0), rotation: (0, 0, 10)),
            kind: Solid(ops: [
                Loft(profiles: [
                    Points([(-60,0), (0,10), (60,0), (0,-10)]),
                    Points([(-80,0), (0,15), (80,0), (0,-15)]),
                ]),
                Shell(thickness: 1.0),
            ]),
        ),
        Component(
            name: "RightWing",
            material: "CFRP",
            transform: (position: (0, -4, 0), rotation: (0, 0, -10)),
            kind: Solid(ops: [
                Loft(profiles: [
                    Points([(-60,0), (0,10), (60,0), (0,-10)]),
                    Points([(-80,0), (0,15), (80,0), (0,-15)]),
                ]),
                Shell(thickness: 1.0),
            ]),
        ),
    ],
)
```

---

## 5. Material system

Materials are a first-class type referenced by name:

```ron
Material(
    name: "Al-6061-T6",
    density_kg_m3: 2700.0,
    yield_strength_mpa: 276.0,
    modulus_gpa: 68.9,
    color: (0.8, 0.8, 0.8),
)
```

Built-in library (in `document::material`):
- Al-6061-T6, 7075-T6
- Steel 4130, 304 SS
- Ti-6Al-4V
- Inconel 718
- CFRP (unidirectional, quasi-isotropic)
- G10/FR4
- PLA, ABS, PETG (for 3D printing)
- Graphite, Copper, Brass

Users can define custom materials in the document or load from an external library.

---

## 6. Geometry evaluation pipeline

### 6.1 Execution stages

```
RON document
    │ parse (ron::from_str)
    ▼
Expanded vehicle graph (shorthands → Solid ops)
    │
    ▼ for each component:
    ├─► For each SolidOp in sequence:
    │     (first op) create raw solid from profile
    │     (subsequent ops) modify the solid
    │     Each op calls into the kernel crate
    │
    ├─► Tessellate final solid → MeshData (mesh.rs)
    ├─► Apply component transform to mesh vertices
    │
    ▼ Merge all meshes → single buffer for viewer
```

### 6.2 Handling Boolean ops

Boolean ops reference another component by name:

```ron
Component(
    name: "Bracket",
    ...
    kind: Solid(ops: [
        Extrude(profile: Rectangle(width: 100, height: 100), height: 10),
        Boolean(
            kind: Difference,
            target: Component("Hole"),  // subtract Hole's solid
        ),
    ]),
)
```

The evaluator builds all component solids first, then evaluates ops that reference other components. This requires a two-pass approach:
1. **Pass 1:** Evaluate components with no external references (topological sort)
2. **Pass 2:** Evaluate components with references, using cached solids from Pass 1

This naturally forms a DAG — the recompute engine already tracks this.

### 6.3 Recompute cache

The existing `RecomputeEngine` caches meshes by component fingerprint. This works for custom solids too — the fingerprint is the hash of the full `Component` RON, including the ops stack.

---

## 7. Tauri IPC commands

| Command | Input | Output | Description |
|---|---|---|---|
| `evaluate` | Component RON | `EvaluateResult` | Evaluate single component |
| `evaluate_vehicle` | Vehicle RON | `EvaluateResult` | Evaluate full vehicle |
| `describe_vehicle` | Vehicle RON | `Vec<ComponentTable>` | Get editable property table |
| `apply_patch` | Vehicle RON + Patch | `PatchResult` | Apply structured edit |
| `validate` | Component RON | `Vec<Issue>` | Schema + physics validation |
| `export_step` | Vehicle RON + path | status string | Export B-Rep as STEP |
| `export_stl` | Vehicle RON + path | status string | Export mesh as binary STL |
| `solid_info` | Vehicle RON + component name | `SolidInfo` | Face/edge/vertex count, bounding box |

### SolidInfo

```rust
pub struct SolidInfo {
    pub face_count: usize,
    pub edge_count: usize,
    pub vertex_count: usize,
    pub bounding_box: BoundingBox,
}

pub struct BoundingBox {
    pub min: Vec3,
    pub max: Vec3,
}
```

---

## 8. Error model

Errors fall into three categories:

| Category | Example | Handling |
|---|---|---|
| **Parse error** | Invalid RON syntax | Frontend shows inline error in editor |
| **Validation error** | Negative radius, fin count < 3 | `validate()` returns `Issue` list; patch is rejected |
| **Geometry error** | Profile self-intersects, boolean fails | Kernel returns error; evaluator reports which op failed |

All errors are reported as structured `Issue` objects with component name, severity, and message.

---

## 9. Frontend

The frontend is a single HTML page with:
- **Editor pane** (left): RON text editor with:
  - Syntax highlighting (future)
  - Inline error markers
  - Preset dropdown for aerospace shorthands
  - STL/STEP export buttons
  - Ctrl+Enter to evaluate
  - Ctrl+Z/Y for undo/redo
  - Debounce auto-evaluate (800ms)

- **Properties panel** (below editor):
  - Expandable table of all components and their parameters
  - Double-click to edit any property
  - Edits go through `apply_patch` (validation gate)
  - Add/remove/reorder components

- **Viewer pane** (right): Three.js with:
  - Orbit controls (pan, zoom, rotate)
  - Axis-aligned grid
  - Semi-transparent material with edge wireframe
  - Status bar with vertex/face count, mass, COM

---

## 10. Kernel dependency graph

```
                      ┌──────────────┐
                      │  truck-base  │
                      │  truck-geo   │
                      └──────┬───────┘
                             │
                      ┌──────▼───────┐
                      │ truck-topo   │  Vertex, Edge, Face, Wire, Shell, Solid
                      └──────┬───────┘
                             │
              ┌──────────────┼──────────────┐
              │              │              │
     ┌────────▼────┐ ┌───────▼──────┐  ┌────▼────────┐
     │truck-modeling│ │truck-polymesh│  │truck-stepio │
     │ builder      │ │ PolygonMesh  │  │ STEP I/O    │
     │ revolve,     │ └──────┬───────┘  └─────────────┘
     │ shell,       │        │
     │ boolean      │ ┌──────▼───────┐
     └──────────────┘ │truck-meshalgo│
                      │ triangulation│
                      └──────────────┘
```

Our `kernel` crate wraps all of these behind a stable, swappable interface:

```rust
// kernel/src/lib.rs — public API surface

pub fn revolve(profile: &[[f64; 2]], angle: Rad<f64>) -> Result<Solid, Error>;
pub fn extrude(profile: &[[f64; 2]], height: f64) -> Result<Solid, Error>;
pub fn loft(profiles: &[Vec<[f64; 2]>]) -> Result<Solid, Error>;
pub fn sweep(profile: &[[f64; 2]], path: &[Vec3]) -> Result<Solid, Error>;
pub fn shell(solid: &Solid, thickness: f64) -> Result<Solid, Error>;
pub fn boolean(solid_a: &Solid, solid_b: &Solid, kind: BooleanKind) -> Result<Solid, Error>;
pub fn fillet(solid: &Solid, radius: f64, edges: &EdgeRef) -> Result<Solid, Error>;
pub fn chamfer(solid: &Solid, distance: f64, edges: &EdgeRef) -> Result<Solid, Error>;
pub fn tessellate(solid: &Solid, tolerance: f64) -> MeshData;
pub fn transform_mesh(mesh: &MeshData, transform: &Transform) -> MeshData;
pub fn write_step(solid: &Solid, path: &Path) -> Result<(), Error>;
pub fn write_stl(mesh: &MeshData, path: &Path) -> Result<(), Error>;
```

This is the only crate that imports Truck directly. If Truck's boolean operations are too fragile for production, we can swap to `opencascade-rs` behind this same interface without touching any other crate.

---

## 11. Open issues / future work

| Issue | Status |
|---|---|
| **Constraint-based sketching** | Future — right now all profiles are raw point lists |
| **Parametric expressions** | Future — allow `length = base_radius * 3.5` in RON |
| **Edge/face selection** | Partial — faces by index/normal, edges by index; needs interactive picking in viewer |
| **Multi-body components** | Future — some designs need multiple solids per component |
| **Assembly mates** | Future — planar/cylindrical mates between components |
| **3MF export** | Future |
| **IGES export** | Future (low priority, STEP supersedes) |
| **WebGPU tessellation** | Future — render without Three.js for better performance |
| **AI editing loop** | Phase 3 — natural language → patch → rebuild → feedback |

---

## 12. Summary

APRO CAD v2 is a **general-purpose parametric CAD** where:
- **Every solid** is defined by an **operation stack** (revolve, extrude, loft, sweep, shell, boolean, fillet, chamfer, transform)
- **Aerospace shorthands** are convenience wrappers that emit the same ops
- **Components** have full 6-DOF transforms
- **Materials** are first-class with density and structural properties
- **The pipeline** is: RON → solid ops → Truck B-Rep → tessellation → mesh → Three.js
- **The kernel** is swappable behind a 12-function interface
- **The frontend** is a code editor + property table + 3D viewer
- **All edits** go through a `validate()` gate
- **Incremental recompute** caches by component fingerprint
