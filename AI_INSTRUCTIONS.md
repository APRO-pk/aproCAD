# APRO CAD — AI Instructions

## Overview
Parametric CAD tool. RON documents describe parts; the Truck CAD kernel evaluates geometry into 3D meshes. General-purpose — brackets, housings, gears, enclosures, fixtures, rocket hardware, anything buildable from extrude/revolve/loft/sweep primitives.

## Architecture
7 Rust crates + Tauri v2 shell + HTML/JS frontend. Frontend sends RON over Tauri IPC, receives mesh buffers + mass properties.

## Document Types & Tauri Commands (ROOT-TYPE AMBIGUITY — READ CAREFULLY)

There are exactly **two** document types and **two** IPC commands, and they are **not interchangeable**:

| RON type | Tauri command | What it accepts |
|---|---|---|
| `Vehicle(...)` | `evaluate_vehicle` | Full assembly with `components: [...]` |
| `Component(...)` | `evaluate` | Single part (no wrapper) |

**Vehicle** is `Vehicle(name: String, units: Units, parameters: Option<[Parameter]>, components: [Component, ...])` — the top-level assembly wrapper.
**Component** is `Component(name: String, material: String, color: Option<String>, visible: bool, transform: Transform, kind: ComponentKind)` — a single part. `color` and `visible` are optional (`color` defaults to `None`, `visible` to `true`).

Units enum: `Millimeters | Centimeters | Meters | Inches | Feet`
Transform: `(position: (f64,f64,f64), rotation: (f64,f64,f64))` — both fields compulsory, use `None` only inside `Option` wrappers.

⚠ ALL struct fields use `name: value` syntax. Never use positional arguments like `Vehicle("name", ...)` — RON requires every field to be labelled.

Rule: `Vehicle` is **never** nested inside `Component.kind`. A `Component.kind` takes a `ComponentKind` variant only (NoseCone, BodyTube, Solid, etc.).

The frontend auto-detects which command to use: if the RON (after stripping `//` comments) starts with `Vehicle(`, it calls `evaluate_vehicle`; otherwise `evaluate`. 

**CONFIRMED (2026-07): `Vehicle(...)` parses correctly as a document root.** An earlier `Expected struct Component but found Vehicle` error came from the frontend routing to the wrong IPC command (because `//` comments prefixing the RON made `startsWith('Vehicle(')` fail), not from a missing parser feature. Fixed by stripping `//` comment lines before the auto-detect check.

## Solid Modeling — PRIMARY path
`kind: Solid([op, op, ...])` — an ordered, comma-separated stack of operations. Use this for all general CAD.

### Working ops (evaluate successfully):
```
Extrude(profile: Points([(x,y),...]), height: 200.0, direction: None, taper: None)
Revolve(profile: Points([(x,y),...]), angle: 360.0, axis: Some(Z))
RevolveChain(segments: [[(x,y),...],[(x,y),...]], angle: 360.0)
Loft(profiles: [Points([...]), Points([...])], guide_curves: None)
Sweep(profile: Points([(x,y),...]), path: Path3D, twist: None)
Boolean(kind: Union|Difference|Intersection, target: Component("Name")|This)
Hole(diameter: 12.0, depth: 10.0, axis: Z)
BoltCircle(count: 6, pitch_diameter: 60.0, hole_diameter: 8.0, depth: 8.0)
RectPattern(x_count: 4, y_count: 4, spacing_x: 20.0, spacing_y: 20.0, hole_diameter: 6.0, depth: 6.0)
TransformOp(translate: Some((0,0,0)), rotate: None, scale: None)
```

**Machined features (2026-08): SUPPORTED.** These cut holes/patterns into the current op stack (they behave exactly like a `Boolean` Difference against a cylinder; they must come AFTER an Extrude/Revolve/Loft/Sweep that creates the body — never first). All are centred on the local origin / Z axis; the pattern is centred on Z; `depth` is how far the hole extends upward from Z=0 (use a depth ≥ the body thickness to punch through).
- `Hole(diameter, depth, axis: X|Y|Z)` — one drilled hole.
- `BoltCircle(count, pitch_diameter, hole_diameter, depth)` — `count` (must be ≥ 3) holes on a ring; ideal for mounting flanges, burp discs, motor mounts.
- `RectPattern(x_count, y_count, spacing_x, spacing_y, hole_diameter, depth)` — a centred rectangular grid of holes (lightening / bolt grids). Put the op straight after the body; the cut drops vertex mass so `evaluate_vehicle` gives a lighter, realistic part.

**CONFIRMED (2026-07): Revolve profile points are `(height, radius)`, NOT `(radius, height)`.** The first value is the position along the revolve axis (Z); the second is the radial distance from the axis. Getting this backwards produces a squashed, oversized result (wide flat disc instead of a tall part) rather than a parse error. This convention applies to `Points` used in `Revolve` and `RevolveChain`; not yet independently verified for `Extrude`/`Loft`/`Sweep` — don't assume they share this order until tested.

**Boolean (2026-08): SUPPORTED.** `Boolean(kind: Union|Difference|Intersection, target: ...)` cuts/joins the current op stack's mesh against another mesh. `target: Component("Name")` references a sibling component by name; `target: This` references the current stack itself. Rules:
- Boolean must NOT be the first op in a Solid stack (it needs an existing mesh to modify).
- The target component may appear anywhere in the vehicle; the engine builds dependencies first. Circular references are detected and yield no geometry.
- `SolidRef(This)` = current stack (e.g. subtract a previously extruded cutter). `SolidRef(Component("OtherName"))` = sibling component.
- **The cutter's own transform is IGNORED.** The target resolves in its LOCAL frame (as if at origin). The boolean result is positioned by the REFERENCING component's transform. To cut a hole at a specific spot, position the cutter via the referencing component's geometry/ops — NOT by moving the cutter component.
- **The target component still renders in the assembly** at its own transform (it is not hidden). A bore that cuts a hole will visually fill the hole. Either place the cutter away from the hole location in the assembly, or accept that both meshes appear.
- Use for drilling holes, cutting fins into a body, or joining parts into one mesh.

Worked example — a block with a drilled hole (bore as a separate component, cut via Difference):
```
Vehicle(
  name: "HoleBlock",
  units: Millimeters,
  components: [
    Component(name: "Block", material: "Al-6061-T6", visible: true,
      transform: (position: (0.0, 0.0, 0.0), rotation: (0.0, 0.0, 0.0)),
      kind: Solid([
        Extrude(profile: Rectangle(width: 60.0, height: 60.0, corner_radius: None), height: 20.0, direction: None, taper: None),
        Boolean(kind: Difference, target: Component("Bore")),
      ])),
    Component(name: "Bore", material: "Al-6061-T6", visible: true,
      transform: (position: (0.0, 0.0, 500.0), rotation: (0.0, 0.0, 0.0)),
      kind: Solid([
        Extrude(profile: Circle(radius: 10.0), height: 20.0, direction: None, taper: None),
      ])),
  ],
)
```
The bore sits at z=500 in the assembly (so it does not visually fill the hole) but its LOCAL cylinder overlaps the block, cutting an r=10 hole through it. The hole appears at the Block's local origin.

### Stub ops — unimplemented, return error on evaluation. Never generate:
`Shell(thickness: f64, faces: None)`
`Fillet(radius: f64, edges: EdgeRef)`
`Chamfer(distance: f64, edges: EdgeRef)`

All three above cause `"not yet supported (Truck 0.6 limitation)"` at evaluation time.

### Referenced enum: EdgeRef
```
EdgeRef = All | Indices([u32, ...])
```
Not `Option<EdgeRef>`. Never use `None` — use `All` or `Indices([0, 1])`.

### Referenced enum: Path3D (full shape)
```
Path3D = Line(start: (f64,f64,f64), end: (f64,f64,f64))           // struct variant, use ()
       | Arc(center: (f64,f64,f64), radius: f64, start_angle: f64, end_angle: f64) // struct variant, use ()
       | Spline([(f64,f64,f64), ...])                              // tuple variant, use []
       | Helix(radius: f64, pitch: f64, turns: f64)               // struct variant, use ()
```
IMPORTANT: All struct variants in RON use `()` parentheses, never `{}` braces. This applies to enum variants like `Line(...)`, `Arc(...)`, `Helix(...)` and also to SolidOp variants like `Extrude(...)`, `Revolve(...)`, etc.

Wrong: `Line((0,0,0),(0,0,17))` — positional args for a named-field variant → "Expected identifier"

Usage in `Sweep` (profile is the cross-section; the path leads away from it):
- `path: Line(start: (0,0,0), end: (0,0,17))` — straight
- `path: Arc(center: (40,0,0), radius: 30, start_angle: -90, end_angle: 90)` — circular arc **in the XY plane** at height `center.z`
- `path: Spline([(0,0,0),(20,10,30),(-10,25,70)])` — Catmull-Rom curve **through** every point → smooth bends (sprung fuel lines, ducts)
- `path: Helix(radius: 20, pitch: 15, turns: 3)` — spring/coil
Curved paths let you make pipes, ducts, springs and feed lines a straight `Line` cannot.
Wrong: `Line { start:..., end:... }` — RON doesn't use `{}` → "Expected opening `(`"
Right: `Line(start: (0.0,0.0,0.0), end: (0.0,0.0,17.0))`

### Referenced enum: SolidRef
```
SolidRef = Component("name") | This
```

### No built-in primitives
No `Box`/`Cylinder`/`Sphere`/`Cube`. Build from ops:
- Box / plate → `Extrude` a rectangular `Points` profile
- Cylinder / solid boss → `Revolve` a rectangular profile with one edge on the axis
- Tube / ring → `Revolve` a rectangular profile offset from axis by inner radius
- Bracket → `Extrude` an arbitrary polygon `Points` profile
- Tapered part → `Extrude` with `taper: Some(angle)` or `Loft` between differently-sized profiles

## Aerospace Shorthands — SECONDARY path
Named `ComponentKind` variants for rocket geometry. Exact field names, no defaults. **Use the variant name directly — never prefix with `ComponentKind.` or `ComponentKind::`.**

Correct: `kind: BodyTube(BodyTubeParams(...))`
Wrong:  `kind: ComponentKind.BodyTube(...)` → "Found invalid std identifier" parse error

### ComponentKind — RON usage (variant name only, no path prefix)
```
NoseCone(NoseConeParams(profile: NoseProfile, length: f64, base_radius: f64, wall: f64, material: String))
BodyTube(BodyTubeParams(length: f64, radius: f64, wall: f64, material: String))
Transition(TransitionParams(length: f64, start_radius: f64, end_radius: f64, wall: f64, material: String))
Tank(TankParams(radius: f64, cylindrical_length: f64, dome: DomeKind, wall: f64, material: String))
Nozzle(NozzleParams(kind: NozzleKind, throat_radius: f64, expansion_ratio: f64, percent_bell: f64, chamber_radius: f64, wall: f64, material: String))
FinSet(FinSetParams(count: u32, root_chord: f64, tip_chord: f64, span: f64, sweep: f64, airfoil: AirfoilParams, thickness: f64, material: String))
Solid([SolidOp, ...])
```

### Referenced enums — variant name only, no path prefix
```
NoseProfile: Conical | Ogive | VonKarman | Power(n: f64) | Haack(c: f64) | Parabolic(k: f64)
NozzleKind:  Conical | Bell | Moc
DomeKind:    Hemispherical | Ellipsoidal(ratio: f64)
AirfoilParams(family: NACA(digits: String))
```

## RON Syntax Rules
- `[f64; 2]` is tuple `(x,y)`, never `[x,y]`
- `Option<T>`: `None` or `Some(value)` — no extra parens
- All struct variants use `(field: value, ...)` syntax — never `{}`
- All tuple variants use `(value, value, ...)` — never `[]`
- All list types use `[item, item, ...]`
- `material` is always a `String`
- All fields compulsory, no defaults
- `//` comments are valid in RON
- 4-space indent
- ⚠ **All `Vehicle` and `Component` fields must use named `field: value` syntax.** Never omit the field name. Wrong: `Vehicle("Pizza Box", ...)`. Right: `Vehicle(name: "Pizza Box", units: Millimeters, components: [...])`. Wrong: `Component("Base", ...)`. Right: `Component(name: "Base", transform: (position: (0,0,0), rotation: (0,0,0)), kind: ...)`.

## Parameters — equations for numeric relationships (2026-08)
`Vehicle` accepts an optional `parameters` block: a list of named values, each either a literal number or a quoted equation string evaluated against the other parameters. This is the right tool when a dimension derives from another dimension (inner diameter from outer minus walls, fin chord from body length, etc.).

**MUST use the list form** (the schema/GBNF only allows properties lists — the pretty `Parameters(body_od: 98.0, ...)` block form is only accepted on manual input, not from the AI):

```
Vehicle(
    name: "Demo",
    units: Millimeters,
    parameters: Some([
        Parameter(name: "body_od", value: 98.0),
        Parameter(name: "wall", value: 2.0),
        Parameter(name: "body_id", value: "body_od - 2 * wall"),
        Parameter(name: "fin_root", value: "body_od * 1.8"),
    ]),
    components: [...],
)
```

Equation rules:
- Names are bare identifiers (lowercase recommended). The `value` of a parameter is either a plain float (`98.0`) or a QUOTED equation string (`"body_od - 2 * wall"`).
- Operators: `+ - * / ^` (power, right-assoc). Parentheses and unary minus allowed. **Multiplication must be explicit**: `2 * wall`, never `2wall`.
- Functions: `abs sqrt sin cos tan min max pow`. Constants: `pi`, `e`.
- Forward references are fine (`body_id` may reference `wall` defined after it). Circular references and unknown names are REJECTED (the whole patch fails) — check your equation names carefully.
- Prefer an equation over a hard-coded number when the user gives a relationship (e.g. "inner = outer − 2 × wall"). Then one edit to `body_od` updates every dependent dimension automatically.
- Changing a parameter recomputes every component that references it; cache fingerprints fold the resolved values in.
- `Profile::UserFunction { expr, variable, range, samples }` evaluates `expr` against the parameters plus the free `variable` — useful for arbitrary curves (airfoils, ogives, racks).

## Component colors — per-part display colors (2026-08)
Every `Component` accepts an optional `color` field. The renderer shows each part in its own color; when `color` is absent a stable color is derived from the material name.

- Value forms: hex `"#ff8800"`, short hex `"#f80"`, or a CSS name `"red"`, `"gold"`, `"navy"`, ... (case-insensitive).
- Set colors when the user asks ("make the nose red, body silver, fins black") or when distinct parts benefit from visual distinction.
- Example: `Component(name: "Nose", material: "Al-6061-T6", color: Some("#ff8800"), visible: true, transform: ..., kind: ...)`.
- Edit an existing part's color with a patch: `SetProperty(component_name: "Nose", key: "color", value: "#ff8800")`. Use value `"auto"` to clear back to material-derived coloring.
- Colors do not affect geometry or mass — they are display-only.

## Known-Unstable / High-Risk List (last updated: 2026-08-21)
These are areas where the AI often generates wrong code. Cross-check output carefully:

| Risk | Issue | Status |
|---|---|---|
| `Vehicle` nesting | AI puts `Vehicle()` inside `Component.kind` | FIXED in docs (v2) |
| `EdgeRef` as `None` | `EdgeRef` is not `Option`, use `All` or `Indices(...)` | FIXED in docs |
| `Line` syntax | Wrong `Line{...}` or `Line((...),(...))` instead of `Line(field: .., field: ..)` | FIXED in docs |
| `{}` vs `()` | RON uses `()` for all struct-like enum variants, not `{}` | Doc fix pending — common mistake |
| Stub ops | Shell/Fillet/Chamfer are in the enum but error at eval | Documented above; Boolean is now supported |
| Sweep editing | Property table cannot edit Sweep fields (falls back to RON editor) | Known limitation |
| `Fillet`/`Chamfer` with `edges: None` | Most common parse error today | See EdgeRef above |
| No `Box`/`Cube` primitives | These don't exist; build from Extrude/Revolve | Documented above |
| Profile point order | `(height, radius)` not `(radius, height)` for Revolve points; backwards = squashed shape | CONFIRMED |
| `(x,y)` vs `(height,radius)` | `Extrude`/`Loft`/`Sweep` point order not yet independently verified | UNTESTED — assume same `(x,y)` convention |
| `ComponentKind.` prefix | AI writes `kind: ComponentKind.BodyTube(...)` instead of `kind: BodyTube(...)` — "Found invalid std identifier" parse error | COMMON — see ComponentKind docs |
| Vehicle/Component positional args | AI writes `Vehicle("name", ...)` or `Component("name", ...)` instead of `Vehicle(name: "...", ...)` or `Component(name: "...", ...)` — causes `Expected identifier` parse error | COMMON — see RON Syntax Rules |
| `units` field wrong type | `"cm"` or `"mm"` string instead of `Units` enum (`Millimeters`/`Centimeters`/etc.) | COMMON — see Vehicle docs |
| `transform: None` | `transform` is a `Transform` struct, not `Option`; use `(position: (0,0,0), rotation: (0,0,0))` | COMMON — see Component docs |
| Vehicle routing | `//` comments before `Vehicle(...)` caused wrong IPC command; fixed with `stripRonComments()` | FIXED 2026-07 |
| Parameters pretty form | AI writes `Parameters(body_od: 98.0)` instead of the list form `parameters: Some([Parameter(name: "body_od", value: 98.0), ...])` | COMMON — pretty block only accepted on manual input; see Parameters section |
| Parameters equation syntax | `2wall` (missing `*`), unquoted equations, or referencing an unknown name → patch rejected | COMMON — see Parameters section |

## Validation Tool
Use `cargo run --bin ron-check path/to/file.ron` to validate a RON file against the parser without launching the full app. Returns 0 on success, prints parse errors on failure. This is the fastest way to check AI output.

## Style
- 4-space indent for RON
- Error messages: lowercase, no trailing punctuation
- Multi-part designs: always wrap in `Vehicle(...)` at top level, never inside `Component.kind`
