# Needle Training Bundle — APRO CAD

Self-contained kit to fine-tune the **Needle 2** on-device tool-calling model on
our CAD tool vocabulary, so the app can run small edits locally/offline while the
API LLM handles planning and hard cases.

Copy this whole `needle-training/` folder to any machine and follow the steps below.
Nothing here contains secrets — you supply your own API key via an env var.

---

## 0. TL;DR (the fast path)

```bash
# 1. environment (use a venv)
python -m venv .venv && . .venv/Scripts/activate      # Windows
pip install -r requirements.txt

# 2. train (GPU box) — 4 epochs on the prepared narrow corpus
python scripts/finetune.py --data data/narrow_train.jsonl --epochs 4 --batch-size 32

# 3. export the model
python scripts/build_cact.py --out my_needle.cact

# 4. verify
python scripts/evaluate.py --weights my_needle.cact
#   expect: "8/8 probes correct"
```

If step 4 prints fewer than 8/8, read **Troubleshooting** — almost always the
`--max-len` (tools truncated) or too few epochs.

---

## 1. What Needle is and why the details matter

- **Needle 2** is a 45M-parameter, ~14 MB, CPU-capable model for **tool calling**:
  text in → one JSON tool call out, constrained by a byte-level grammar compiled
  from your tool schemas. Apache-2.0.
- It runs with a **~256-token sliding attention window** with the tools "pinned as
  KV sinks". **This is the single most important constraint** (see §6).
- It reports a `confidence` score — but **fine-tuned weights report `confidence:
  None`** (the confidence head is not trained). A tuned model must therefore be
  trusted via **validation of the emitted call**, not confidence.
- Intended usage: **a few tiny tools**, each with a small, choice-constrained
  argument set — not one giant tool with a 29-value enum.

---

## 2. Prerequisites

- Python 3.10–3.14 (3.14 tested).
- ~4 GB disk for the base checkpoint + engine; the base auto-downloads on first use.
- For GPU training (NVIDIA): a CUDA-capable driver.

### CPU-only install
```bash
pip install -r requirements.txt      # installs cactus-needle (pulls JAX CPU)
```
CPU training works but is **slow** (a 1-epoch run over ~1.5k examples can take
30–60+ min and may OOM on low-RAM machines). Use a GPU box for real runs.

### GPU setup (NVIDIA / CUDA) — recommended
```bash
pip install "cactus-needle[gpu]"
pip install "jax[cuda12]"            # CUDA build of JAX
# verify JAX sees the GPU:
python -c "import jax; print(jax.devices())"
#   -> [CudaDevice(id=0)]  (or GpuDevice)
```
If `jax.devices()` shows only CPU, JAX/CUDA isn't wired up yet; fix that first —
`needle finetune` uses JAX under the hood.

---

## 3. Layout

```
needle-training/
├─ README.md                 <- this file
├─ requirements.txt
├─ docs/
│  ├─ NEEDLE_MODE.md         <- architecture: two-tier design, integration, roadmap
│  └─ NEEDLE_DIAGNOSIS.md    <- the measured root-cause analysis (read this!)
├─ schemas/
│  ├─ needle_tools_narrow.json  <- RECOMMENDED serving schema (5 small tools, ~389 tokens)
│  └─ needle_tools.json         <- legacy big schema (do NOT train/serve this; exceeds the window)
├─ data/
│  ├─ narrow_train.jsonl     <- merged training corpus (1560 examples) — USE THIS
│  ├─ seed_narrow.jsonl      <- hand-authored seed (60) for the narrow tools
│  ├─ narrow_generated.jsonl <- 1500 API-generated examples (narrow schema)
│  ├─ needle_train.jsonl     <- early seed corpus (legacy big schema)
│  └─ needle_train_v2.jsonl  <- early merged corpus (legacy big schema)
├─ scripts/
│  ├─ generate_data.py       <- (re)generate data via OpenAI/OpenRouter
│  ├─ finetune.py            <- train LoRA (GPU-aware)
│  ├─ build_cact.py          <- merge adapter + export .cact
│  ├─ evaluate.py            <- probe evaluation
│  ├─ gen_seed_narrow.py     <- regenerate the narrow seed corpus
│  └─ gen_needle_train.py    <- regenerate the legacy seed corpus
└─ integration/
   ├─ needle_worker.py       <- JSON-lines sidecar used by the desktop app
   └─ needle.rs              <- the Rust Tauri command that drives the sidecar
```

---

## 4. The tool schemas (what the model can call)

`schemas/needle_tools_narrow.json` is the one to use. Design rules (learned the hard way):

- **≤ 5 tools.** Above 5, Needle's built-in retrieval shows only the top-5 per turn;
  an unselected tool is unreachable.
- **Each tool < ~100 tokens.** Keep descriptions short.
- **Constrain arguments** with small `enum`s (e.g. `key: ["length","radius","wall","color","material","visible"]`).
  The grammar then only allows valid values, which is exactly where a tiny model wins.
- **One tool = one narrow job.** Split operations into separate tools rather than
  one tool with an `operation` enum + giant `key` enum.

The narrow set:
| Tool | Args |
|---|---|
| `set_property` | `component_name`, `key` (6 choices), `value` |
| `set_throat` | `component_name`, `value` |
| `set_circle` | `component_name`, `count`, `pitch_diameter`, `hole_diameter`, `depth` |
| `add_parameter` | `name`, `value` |
| `describe_vehicle` | (none) |

> ⚠️ `schemas/needle_tools.json` (the old 4-tool / 633-token schema) is kept only
> for reference. Its `apply_patch_vehicle` alone is ~466 tokens and **cannot fit
> Needle's window** — training or serving it produces garbage. That was the original bug.

---

## 5. Data format

One JSON object per line (`data/narrow_train.jsonl`):
```json
{"query":"paint the nose red","tools":[…full schema array…],"answers":[{"name":"set_property","arguments":{"component_name":"Nose","key":"color","value":"red"}}],"reasoning":"'red' -> color"}
```
- `tools`: the exact schema array you will serve (must match `schemas/needle_tools_narrow.json`).
- `answers`: the expected call(s); an off-topic example uses `"answers": []`.
- `reasoning`: optional short derivation.

**Regenerate / expand data** (needs an OpenAI-compatible key):
```bash
# PowerShell
$env:OPENROUTER_API_KEY="sk-..."          # works for OpenAI too
python scripts/generate_data.py --tools schemas/needle_tools_narrow.json \
    --num-samples 1500 --model gpt-4o-mini --out data/narrow_generated.jsonl
```
`OPENROUTER_URL` defaults to OpenAI (`https://api.openai.com/v1/chat/completions`).

To add hand-authored examples, edit `scripts/gen_seed_narrow.py` and run it, then
concatenate `seed_narrow.jsonl` + generated data into a new `*_train.jsonl`.

---

## 6. THE CRITICAL SETTING: `--max-len` (do not skip)

`needle finetune` builds each training prompt as
`system + <tools>{schema}</tools> + query` and then **truncates to `--max-len`**.
If `--max-len` is smaller than the tool schema block, the model **never sees the
tools** — it memorizes a truncated prompt, loss collapses to `0.0000` instantly,
and evaluation is nonsense.

- Narrow schema ≈ **389 tokens** → use **`--max-len 512`** (or higher).
- Rule of thumb: `max-len ≥ tokens(tools) + tokens(query+answer) + slack`.
- **Red flag:** if the first reported loss is `0.0000`, you truncated the tools.
  A healthy run starts around **~2.0** and descends.

Measure your schema's token cost any time:
```bash
python -c "import json;from needle.model.tokenizer import get_tokenizer as g;t=g();d=json.load(open('schemas/needle_tools_narrow.json'));print('tokens:',len(t.encode(json.dumps(d,separators=(',',':')))))"
```

---

## 7. Step-by-step training

```bash
# (optional) fresh venv + deps
python -m venv .venv && . .venv/Scripts/activate
pip install -r requirements.txt

# 1) TRAIN  (GPU: batch 32-64; CPU: batch 8-16, expect slow)
python scripts/finetune.py --data data/narrow_train.jsonl \
    --epochs 4 --max-len 512 --batch-size 32

#    watch the log: loss should start ~2.0 and fall. The adapter lands at
#    checkpoints/needle_lora.pkl. The base checkpoint auto-downloads on first run.

# 2) EXPORT
python scripts/build_cact.py --out my_needle.cact
#    -> my_needle.cact (~14 MB). To push quality/size, add --bits 2.

# 3) VERIFY
python scripts/evaluate.py --weights my_needle.cact
#    -> target: 8/8 probes correct.
```

Expected GPU time: a few minutes for 1560 examples × 4 epochs. CPU: tens of minutes+.

---

## 8. Troubleshooting

| Symptom | Cause / fix |
|---|---|
| First loss is `0.0000`; eval garbage | **Tools truncated.** Raise `--max-len` (≥512 for narrow). See §6. |
| `RESOURCE_EXHAUSTED / Out of memory` | Lower `--batch-size` (8) and/or `--max-len` (but not below the tool block). Prefer GPU. |
| `set OPENROUTER_API_KEY to generate data` | Export the key before `generate_data.py`. |
| Every call returns `respond` / no call | Too few epochs or truncated tools; also ensure `tools` in the data exactly equals the serving schema. |
| `confidence: None` on tuned model | Expected. Gate the tuned model by **validation**, not confidence. |
| GPU not used | `pip install "jax[cuda12]"`; check `python -c "import jax; print(jax.devices())"`. |
| `needle` CLI not found | `pip install cactus-needle`; on Windows it may be at `%APPDATA%\Python\Python3xx\Scripts\needle.exe` — set `NEEDLE_EXE` to it. |

---

## 9. Integrating the trained model into the app

The desktop app has the full two-tier plumbing (built and committed):

- `integration/needle_worker.py` — a warm sidecar that loads a `Needle` agent and
  answers `{id, query, tools, weights, reset}` requests over stdin/stdout (one JSON
  per line). **It resets the engine before every turn** (see §11 — required).
- `integration/needle.rs` / `src-tauri/src/needle.rs` — the Tauri command
  (`needle_run`) that spawns/keeps the worker and performs one `complete()` turn.
- `src-tauri/src/lib.rs` — `get_needle_schema` (serves the embedded narrow schema)
  and `pick_file_dialog` (native `.cact` picker).
- `src/main.js` — **Needle mode**: `callNeedle()` → `needleCallToPatch()` (maps a
  Needle tool call onto a RON `Patch`, resolving the model's canonical
  `Nose/Body/Nozzle/Fins` onto the real component names by kind) → the existing
  validation gate. Any failure (no call, ungrounded/negated call, unknown
  component, unsupported tool, patch rejected) **escalates that todo to the API**.

To use a trained model in the app:
1. **Settings → Generation → Needle**, then **Browse…** to the `.cact` file.
2. Optionally keep **"AI breaks work into Needle-sized micro-steps"** on (default).
3. Send a request as usual: the API LLM still plans; each simple todo runs locally
   first (~300 ms) and hard ones go to the API.

### Micro-step mode (the important optimisation)

With the checkbox on, the planner is given a different contract
(`NEEDLE_PLAN_CONTRACT` in `src/main.js`): it emits **one atomic change per todo**,
tagged with the executor that should handle it.

- `needle: set the Nose length to 450` — phrased in the exact tiny vocabulary the
  local model was trained on; executed on-device.
- `api: add a FinSet named Fins-2 with 4 fins, ...` — anything outside that
  vocabulary; goes straight to the cloud, never wasting a local round trip.

Measured effect on the same probe set: generic todo phrasing (`"Increase Nose
length to 300.0"`) resolved **6/7** locally; micro-step phrasing resolved **7/7**,
because each step is a single property and matches a trained command shape. The
planner's routing also means unsupported work skips the local model entirely.

### Fast mode — batching the API calls (rate-limit fix)

The default agent loop made **~1 plan + 1 API call per todo + 3 review + 1
optimize** calls, which trips provider rate limits almost immediately. The
**"Fast mode"** checkbox (default on, `needleFastEnabled()` in `src/main.js`)
replaces that with a single session (`runNeedleSession`):

1. **One plan call** → micro-steps tagged `needle:`/`api:`.
2. The local model executes every `needle:` step (no API calls at all).
3. **One batched patch call** for everything left (`api:` steps + local misses) —
   the model emits a single `PatchList` applied atomically. Only if that is
   rejected does it fall back to the reliable per-step loop.
4. **One combined review call**, and only when validation actually reports issues
   (the 3-round review + optimize passes are folded into this).

| | API calls per request |
|---|---|
| In-Line / non-fast | 1 + N + up to 4 |
| Fast Needle mode | **1–3 total** |

Rate-limit handling in `callAi` was also hardened: it now honours the
`Retry-After` header and the body's `try again in Ns/ms`, falls back to
exponential backoff with jitter, and allows up to 8 attempts.

> The **runtime to adopt later** is the native **Cactus** engine (`bindings/rust`,
> C API with built-in tool calling + confidence + cloud handoff). It removes the
> Python/JAX dependency entirely. See `docs/NEEDLE_MODE.md` for that roadmap.

---

## 10. Status / what was already established (this machine)

- Pipeline verified end-to-end: install → generate-data (OpenAI) → finetune → build
  → load `.cact` in-process, offline.
- Root cause of the earlier failures **measured and documented** in
  `docs/NEEDLE_DIAGNOSIS.md`: our old schema (633 tokens) exceeded Needle's window
  and `--max-len 128` truncated the tools during training.
- Base model + narrow schema still fails the probes → **a real fine-tune is required**
  (which is what this bundle is for). Training was started here and loss was
  descending correctly once `--max-len 512` was used, but this CPU box was too slow
  to finish — hence this bundle for the GPU PC.

**Next action on the GPU PC:** §7, then report `python scripts/evaluate.py --weights my_needle.cact`.

---

## 11. RESULT: the CUDA-trained model (measured)

The GPU-trained adapter (`needle-training-new/…/checkpoints/needle_lora.pkl`,
rank 32, alpha 2.0) **works**. Findings, all measured on this machine:

- **Training succeeded.** Loading base + adapter in **float32** and decoding
  **greedy** (Python reference path) reproduces the training behaviour: **6/6** on
  the core probes, with correct capitalised component names.
- **The `.cact` engine carries decode state between `complete()` calls.** Serving
  several queries through one warm agent without resetting bleeds a stale KV cache
  into every later query — measured **1/8** correct. Calling `agent.reset()` (the
  worker does this by default now) restores **6/8**. Results are then perfectly
  reproducible run-to-run (the engine *is* greedy).
- **Serve the exact `needle_tools_narrow.json` block.** The model memorised the
  precise tool JSON; a terser re-description of the same 5 tools collapsed it to
  **1/8**. Do not "improve" the schema text at serving time.
- **Ceiling here is 6/8**, and the two misses (`"Nose"`→`"NS"`, and
  `hide the nozzle` → wrong tool) are **introduced by the 4-bit export** — float32
  gets both right. Only `--bits 2|4` exports exist, so this is the precision limit.
  Treat the local model as a **fast draft executor** and gate every call by
  validation, escalating to the API LLM on reject (the two-tier design in
  `docs/NEEDLE_MODE.md`).

Reproduce:
```bash
python scripts/evaluate.py --weights my_needle.cact   # add --reset (now default)
```

Warm latency measured through the sidecar: **~250–400 ms/turn** (CPU).
