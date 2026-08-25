# APRO CAD

Parametric CAD for rocket hardware and general mechanical parts. Documents are written in RON (Rusty Object Notation) and evaluated into 3D meshes by the Truck B-Rep kernel, rendered live in a Tauri v2 desktop app with an AI assistant.

---

## What It Is

APRO CAD is a text-first, parametric CAD application. Instead of drawing, you describe geometry in RON — a readable, diff-friendly data format — and the app turns it into a renderable 3D mesh. It is built around a small Rust workspace with a Tauri v2 desktop shell.

The core idea: **every solid is an ordered stack of operations** (`SolidOp`), evaluated through a single pipeline. Aerospace parts (nose cones, tanks, nozzles, fins) are convenience shorthands that expand into the same operation stacks.

---

## How It's Built

| Layer | Technology |
|---|---|
| App shell | Tauri v2 (WebView2 on Windows) |
| Frontend | Plain HTML/CSS/JS, Three.js (r163) via importmap, no bundler |
| Geometry kernel | Truck 0.6 (B-Rep revolve/extrude, tessellation) |
| Data format | RON via serde, externally-tagged enums |
| Crates | `geometry`, `kernel`, `document`, `features`, `recompute`, `massprops`, `ron-check`, `src-tauri` |

Data flow: RON document → serde parse → SolidOp evaluation → mesh buffers + mass properties → Tauri IPC → Three.js scene.

---

## Capabilities

### Document Model
- `Vehicle(...)` — top-level assembly with named, 6-DOF-transformed components
- `Component(name, transform, kind)` — a single part with position + rotation
- `Units` enum: Millimeters, Centimeters, Meters, Inches, Feet
- `//` comments are valid inside RON documents
- Materials are first-class strings on components (used by mass properties)

### Solid Operations (the primary modeling path)
| Op | Description |
|---|---|
| `Extrude` | Sweep a 2D profile linearly to a height, optional taper |
| `Revolve` | Rotate a profile around the Z axis (partial or full angles) |
| `RevolveChain` | Concatenated revolve segments forming a single solid (avoids shoulder artifacts) |
| `Loft` | Blend between two profile shapes (mesh path) |
| `Sweep` | Sweep a profile along a `Line` path |
| `TransformOp` | Translate/rotate/scale within an op stack |

Profiles are point lists (`Points([(x,y),...])`); the `(height, radius)` convention for revolve profiles is confirmed and documented.

### Aerospace Shorthands (expand to SolidOps)
- `NoseCone` — Conical, Ogive, Von Kármán, Power, Haack, Parabolic profiles, with micro-tip radius for pole stability
- `BodyTube`, `Transition`, `Tank` (Hemispherical / Ellipsoidal domes)
- `Nozzle` — Conical and Bell, with throat/expansion ratio/percent-bell parameters
- `FinSet` — NACA airfoil fins

### Application Features
- **Live evaluation**: auto-re-evaluates 800 ms after you stop typing, plus manual Evaluate
- **Two-panel layout**: RON editor / AI chat tabs on the left (resizable), 3D viewport on the right
- **Properties panel**: component property tables with inline editing (numeric, text, and multi-point profiles), add/remove Solid ops from the UI
- **Viewport**: orbit/pan/dolly controls, grid/axes/wireframe toggles, zoom-to-fit, camera reset
- **Undo/redo** history (50 entries), RON format button, keyboard shortcuts (Ctrl+Enter evaluate, Ctrl+Z/Y undo/redo, Ctrl+F format)
- **Export**: `.STL` and `.STEP` (STEP via truck-stepio)
- **Status bar**: vertex/face counts, component count, cache hit rate, mass + center of mass
- **AI Assistant** (OpenAI-compatible API):
  - Plan mode — design discussion
  - Edit mode — proposes RON changes with pre-applied preview
  - Chat bubbles with markdown formatting, collapsible RON blocks, and an **Implement** button to apply proposed code
  - Diff stats (+added / -removed lines) and Accept/Decline per proposal
  - Animated "thinking" indicator, API key/model/endpoint settings stored in localStorage
  - Grounded by `AI_INSTRUCTIONS.md` — a schema reference loaded into the system prompt

### Tooling
- `ron-check` CLI — validate RON documents without launching the app (exit 0/1)
- Golden RON test corpus — parser-verified example documents used as regression tests and few-shot references
- **120+ automated tests** across all crates

---

## Strengths

1. **Text-first parametric modeling** — RON is diffable, reviewable, version-controllable, and generated/patchable by AI. This is the fundamental differentiator vs. GUI-only CAD.
2. **Stable, swappable kernel** — the `kernel` crate wraps Truck behind a thin interface; geometry is pure math with zero kernel dependencies, so the kernel can be replaced without touching the frontend or document model.
3. **Mesh-only pipeline by default** — evaluation goes straight to tessellated meshes, making interactive editing fast and avoiding heavy B-Rep operation chains for display.
4. **AI-native design** — the chat interface, schema-grounded instructions file, ron-check validation loop, and one-click Implement flow are purpose-built to make an LLM a competent CAD operator.
5. **Shorthand → full pipeline unification** — aerospace parts are not special-cased in the frontend; they expand to the same op stacks as hand-written solids, so the whole feature set benefits from one evaluator.
6. **Robust parser handling** — forgiving aliases, comment stripping, dual-format point parsing, and a validation CLI make the tool resilient to both human and AI input errors.
7. **Fast feedback loop** — debounced auto-eval, editable property tables, and per-component caching (visible in the status bar) keep iteration quick.

---

## Weaknesses & Known Limitations

1. **Stub operations error at evaluation**: `Shell`, `Boolean`, `Fillet`, `Chamfer` exist in the enum but return "not yet supported (Truck 0.6 limitation)". The docs tell the AI never to generate them.
2. **No B-Rep loft**: `Loft` uses direct mesh generation (truck 0.6 cannot stitch a capped lofted solid). Same for fins.
3. **No boolean/union** of solids — multi-segment sweeps and true shelling are impossible today.
4. **STEP export only works for shorthands** — the mesh pipeline cannot reconstruct B-Rep solids for Solid kind components, so general Solid parts can't round-trip to STEP.
5. **Revolve profile limits**: very dense collinear point profiles (>~42 points) can fail `try_attach_plane`.
6. **No wall thickness from within Solid ops** — hollow parts rely on the (unimplemented) `shell` op; profiles must encode thickness manually.
7. **Frontend is a single large JS file** — no module structure, no bundler, no syntax highlighting in the RON editor.
8. **AI assistant requires an API key + internet**, and its output quality depends heavily on `AI_INSTRUCTIONS.md` staying in sync with the schema.
9. **Profile point-order conventions are only fully confirmed for Revolve** — Extrude/Loft/Sweep order is documented by assumption, not verified.
10. **No undo/redo for chat-proposed edits that were never accepted** (acceptance is a separate explicit step).
11. **Exports are hardcoded to a fixed path** (`C:\Users\henry\Documents\APRO-export.*`) with no file picker.
12. **Debug-mode app only so far** — no installer, no release packaging, no icons, dev-server-less local file serving.

---

## Potential

1. **Kernel upgrade path** — the abstraction layer makes moving to Truck 0.7+ or a fork (monstertruck) a contained change, which would unlock: true boolean ops, capped loft, shell, fillet/chamfer, and B-Rep STEP export for Solid components.
2. **Adaptive tessellation** — coarse tolerance during editing, fine tolerance at export; improves both interactivity and output quality.
3. **Schema-first AI integration** — derive `JsonSchema` on all types for an auto-generated schema dump, keeping `AI_INSTRUCTIONS.md` always in sync with the code.
4. **General CAD depth** — profile point-order verification, closed-profile revolve (wall thickness without `shell`), syntax highlighting, multi-select editing, and more ops (arrays, mirrors, patterns) would push it toward production CAD territory.
5. **Automated RON round-trip testing** — generate → ron-check → evaluate → export → reimport loops as CI.
6. **Collaboration** — the text/RON core makes code-review-style design workflows, per-component blame, and generative design pipelines natural next steps.
7. **Export UX** — file pickers, export presets, and STEP support for all Solid kinds once the kernel matures.

---

## Running It

```powershell
# Dev mode (Tauri CLI required)
npx tauri dev

# Build (Rust only)
cargo build --workspace

# Run the built binary
target\debug\apro-cad.exe

# Validate a RON document
cargo run --bin ron-check -- path\to\file.ron

# Full test suite
cargo test --workspace
```

Frontend-only changes (HTML/JS/CSS) take effect on app restart — no rebuild needed, since Tauri serves `src/` from disk.

---

## Project Structure

```
src/                  Frontend (index.html, main.js)
src-tauri/            Tauri shell + IPC commands
crates/
  geometry/           Pure-math profiles (nose cones, fins, etc.)
  kernel/             Truck wrapper: ops, tessellation, mesh data
  document/           RON document types, SolidOp, patch system, golden tests
  features/           Shorthand expansion, SolidOp evaluation
  recompute/          Build-failure tolerant recompute
  massprops/          Mass properties from meshes
  ron-check/          CLI validator
AI_INSTRUCTIONS.md    AI system prompt / schema reference
```
