# APRO CAD

Text-first parametric CAD for rocket hardware and general mechanical parts.
You describe geometry in **RON** (Rusty Object Notation) instead of drawing it;
the app evaluates that into 3D meshes with the Truck B-Rep kernel and renders
them live in a Tauri v2 desktop window. An AI assistant can read and edit the
same RON, so designs are diffable, reviewable and version-controlled.

---

## Run it

The easiest path is the packaged build — no toolchain needed:

```
dist\APRO-CAD-Windows\apro-cad.exe
```

`ron-check.exe` sits beside it to validate a document without opening the app:

```powershell
.\ron-check.exe path\to\file.ron    # exit 0 = valid
```

### From source

```powershell
cargo build --release          # -> target\release\apro-cad.exe
cargo run --bin ron-check -- path\to\file.ron
cargo test --workspace
```

Frontend files in `src/` are embedded into the binary at build time, so editing
HTML/CSS/JS needs a rebuild — unlike a dev-server setup.

---

## Writing a model

Every solid is an ordered stack of operations. A part is a `Component`; a
multi-part design is a `Vehicle` wrapping components.

```ron
Vehicle(
    name: "Bracket",
    units: Millimeters,
    components: [
        Component(
            name: "Plate",
            material: "Al-6061-T6",
            color: Some("#4f8bff"),
            transform: (position: (0.0, 0.0, 0.0), rotation: (0.0, 0.0, 0.0)),
            kind: Solid([
                Extrude(
                    profile: Rectangle(width: 80.0, height: 50.0, corner_radius: None),
                    height: 8.0,
                    direction: None,
                    taper: None,
                ),
                Hole(diameter: 8.0, depth: 8.0, axis: Z),
                BoltCircle(count: 4, pitch_diameter: 30.0, hole_diameter: 4.0, depth: 8.0),
            ]),
        ),
    ],
)
```

`//` comments are allowed. All struct fields are `name: value`. Struct variants
use parentheses (`Extrude(...)`), never braces.

### Operations

| Op | Description |
|---|---|
| `Extrude` | Sweep a 2D profile to a height, optional taper |
| `Revolve` | Rotate a profile around the Z axis |
| `RevolveChain` | Concatenated revolve segments as one solid |
| `Loft` | Blend between two profiles |
| `Sweep` | Sweep a profile along a `Path3D` (`Line`, `Arc`, `Spline`, `Helix`) |
| `Boolean` | Union / Difference / Intersection with a sibling component |
| `Hole`, `BoltCircle`, `RectPattern` | Machined features cut into the stack |
| `TransformOp` | Translate / rotate / scale within a stack |

A `profile` is a point list (`Points([(x,y),...])`), a parametric shape
(`Circle`, `Rectangle`, `Polygon`, `UserFunction`), or a **`Reference` to a
sketch**. Boolean must not be the first op. `Shell`, `Fillet` and `Chamfer`
exist in the schema but are **not implemented** and error if used.

### Sketches

A `Sketch` is 2D construction geometry on a workplane (`XY`, `XZ`, `YZ`) that a
solid consumes by name:

```ron
Component(
    name: "Sketch1",
    material: "",
    kind: Sketch(SketchParams(
        plane: XY,
        offset: 0.0,
        entities: [
            Rectangle(corner1: (0.0, 0.0), corner2: (80.0, 50.0)),
        ],
    )),
)
Component(
    name: "Plate",
    material: "Al-6061-T6",
    kind: Solid([
        Extrude(profile: Reference("Sketch1"), height: 8.0, direction: None, taper: None),
    ]),
)
```

Entities: `Line`, `Rectangle`, `Circle`, `Arc` (angles in **radians**), and
`Spline` (Catmull-Rom through every point, optionally `closed`).

The compiler stitches open chains end-to-end, so four separate lines that meet
at their corners close into one loop regardless of draw order, and winding is
normalised. A sketch must form **exactly one closed region** — open geometry and
disjoint regions are reported as errors rather than silently producing nothing.
Sketches have no hole support; cut holes with `Boolean`/`Hole`/`BoltCircle`.

Sketch planes place their extrusion automatically: `XY` extrudes along +Z, `XZ`
along -Y, `YZ` along +X, with `offset` shifting the plane along its normal. A
sketch renders as a line overlay (closed loops teal, unclosed leftovers amber)
and needs no extra transform to seat it.

### Drawing a sketch interactively

The **Sketch** ribbon tab draws directly in the viewport: pick Line, Rectangle,
Circle, Arc or Spline, click to place points, then Finish to write the sketch
into the document. Snapping infers endpoints, midpoints, centres, quadrant
points, grid steps and horizontal/vertical alignment, with live dimension
readouts and Shift for ortho lock. Switch the plane or grid step while drawing;
Escape cancels, Enter closes a spline.

### Aerospace shorthands

`NoseCone` (Conical, Ogive, Von Kármán, Power, Haack, Parabolic), `BodyTube`,
`Transition`, `Tank` (Hemispherical / Ellipsoidal), `Nozzle` (Conical, Bell) and
`FinSet` (NACA airfoils) expand into the same op stacks as hand-written solids.

### Parameters

`Vehicle` accepts an optional `parameters` block so dimensions can derive from
each other:

```ron
parameters: Some([
    Parameter(name: "body_od", value: 98.0),
    Parameter(name: "wall", value: 2.0),
    Parameter(name: "body_id", value: "body_od - 2 * wall"),
]),
```

Values are a literal number or a quoted equation over other parameter names
(`+ - * / ^`, `abs sqrt sin cos tan min max pow`, constants `pi`/`e`). Use the
list form above — the pretty block form is not accepted from the AI.

---

## Application

The UI is deliberately small: everything visible works.

**Ribbon**

| Tab | Buttons |
|---|---|
| Home | New, Open, Save, Library, Export STL/STEP/Image, Undo/Redo/Format, AI Chat |
| Sketch | Line, Rectangle, Circle, Arc, Spline |
| View | Front, Top, Right, Isometric, Fit All, Wireframe/Shaded/X-Ray, Grid, Axes |

**Viewport toolbar** — Orbit, Pan, Fit, Grid, Axes, Wireframe, Home, plus a
nav cube and a live mass/volume/centre-of-mass readout.

**Left dock** — Manager (feature tree), Editor (RON), Chat (AI), Inspector.

**File menu** — New, Open, Save, Save As, Export STL/STEP, Reset View, Fullscreen.

Also: debounced live evaluation (800 ms), inline property editing, undo/redo
history, `.STL`/`.STEP` export through native file pickers, and a SQLite-backed
component library that works offline.

While a document evaluates, a progress card blurs the viewport and shows live
per-component progress. The heavy IPC commands (`evaluate`, `evaluate_vehicle`,
`check_interferences`) are `async` deliberately: Tauri runs a *synchronous*
command on the main thread, which freezes the window and also blocks delivery of
the progress events, so an `async` command is what keeps the UI responsive and
lets the bar animate.

**AI assistant** — OpenAI-compatible. Providers: Custom, **DeepSeek**, OpenAI,
Anthropic. Output is pinned to the schema where the provider supports it (a
named JSON schema on OpenAI-style endpoints, a GBNF grammar on local
llama.cpp, a strict RON contract on DeepSeek). `AI_INSTRUCTIONS.md` grounds the
assistant and is embedded in the binary.

**Keyboard** — Ctrl+Enter evaluate · Ctrl+Z / Ctrl+Y undo/redo · Ctrl+F format
· F fit · Ctrl+N/O/S new/open/save · Esc cancel.

---

## Known limitations

- `Shell`, `Fillet`, `Chamfer` are declared but unimplemented (Truck 0.6).
- Booleans run as mesh CSG on tessellated geometry, not exact B-Rep.
- `Loft` and fins use direct mesh generation; no B-Rep capped loft.
- STEP export works for shorthands only — general `Solid` parts are mesh-only.
- Revolve profiles with very dense collinear points (>~42) can fail.
- No wall thickness inside Solid ops; encode it in the profile.
- The sketch layer snaps and infers but has **no constraint solver**, so a
  sketch is exactly what you drew. No holes, sketch-on-face, or edge projection.
- Export/import is STL/STEP only; no native file format yet.

---

## Layout

```
src/                  Frontend (no bundler; three.js vendored, importmap)
src-tauri/            Tauri shell + IPC commands
crates/
  geometry/           Pure-math profiles (nose cones, fins, ...)
  kernel/             Truck wrapper: ops, tessellation, mesh data
  document/           RON types, SolidOp, patch system, golden tests
  features/           Shorthand expansion + SolidOp evaluation + sketches
  recompute/          Failure-tolerant recompute + caching
  massprops/          Mass properties from meshes
  library/            Component library (SQLite + retrieval)
  embed/              Text embedding for library search
  grammar/            JSON Schema + GBNF for AI-constrained output
  ron-check/          CLI validator
dist/APRO-CAD-Windows/  Packaged Windows build
AI_INSTRUCTIONS.md    Schema reference for the AI assistant
```

`AI_INSTRUCTIONS.md` is the authoritative schema reference — keep it in sync
when the document model changes, since both the assistant and the packaged
binary read it.
