# Needle Mode — Architecture Analysis (Kernel-v2-era)

Status: ANALYSIS ONLY — no code yet. This documents why/how "Needle Mode" (on-device
tool-calling) could slot into APRO CAD, the hard constraints, and a phased plan.
Decision gate at the end.

---

## 1. What Needle 2 actually is (verified from source)

| Property | Value |
|---|---|
| Params | 45M (Simple Attention Network: Hadamard-MLP FFN, GQA, engram KV memory) |
| Footprint | ONE ~14MB `.cact` binary (weights baked); ~28MB RAM per session |
| Runtime | Python package `cactus-needle` wrapping a compiled engine (`libneedle.so` / `needle.dll`; `win_amd64`, `win_arm64`, `macosx_11_0_arm64`, `manylinux2014_x86_64/aarch64`, `musllinux…`) |
| Offline | Yes — engine fetched once & cached (`~/.cache/cactus-needle/…`), inference does NO network. Air-gapped supported (feats happen in doc/apis.md) |
| License | Apache-2.0 |
| I/O contract | `agent.complete(query) -> {type:"call", function_calls:[{name,arguments}], reasoning, confidence, …}`; empty `function_calls:[]` = off-topic refusal; no free-text fallback |
| Confidence | Learned head + token prob, min-of-two; act ≥ threshold, escalate below |
| Tool retrieval | >5 tools → contrastive embed of schemas; top-5 per turn enter context; unselected tools are UNREACHABLE in that turn |
| Memory | 256-token sliding window, tools pinned as KV sinks (~28MB stable) |
| Grammar | Byte-level grammar compiled from your JSON schemas constrains every emitted token — malformed calls impossible |
| Structured data | Extraction == tool-calling with one tool |
| Fine-tune | LoRA + merge → still a single `.cact`; JAX, OpenRouter data synth |

Crucially: **Needle is a Python package + a native engine binary. There is no
official Rust/WASM/C runner published in the repo.** The engine is weight-agnostic
and there's a CLI runner (`needle download <platform>`), but the only *public*
way to drive it today is the Python API (`import needle`).

---

## 2. The design we want

> "Every function we have worked on is simply a function with this model."

Two-tier generation:

- **API LLMs** (existing `callAi`, OpenRouter/OpenAI/local): read human text →
  produce a *set of detailed, sequential instructions* (a task script) for Needle
  to interpret and execute.
- **Needle (in-process)**: cheap, deterministic, offline — performs the
  *easy, mechanical, tool-shaped* steps (move 2mm, set wall=2.0, add BoltCircle,
  patch a color, boolean a hole, list library parts). Large/complex redesigns
  stay on the API LLM (planner).

That gives: instant local response for small edits, near-zero marginal cost,
offline/private designs — while the big model keeps the "shape the whole thing"
thinking we can't do locally.

---

## 3. Our function surface = Needle tools

We already have a clean RPC boundary (Tauri commands). These map 1:1 to Needle tools:

- `evaluate`, `evaluate_vehicle` (read-only geometry check)
- `describe_vehicle`, `describe_parameters`, `validate` (inspect)
- `apply_patch_vehicle`, `apply_patch_ron` (the ONLY real mutators) — needs
  `with_patch: Patch` arg; the Patch schema is already JSON-schema'd for the API
  path, so Needle's grammar gets it for free
- `modify_property` (mutate a component property)
- `library_list`, `library_save_ron`, `library_retrieve`, `library_delete` (library)
- `check_interferences` (feedback signal)
- `get_ai_schema` (introspection)

Needle's "describe tools well" requirement maps to AI_INSTRUCTIONS.md + our GBNF/JSON
schemas. Because Needle emits grammar-constrained JSON, `apply_patch_ron`'s
PatchList becomes a first-class "action" it can call repeatedly.

**Critical safety gate:** mutators are the dangerous set. Needle mode should
expose a *constrained* toolset by default (inspect + patch), gated by the same
validation we already run, and by its own confidence threshold.

---

## 4. Feasibility — the honest constraints

### 4.1 The Python dependency is the crux
Embedding a Python model "inside our software" (Tauri v2, Rust) has three honest
options, in order of fit:

1. **Bundle `cactus-needle` via a bundled Python runtime.** Ship CPython + the
   wheel + the cached engine as app resources; Rust spawns a small helper
   (`#[tauri::command] needle_*`) that talks to a long-lived `python` subprocess
   over stdin/stdout JSON. The subprocess holds the model warm (~28MB). **Most
   reliable**, matches the upstream API exactly, keeps the whole `agent.run` /
   `complete()` / confidence / retrieval contract intact. Cost: bundle a Python
   runtime (~30–60MB) + wheel + engine (~14MB + platform lib); Windows
   `win_amd64` runner exists. Startup spawn cost, JSON IPC each call.

2. **Use the standalone engine runner** (`needle download win-amd64`) — a native
   runner the CLI can drop per-platform. If it exposes a subprocess/C-ABI, we
   could avoid bundling Python entirely and call the engine directly from Rust.
   **Needs verification** — the repo documents the runner for *download*, but
   not yet a documented Rust/C API surface; assume it needs a small FFI or
   subprocess shim we write.

3. **WASM / pure-Rust port** — does not exist upstream. Out of scope.

**Recommendation:** start with option 1 (bundled Python + JSON subprocess). It
gets us to a working end-to-end Needle Mode fastest and is robust. A later task
can investigate the native runner (option 2) to drop Python.

### 4.2 Latency & capacity
- ~45M CPU params, 256-token window, 4300 tok/s prefill / 850 tok/s decode
  (from doc). A one-call turn ≈ tens of ms. Within a desktop app this feels
  instant for small edits.
- Confidence-gated: below threshold → route back to API LLM. This is the
  escalation spine of the two-tier system, exactly as the user described.

### 4.3 Tool schema → grammar is already ours
Patch/Vehicle/Component JSON schemas already exist (`get_ai_schema`, GBNF). We
feed those schemas to Needle's `tools=` (JSON schema dicts). Byte-level grammar
then guarantees well-formed patches — higher reliability than the API path.

### 4.4 Memory & concurrency
- ~28MB per Needle process. One process, warm, serialized calls (or a small
  pool). Fine on desktop.

---

## 5. Architecture sketch

```
[chat prompt]
      │
      ▼
 [Router "Needle Mode" ON?]
      │
      ├─ no ──► existing API-LLM agent loop (unchanged)
      │
      └─ yes
           │
           ▼
      ┌───────────────────────────────────────────────┐
      │ 1. PLANNER (API LLM, cheap temperature)        │
      │    reads human text → task script: a sequence  │
      │    of tool intents, e.g.                        │
      │      [ {tool:"apply_patch_vehicle",             │
      │         with:{patch:{SetProperty:{...}}}}, … ]  │
      │    Only for intents too hard → it may ALSO      │
      │    emit {mode:"escalate", reason}               │
      └───────────────────────────────────────────────┘
                 │ task script (JSON)
                 ▼
      ┌───────────────────────────────────────────────┐
      │ 2. CONDUCTOR (Rust, owns state + validation)   │
      │    for each step:                              │
      │      build Needle tool argument                │
      │      call needle.complete(step) → {call,...}   │
      │      confidence ≥ THRESHOLD?  yes ─► execute   │
      │                               no  ─► escalate  │
      │      feed result → next complete()             │
      │    = the agentic loop the API LLM used to run  │
      └───────────────────────────────────────────────┘
                 │ executed tool results
                 ▼
      [document updated; re-evaluate; show done]

Escalation: any step below threshold, or a planner
{escalate} intent, falls through to the FULL existing
API agent loop with the accumulated context.
```

Key insight: **Needle doesn't replace the agent; it becomes the cheap executor
for a script the API planner writes.** The expensive decisions (what to change)
stay with the big model; the mechanical "call the tool with these exact args"
runs locally and deterministically. This is the user's stated intent.

---

## 6. What makes this genuinely valuable for us

1. **Instant, free small edits.** "Add a bolt circle", "wall to 2.0", "move the
   nozzle 5mm up" no longer need an API round trip or rate-limit cooldown.
   A local 45M model can pick and fill these calls reliably (grammar-constrained).
2. **Offline / private.** Design drafts never leave the machine for simple ops.
3. **Determinism + safety.** Byte-grammar means no malformed patches; confidence
   gate escalates instead of guessing wrong; validation still runs before write.
4. **Cost control.** Free local calls for the long tail; API only for
   planning/escalation.
5. **Hardening the existing agent.** Even in In-Line mode, Needle is a useful
   *verifier/extractor*: extract structured intent, or pre-fill tool args.
6. **Reuses everything we built.** Patches, schemas, validation, describe_*,
   library, interference — they're already "documented functions"; Needle is
   purely a new front-end that calls them.

---

## 7. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Bundling Python on Windows desktop | Minimal `embed`/windows runtime; test `win_amd64` engine + wheel in CI; fall back to system python (isolated venv) |
| Subprocess JSON IPC overhead | Batch: one `complete()` per step; keep process warm; ~ms overhead dwarfs model decode |
| Needle chooses wrong/wrong-args call | Grammar constrains JSON shape; confidence threshold; ONLY inspect+patch exposed by default; destructive tools (library_delete) behind confirm |
| Escalation loops / No-response (`[]`) | `[]` → route to API LLM planner; cap steps; surface "Needle declined" to user |
| Confidence `None` after fine-tune | Ship base model; fine-tuning only if we release our own `.cact` (then assert confidence or always escalate) |
| Memory/footprint on low-end | ~28MB model + warm interpreter ≈ ok for desktop; one process |
| Upstream is early (2.x, new) | Pin version; vendored wheel + engine; re-evaluate before release |

---

## 8. Phased plan

**Phase 0 (this analysis + a spike decision)** — choose bundling strategy.
Spike: bundle CPython + `cactus-needle` + `win_amd64` engine in a dev build;
prove `needle.complete()` round-trips over a subprocess JSON pipe in < 200ms.
Gate: if the win_amd64 runner/wheel installs cleanly & offline, proceed to Phase 1.

### Phase 0 SPIKE RESULTS (verified in dev env, Python 3.14)

**✅ Transport & bundling: PROVEN VIABLE.**
- `pip install cactus-needle` → `2.0.10` on py3.14 (pulls JAX/flax) — installed clean.
- Engine fetched + runs **in-process, offline**; no network during inference.
- Rust↔Python JSON-lines pipe works: wir ed a `needle_run` Tauri command + a
  warm `needle_worker.py` sidecar (`src-tauri/needle.rs`, `src-tauri/needle_worker.py`).
- Warm turn latency measured over the subprocess: **~220–420 ms/turn** (peak RAM
  ~114 MB in JAX/desktop — heavier than the doc's 28 MB claim, still trivial for a
  desktop CAD app).

**❌ Base model reliability on OUR toolset: NOT adequate without fine-tuning.**
Tested the real Patch schema as the tool:
- "set the nose length to 300" → correct call, but **confidence 0.019** (below any
  usable threshold).
- "change body wall to 2.0" → **no call** (`respond`).
- "paint the fin set red" → **wrong grounding** (returned `Nose.length="red"`),
  because the description didn't declare `color` as a valid key.
Root cause: the **base 45M model is not calibrated/tuned for our CAD-specific
vocabulary**, and confidence is only calibrated for the base — so gating on
confidence would escalate nearly everything. Fixes (both in scope):
1. **Author richer tool schemas** — `enum` of valid keys, per-arg descriptions,
   `Field` constraints. (Improves grounding substantially; raises but won't
   fix confidence.)
2. **LoRA fine-tune Needle 2 on our Patch corpus** (`needle finetune` + merge →
   `.cact`) — the repo explicitly supports this and it's the real lever for
   "hours of [confidence]-gated reliable calls on our tools." Note the doc flag:
   **tuned weights report confidence `None`** (head not updated), so a tuned model
   needs a different gate (e.g., always-escalate-on-uncertainty or a fixed trust).

**Conclusion:** the architecture is sound and the bundling/runtime path is green;
to make Needle Mode genuinely handle our functions we must **fine-tune the model
on our tool vocabulary** (Phase 1's true crux), or ship a *narrow* toolset where
its low-confidence-but-grammar-correct output is safe and everything else escalates
to the API planner.

**Phase 1 — tool adapter.** Map a SAFE subset of our functions to Needle tools:
`describe_vehicle`, `describe_parameters`, `validate`, `apply_patch_vehicle`,
`apply_patch_ron`, `modify_property`, `library_list`, `check_interferences`.
Rust command `needle_run(script_json, step)` that drives `complete()` and
executes accepted calls. Add `Settings: AI generation → In-Line | Needle`
(Needle requires the bundled runtime present; if absent, show "Not installed").

**Phase 2 — two-tier controller.** Router + planner prompt: API LLM emits the
task script (or `escalate`). Conductor executes via Needle with confidence
gating; escalations fall back to the existing agent loop. Wire into the SAME
`runAgentSession` entry so chat UX is unchanged.

**Phase 3 — breadth + tuning.** Expose library_save_ron/retrieve/delete behind
explicit confirmation, add tool-description tuning to AI_INSTRUCTIONS-fed
schemas, and evaluate a Needle fine-tune on our Patch corpus (LoRA → `.cact`)
for higher confidence on our exact toolset.

---

## 9. Decision gate / recommendation

- **Verdict:** Worth building — the architecture is a clean, high-leverage
  two-tier split that reuses our entire existing surface, and Needle's size +
  offline + confidence-gating make it a strong fit for a desktop CAD app.
- **Do it only if Phase 0 spike confirms** the bundled-Python/engine path on
  Windows is workable offline. That is the single hard dependency. If it fails,
  the whole approach degrades to "bundle a big Python env" which is heavier
  than the value it returns.
- **Recommendation:** run the Phase 0 spike first (bundled python + engine),
  then Phase 1 (tool adapter + Settings toggle) and Phase 2 (two-tier). Stop
  after each phase to validate UX before continuing.

Open questions to answer in the spike:
- Does `pip install cactus-needle` + engine fetch work on a clean Windows box
  and offline (our CI/dev machine)?
- Does the engine need VC++ redist / specific CPU features? (AVX2? it's CPU
  quantized 2-bit — likely fine, verify instruction-set requirements.)
- How long does warm startup take, and can we keep it resident?
