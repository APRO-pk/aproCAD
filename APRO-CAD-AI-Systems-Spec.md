# APRO CAD: AI Systems Implementation Spec

Target state: a text-first parametric CAD tool where the AI is a reliable, verifiable design operator rather than an unpredictable code generator.

Version 1.0 | For implementation against the current Rust workspace + Tauri v2 shell.

---

## 0. Build Order and Rationale

| Phase | System | New crates | Blocks | Effort |
|---|---|---|---|---|
| 1 | Component Library + Retrieval | `library`, `embed` | Nothing | Medium |
| 2 | Grammar-Constrained Generation | `grammar` | Nothing | Small |
| 3 | Requirements + Agent Loop | `agent`, `requirements` | Nothing | Medium |
| 4 | Kernel Upgrade (booleans) | none, edits `kernel` | Fit checking, STEP for Solids | Large |
| 5 | Local Model Runtime | `llm-local` | Nothing | Medium |
| 6 | Fine-Tune Pipeline | `corpus-gen` (tooling only) | Phase 1 corpus | Small |

Phases 1 to 3 work against the existing cloud API path and require no local model. They are where the product feel changes. Phase 4 is the hard ceiling on design validation. Phases 5 and 6 are distribution and polish.

---

## 1. System A: Component Library and Retrieval

### 1.1 Purpose

Three jobs, in order of value:

1. **Few-shot grounding.** Retrieved snippets injected into the prompt improve AI output quality more than fine-tuning does, and adapt per user with no retraining.
2. **Human reuse.** Load a saved part instead of re-declaring it.
3. **Corpus generation.** Every saved component becomes a validated training pair later.

### 1.2 Storage layout

```
%APPDATA%/APRO-CAD/library/
  index.sqlite            Metadata, params, embeddings (BLOB), FTS5 table
  components/
    <ulid>.ron            The RON snippet (single Component or SolidOp stack)
    <ulid>.png            256x256 rendered thumbnail
  collections.json        User folders / tags
```

SQLite over a directory scan: you need range queries on numeric parameters, and FTS5 gives you keyword search for free. Use `rusqlite` with the `bundled` feature so there is no external dependency in the installer.

### 1.3 Entry schema

```rust
pub struct LibraryEntry {
    pub id: Ulid,
    pub name: String,
    pub description: String,          // user text, feeds the embedding
    pub tags: Vec<String>,
    pub kind: EntryKind,              // NoseCone | BodyTube | Tank | Nozzle | FinSet | Solid | Assembly
    pub ron: String,                  // the snippet, must pass ron-check
    pub params: ParamVector,          // numeric layer
    pub embedding: Option<Vec<f32>>,  // 384-dim, bge-small
    pub mass_props: Option<MassProps>,
    pub thumbnail: Option<PathBuf>,
    pub source: Source,               // UserSaved | Builtin | AIGenerated
    pub created: DateTime<Utc>,
    pub use_count: u32,
}

pub struct ParamVector {
    pub od_mm: Option<f64>,           // outer diameter
    pub id_mm: Option<f64>,           // inner diameter
    pub length_mm: Option<f64>,
    pub mass_g: Option<f64>,
    pub throat_mm: Option<f64>,       // nozzle only
    pub expansion_ratio: Option<f64>, // nozzle only
    pub material: Option<String>,
    pub custom: BTreeMap<String, f64>,
}
```

`ParamVector` is extracted automatically at save time by evaluating the snippet and reading the bounding box plus mass properties, then overlaying any explicit shorthand parameters. Do not ask the user to type these in. Anything the user does type should override the extracted value and be flagged as `user_asserted` so you never silently overwrite it on re-index.

### 1.4 The three-layer index

**Do not use embeddings alone.** Embedding models encode "54mm" and "98mm" as near-identical vectors. A pure vector search over parametric components will confidently return the wrong size part. This is the single most common failure in naive CAD-plus-RAG systems.

**Layer 1: Numeric filter (authoritative).**
SQL range query over `ParamVector` columns. Any query containing a dimension must pass through here first as a hard filter, not as a ranking signal.

```sql
SELECT id FROM components
WHERE kind = ?1
  AND od_mm BETWEEN ?2 * 0.98 AND ?2 * 1.02
  AND (?3 IS NULL OR length_mm <= ?3)
```

Tolerance defaults to 2%, exposed as a setting. For "fits inside" queries, use `od_mm <= target_id_mm - clearance` with a default clearance of 0.5mm.

**Layer 2: Semantic rank.**
Cosine similarity over `bge-small-en-v1.5` embeddings of `format!("{} {} {}", name, description, tags.join(" "))`. Applied only to the set surviving Layer 1. If Layer 1 returns nothing, Layer 2 runs unfiltered and results are marked `dimension_mismatch: true` so the UI can say "no exact fit, here is something similar."

Use `fastembed-rs`. The model is 133MB ONNX, runs on CPU in single-digit milliseconds, and requires no GPU. Bundle it, do not download at runtime.

**Layer 3: Keyword fallback.**
FTS5 over name and description. Catches part numbers, internal codes, and anything the embedding model has never seen. Union the results with a lower rank weight.

**Final scoring:**

```
score = 0.5 * numeric_fit + 0.35 * semantic_sim + 0.10 * fts_rank + 0.05 * recency_use_boost
```

where `numeric_fit` is 1.0 for an exact dimensional match, decaying linearly to 0 at the tolerance edge, and 0.3 if the query specified no dimensions at all.

### 1.5 Retrieval into the prompt

On every AI request, before the call:

1. Embed the user's message.
2. Retrieve top-k (default k=4, cap the total at 2000 tokens).
3. Also always include the two most recently *accepted* AI proposals from this session, so the model tracks the user's evolving style within a document.
4. Inject as a `<retrieved_examples>` block ahead of the user turn, each entry as name, description, RON.

Order matters. Put retrieved examples before `AI_INSTRUCTIONS.md` schema content, not after, so the schema is the most recent thing in context.

Log which entries were retrieved on each turn. When a proposal is accepted, increment `use_count` on the retrieved entries. This gives you a free relevance signal to tune the scoring weights against later.

### 1.6 Save flow

Right-click a component in the properties panel, or select an op range in the editor, then "Save to Library."

1. Extract the RON subtree.
2. Run `ron-check` on it in isolation. Reject with a clear error if it does not stand alone (most commonly, a reference to a parent transform).
3. Evaluate it, capture mesh stats and mass properties.
4. Render a 256x256 thumbnail offscreen (reuse the Three.js scene, orthographic, three-quarter view, fit to bounds).
5. Prompt for name, description, tags. Pre-fill all three by asking the model to describe the snippet. Never block on the AI, if it fails or is slow, fall back to the component name.
6. Embed, insert, done.

### 1.7 Builtin corpus

Ship 60 to 100 builtin entries covering the standard ranges: 29/38/54/75/98/152mm body tubes, matching transitions, nose cones in every profile type at fineness ratios 3 through 10, bell and conical nozzles across expansion ratios 4 to 12, tank domes in both types, and a set of NACA fin planforms.

Generate these programmatically (see Section 6.2), validate them all, and mark them `Source::Builtin` so they can be filtered out of the user's own view but still serve retrieval.

---

## 2. System B: Grammar-Constrained Generation

### 2.1 Purpose

Make it structurally impossible for the model to emit invalid RON, hallucinate a `SolidOp` that does not exist, or reach for your stub operations. This eliminates the largest category of agent-loop failure at very low cost, and it works on both the cloud path and any future local path.

Expected effect: zero-shot schema validity moves from roughly 30 to 50% to effectively 100% for syntax, and remains at 100% for enum membership permanently, including after you add new ops.

### 2.2 Grammar generation, not grammar authoring

Do not hand-write a GBNF file. It will drift from the schema and you will be back to weakness #8. Generate it.

1. Derive `schemars::JsonSchema` on every type in the `document` crate alongside the existing serde derives.
2. New crate `grammar` with a build step that walks the JSON Schema and emits GBNF.
3. A test asserts the emitted grammar parses and accepts every document in the golden corpus. If someone adds a `SolidOp` variant without updating anything, the corpus test still passes but a second test asserting `grammar_covers_all_variants()` fails loudly.

### 2.3 Suppressing stub ops

`Shell`, `Boolean`, `Fillet`, and `Chamfer` exist in the enum but error at evaluation. Models will reach for them constantly because they exist in every other CAD tool's vocabulary.

Add an attribute:

```rust
#[derive(Serialize, Deserialize, JsonSchema)]
pub enum SolidOp {
    Extrude { .. },
    Revolve { .. },
    // ...
    #[schemars(skip)]
    #[cad(unimplemented = "Truck 0.6 limitation")]
    Shell { .. },
}
```

The grammar generator excludes anything marked `unimplemented`. The document parser still accepts them, so existing files and hand-written experiments do not break. The AI simply cannot produce them. When the kernel upgrade lands, delete one attribute per op and the capability appears in the grammar automatically.

### 2.4 Applying the constraint

**Cloud path.** Most OpenAI-compatible endpoints support `response_format: { type: "json_schema", ... }`. Emit the same schema, have the model produce JSON, and convert JSON to RON deterministically on the Rust side. This is more reliable than asking any provider to honour a custom grammar. Keep RON as the storage and display format, the JSON is a wire detail the user never sees.

**Local path.** llama.cpp accepts GBNF directly via the `grammar` parameter. Pass the generated file. No JSON intermediate needed.

**Degradation.** If the endpoint rejects the schema parameter, fall back to unconstrained generation plus a repair pass (Section 3.5). Surface this in the UI as a small "unconstrained" badge on the model selector so the user knows why quality dropped.

### 2.5 Structured edits, not whole-document rewrites

Constrain the model to emit a **patch**, not a full document. You already have a patch system in the `document` crate. Expose it as the AI's output type:

```rust
pub enum Patch {
    AddComponent { at: usize, component: Component },
    RemoveComponent { id: ComponentId },
    ReplaceComponent { id: ComponentId, component: Component },
    SetParam { path: ParamPath, value: Value },
    AddOp { component: ComponentId, at: usize, op: SolidOp },
    RemoveOp { component: ComponentId, index: usize },
    ReorderOps { component: ComponentId, order: Vec<usize> },
}
```

Benefits: token cost drops by an order of magnitude on large documents, the diff view becomes exact rather than textual, undo/redo of AI edits becomes trivial (each patch is already a history entry), and the model cannot accidentally destroy an unrelated component while editing one.

---

## 3. System C: Requirements Block and the Agent Loop

### 3.1 The termination problem

"Continues until it believes it has completed the design" is the failure mode. Model self-assessment of completion is unreliable in both directions: it stops early on hard problems and polishes forever on easy ones. Replace belief with a checklist that is machine-evaluable.

### 3.2 The Requirements block

A new optional top-level RON node, sibling to `Vehicle`:

```ron
Requirements(
    // Hard geometric constraints
    body_od: Exactly(98.0),
    total_length: AtMost(1800.0),
    fineness_ratio: AtLeast(8.0),

    // Mass
    dry_mass: AtMost(4.2),
    cg_from_nose: Between(0.55, 0.68),   // fraction of total length

    // Propulsion
    nozzle_expansion: Exactly(6.5),
    throat_diameter: Between(11.0, 13.0),

    // Structural, requires external analysis
    min_wall_thickness: AtLeast(1.6),

    // Free-text goals the loop cannot check, shown to the user as manual review items
    notes: [
        "Fin can must accept 3x 3mm G10 fins",
        "Recovery bay accessible without tool",
    ],
)
```

Constraint kinds: `Exactly(f64)` with tolerance, `AtLeast`, `AtMost`, `Between(f64, f64)`, `OneOf([..])`.

Every constraint evaluates to `Pass`, `Fail { actual, target }`, or `Unverifiable { reason }`. The `notes` array is always `Unverifiable` and always surfaces as a manual checklist. This is the honest boundary and it should be visible.

### 3.3 The verifier set

What you can check today:

| Check | Source | Status |
|---|---|---|
| Schema validity | `ron-check` | Available |
| Evaluation success | `features` evaluator | Available |
| Mass, CG | `massprops` | Available |
| Bounding box, length, OD | mesh AABB | Available |
| Fineness, aspect ratios | derived | Available |
| Inertia tensor | `massprops` extension | Small addition, do it |
| CP and static margin | Barrowman, new | Medium, high value |
| Wall thickness | requires shell or boolean | **Blocked on kernel** |
| Part interference | requires boolean | **Blocked on kernel** |
| Fit and clearance | requires boolean | **Blocked on kernel** |

The three blocked checks are the real ceiling on this system and the strongest argument for the Truck upgrade. Until then, be explicit in the UI that interference is unchecked.

Add Barrowman CP and static margin. It is a few hundred lines of pure math against geometry you already have, it is the single most meaningful design check in amateur and small commercial rocketry, and it makes the requirements block feel like it understands rockets rather than boxes.

### 3.4 Loop architecture

```
┌─────────────────────────────────────────────────┐
│  PLAN                                           │
│  Model reads Requirements + current document,   │
│  emits an ordered task list. Shown to user.     │
│  User may edit or approve before execution.     │
└────────────────────┬────────────────────────────┘
                     ▼
┌─────────────────────────────────────────────────┐
│  STEP  (per task)                               │
│  1. Retrieve relevant library entries           │
│  2. Constrained generation of a Patch           │
│  3. Apply to a scratch document                 │
│  4. ron-check                                   │
│  5. Evaluate                                    │
│  6. Score against Requirements                  │
└────────────────────┬────────────────────────────┘
                     ▼
              ┌──────────────┐
              │ All pass?    │
              └──┬────────┬──┘
                 │ no     │ yes
                 ▼        ▼
        ┌────────────┐   ┌─────────────────────┐
        │  REPAIR    │   │  next task, or DONE │
        │ (≤3 tries) │   └─────────────────────┘
        └─────┬──────┘
              │ still failing
              ▼
        ┌─────────────────────────────┐
        │ ESCALATE to user with the   │
        │ exact failing constraint    │
        └─────────────────────────────┘
```

### 3.5 Repair behaviour

Feed the failure back as structured data, not prose. The repair prompt gets:

- The exact validator error, verbatim, including line and column from `ron-check`
- The specific failing constraint with actual versus target
- The patch that caused it
- An instruction to fix only that

Three attempts per task, then escalate. Track the failure reason. If you see the same `try_attach_plane` failure repeatedly, that is your 42-point collinear profile limit and the repair loop cannot solve it, so hard-code a pre-check that catches dense collinear profiles before evaluation and rewrites them by decimation.

### 3.6 Budgets and stop conditions

Every run is bounded on all four axes, defaults configurable:

```rust
pub struct AgentBudget {
    pub max_steps: u32,          // 25
    pub max_tokens: u64,         // 150_000
    pub max_wall_seconds: u64,   // 300
    pub max_repairs_per_step: u32, // 3
}
```

Terminate on: all requirements `Pass` or `Unverifiable`, any budget exhausted, user cancel, or no measurable progress across three consecutive steps (constraint scores unchanged). The last one catches the polish-forever loop.

### 3.7 UI behaviour

Non-negotiable properties, in priority order:

1. **Never mutate the live document mid-run.** The agent works on a scratch copy. The user's document changes only on accept.
2. **Cancel is instant.** A cancel token checked between every step and passed into the HTTP client.
3. **Streaming visibility.** Show the task list with live status per task, the constraint scoreboard updating in place, and a collapsible log of each patch attempted. The user must be able to see it thinking without reading a wall of tokens.
4. **Every step is a checkpoint.** The user can rewind to any step and take over manually. This is what makes the loop safe to trust.
5. **Single accept at the end,** producing one undo entry, plus a per-step accept for people who want finer control.
6. **Distinguish green from validated.** The completion banner reads "All checkable requirements met" and lists the `Unverifiable` items and the blocked checks (interference, wall thickness) alongside. Never let the checkmarks imply flight readiness.

---

## 4. System D: Local Model Runtime

### 4.1 Model choice

Target **Qwen2.5-Coder** (7B and 14B), not an abliterated general model.

Reasoning: refusal behaviour in this domain clusters around propellant chemistry, grain geometry tied to range, and guidance. Emitting RON for nose cones, tanks, and nozzle contours is geometry, and geometry is not what trips safety training. Coder-family models have very little refusal disposition to begin with, and a LoRA on your own corpus dominates whatever residual there is. Abliterated models pay a measurable 5 to 15% penalty on exactly the structured-output and coding benchmarks you depend on, which is the wrong trade.

Licensing also matters now that you are incorporating: Qwen2.5-Coder is Apache 2.0 and redistributable inside a commercial desktop app. Llama's licence is less clean for that.

| Tier | Model | Quant | Disk | VRAM | Notes |
|---|---|---|---|---|---|
| Minimum | Qwen2.5-Coder-1.5B | Q4_K_M | 1.1GB | 3GB | Autocomplete and param edits only |
| Default | Qwen2.5-Coder-7B | Q4_K_M | 4.4GB | 8GB | Full generation, agent loop viable |
| High | Qwen2.5-Coder-14B | Q4_K_M | 8.9GB | 12GB | Better planning |
| CPU-only | Qwen2.5-Coder-7B | Q4_K_M | 4.4GB | 0 | 3 to 8 tok/s, agent loop impractical |

### 4.2 Runtime

llama.cpp as a Tauri sidecar, not a Rust binding. Reasons: GBNF grammar support is first-class, the binary is prebuilt per platform with CUDA/Metal/Vulkan variants, and a crash in inference does not take down the app.

`mistral.rs` is the pure-Rust alternative if staying in-workspace matters more than grammar support, but you would lose constrained decoding, which is the highest-value feature in this whole document. Do not make that trade.

Spawn with `--server`, talk to it over the same OpenAI-compatible client you already use. The AI assistant code path stays identical, only the base URL changes.

### 4.3 Download and hardware detection

A first-run wizard that:

1. Detects VRAM (`nvml-wrapper` on NVIDIA, `metal` on macOS, fall back to system RAM).
2. Recommends a tier, allows override with a clear warning if the choice will swap.
3. Downloads from Hugging Face with resume, SHA256 verification, and a visible speed and ETA. These are multi-gigabyte files on connections that drop, so resumable is not optional.
4. Runs a benchmark generation and reports actual tokens per second before declaring success.

Store models in `%APPDATA%/APRO-CAD/models/`, allow a custom path for people who keep GGUFs elsewhere, and detect existing files so nobody redownloads a model they already have.

### 4.4 Routing

Do not make it either-or. Route by task:

| Task | Default route |
|---|---|
| Parameter tweak, single op edit | Local, small |
| Full component generation | Local, default tier |
| Multi-step plan | Cloud if available, local otherwise |
| Design critique, free-form discussion | Cloud |

Expose the routing table in settings. Some users will want fully offline, and that must be a single toggle that provably makes zero network calls.

---

## 5. Kernel Upgrade (Phase 4)

Not a new system, but it gates three of the most valuable verifiers, so it belongs in the plan.

Moving to Truck 0.7+ or a maintained fork unlocks, in order of value to this spec:

1. **Boolean operations** which unlock interference checking, fit and clearance verification, and true multi-body assemblies. This is the difference between "the agent produced valid geometry" and "the agent produced geometry that fits together."
2. **Shell** which unlocks wall thickness as a first-class parameter rather than something encoded manually in profiles, and removes the most common reason a user has to hand-edit profile points.
3. **Capped loft** which removes the mesh-only path for `Loft` and `FinSet`.
4. **B-Rep STEP export for Solid components**, fixing the round-trip gap.

Sequence it as: pin the current Truck version, write a kernel-capability trait with runtime feature flags, port behind the flags, run the golden corpus against both kernels, then flip the default. The existing abstraction layer is what makes this a contained change, so do not let anything in `features` or `document` reach past `kernel` in the meantime.

---

## 6. Fine-Tuning Pipeline

Do this last. It is cheap once the corpus exists and worthless before.

### 6.1 What it will and will not do

**Will:** raise schema conformance, teach your RON idiom and naming conventions, reduce token count by removing the need for long few-shot blocks, and make the small local models usable.

**Will not:** teach aerodynamics, structural sense, or design judgement. A model trained on 5k RON documents learns what a nose cone declaration looks like, not what fineness ratio suits a flight regime. Design quality comes from the Requirements block and the verifier set, not from weights. Keep these separate or a successful fine-tune will feel like a failure.

Note that with grammar-constrained decoding in place (Phase 2), the syntax portion of the fine-tune's value is already captured. What remains is idiom, parameter sensibility, and planning quality.

### 6.2 Corpus generation

You have something most fine-tuning efforts lack: a machine verifier.

```
1. Parameter sweep
   Programmatically enumerate the shorthand parameter space.
   Nose cones: 6 profiles x 8 fineness x 6 diameters = 288
   Nozzles: 2 types x 8 expansion x 5 throat x 3 percent-bell = 240
   Tanks, tubes, transitions, fin sets similarly.
   Then combine into multi-component vehicles by valid-joint rules.

2. Validate
   ron-check, then evaluate. Discard failures.
   Log the failure modes, they are free bug reports on the evaluator.

3. Back-translate
   Show a large model the RON plus its mass properties and derived
   metrics, ask for the natural-language request that would have
   produced it. Generate 3 phrasings per document at varying
   specificity, from "make a nose cone" to full spec.

4. Rejection sample
   Have a candidate model answer the back-translated request.
   Keep the pair only if the output validates AND is geometrically
   close to the original (Hausdorff distance below threshold).

5. Augment with repairs
   Take validation failures from step 2 and real user sessions,
   pair (broken RON + error message -> fixed RON).
   This is what teaches the repair loop to actually repair.
```

Target 5,000 to 15,000 pairs. Reserve 10% as a held-out eval set that is never trained on.

### 6.3 Training

LoRA, rank 16 to 32, on the 7B. Roughly $30 to $80 of rented H100 time. Use `unsloth` or `axolotl`. Merge and requantize to GGUF for distribution.

Evaluate against the held-out set on: schema validity rate, requirement satisfaction rate, geometric distance to reference, and mean repair attempts to convergence. Track all four across versions. If requirement satisfaction does not move, the fine-tune added nothing that grammar constraints were not already giving you, and you should say so rather than ship it.

---

## 7. Additional Features

Grouped by what they improve. Ordered within each group by value per unit of effort.

### 7.1 Design capability

**Parameters and expressions block.** The largest single gap between this and production parametric CAD.

```ron
Parameters(
    body_od: 98.0,
    wall: 2.0,
    body_id: "body_od - 2 * wall",
    fin_root: "body_od * 1.8",
    nose_len: "body_od * 5.0",
)
```

Evaluate with `evalexpr` or a small hand-rolled parser, build a dependency DAG, detect cycles, and recompute only downstream of a changed parameter. Then expose the parameter list as a slider panel and the whole tool becomes interactively parametric. This also gives the AI a far better editing target: changing one number beats regenerating a component, and it makes design intent explicit in the document rather than implicit in the numbers.

Do this before the agent loop if you can. The agent gets dramatically more effective when it can express relationships instead of constants.

**Patterns and arrays.** `LinearArray`, `PolarArray`, `Mirror` as ops. Polar array in particular removes most of the need for special-cased `FinSet` handling and covers bolt circles, vent holes, and stiffener rings.

**Sketch constraints, lightweight.** Full 2D constraint solving is a large project. A useful 20% version: named points, `Horizontal`, `Vertical`, `Equal`, `Tangent`, and dimensional constraints on profile point lists, solved with a small Gauss-Newton pass. This makes profiles editable without recomputing coordinates by hand.

**Barrowman CP, static margin, and full inertia tensor.** Covered in 3.3. High value, small effort, directly feeds APRO Works' other modules.

**Handoff to Rocket Mission Designer and HexaDOF.** Export mass, CG, inertia tensor, and reference geometry in a format the trajectory modules ingest directly. This is the ecosystem play. CAD that hands a validated mass and inertia model straight to 3DOF/6DOF simulation is something no standalone CAD package offers, and it makes the whole suite worth more than the parts.

### 7.2 Speed

**Adaptive tessellation.** Coarse tolerance during editing (target under 50k triangles for the whole document), fine at export. Biggest single interactivity win, low effort. Expose the two tolerances in settings.

**Content-addressed evaluation cache.** Hash each component's RON subtree plus its resolved parameters, key the mesh cache on that hash. Your status bar already shows a hit rate, so extend it to survive across sessions by persisting to disk. Startup on a large document becomes near-instant.

**Parallel component evaluation.** Components are independent until assembly. `rayon` over the component list. Near-linear speedup on multi-part vehicles, roughly a dozen lines.

**Dirty-region recompute.** With the parameter DAG from 7.1, a parameter change recomputes only affected components. Combined with the cache, most edits touch one component.

**Move tessellation off the UI thread.** If any evaluation currently blocks the WebView, fix that first, before any of the above. Perceived speed is dominated by whether the app stays responsive, not by throughput.

**Mesh transfer format.** If you are passing mesh data through Tauri IPC as JSON, switch to a binary channel with typed arrays. On a 200k-triangle model this is the difference between a stutter and nothing.

### 7.3 Interface

**Replace the RON editor with CodeMirror 6.** Syntax highlighting, bracket matching, folding, inline error markers from `ron-check`, and autocomplete driven by the generated schema from Phase 2. The schema already exists at that point, so completions come free. This is the highest-impact interface change on the list.

**Split the frontend.** The single large JS file is listed as a known weakness and it will block every other interface item here. Native ES modules with an importmap, no bundler needed, keeps the current zero-build workflow intact. Split along: viewport, editor, properties, chat, library, agent, ipc, state.

**Command palette.** Ctrl+K, fuzzy search over every action including library insertion and agent tasks. Cheap to build once the frontend is modular, and it becomes the fastest path for power users.

**Side-by-side diff for AI proposals.** Current +/- line stats are too coarse to review confidently. With patch-based edits (Section 2.5) you can show exactly which component and which field changed, and highlight the affected geometry in the viewport on hover. That last part, hovering a diff line and seeing the part glow, is the feature that makes people trust AI edits.

**Section view and clipping plane.** One-click cutaway. Essential for internal geometry and currently impossible to inspect.

**Measurement tools.** Point-to-point, edge length, angle, radius. Cheap on the mesh, constantly needed.

**Exploded assembly view.** Slider that offsets components along the vehicle axis by their stacking order. Nearly free given your 6-DOF transforms, and it is what people screenshot.

**Unit-aware numeric input.** Accept `2in`, `50.8mm`, `2 in` in any numeric field and convert to document units. Removes a whole class of error.

**Export file picker and presets.** Fixes the hardcoded `C:\Users\henry\...` path. Presets for STL binary/ASCII, tessellation tolerance, per-component versus merged. Small, and the current state is a visible rough edge in any demo.

### 7.4 Robustness and shipping

**Crash-safe autosave.** Write the document to a recovery file on every successful evaluation. Offer recovery on next launch. Cheap insurance.

**Release packaging.** Installer via `tauri-bundler`, icons, code signing, and `tauri-updater` for auto-update. Currently debug-mode only. Nothing else on this list matters if users cannot install it.

**Auto-generated AI_INSTRUCTIONS.md.** Once `JsonSchema` derives exist for Phase 2, generate the schema section of the instructions file from code at build time. Hand-written prose sits above, generated schema below, with a CI check that fails if the generated section is stale. This permanently closes weakness #8.

**AI regression harness.** A fixed set of 50 prompts with expected outcomes, run against each model and prompt change, reporting validity rate, requirement satisfaction, and mean repair count. Without this you cannot tell whether a prompt edit helped or hurt, and you will make it worse at some point without noticing.

**Profile point-order verification.** Weakness #9 says Extrude, Loft, and Sweep ordering is documented by assumption. Write the tests. An assumption in the schema reference becomes a systematic error in every AI-generated document, and it is the kind of bug that is invisible until it is expensive.

**Telemetry, opt-in.** Which ops fail, which requirements go unmet, which library entries get retrieved and then rejected. This is the data that tells you where the tool is actually weak, as opposed to where you think it is.

---

## 8. Expected Outcome

Honest assessment of the end state.

**Schema conformance and generation speed: very good.** Grammar constraints plus retrieval plus fine-tune gets first-pass validity near 100% and first drafts in seconds. This is a real, defensible differentiator that GUI-only CAD structurally cannot match.

**Design quality: competent, not creative.** A fast, tireless junior draftsman that produces correct conventional parts and never invents anything. It will nail a standard ogive nose cone and a conventional bell nozzle. It will not find a clever packaging solution or notice a thin flutter margin.

**Design validation: partial, and clearly bounded.** Dimensional, mass, and stability checks are solid. Interference, fit, and wall thickness are blocked until the kernel upgrade. The Requirements block makes this boundary visible instead of hidden, which is the correct behaviour.

**Flight hardware trust: requires human review plus external FEA and CFD.** True of every CAD package. The risk specific to this design is that a green requirements scoreboard creates false confidence, so the UI must keep "checkable requirements met" visually distinct from "validated for flight."
