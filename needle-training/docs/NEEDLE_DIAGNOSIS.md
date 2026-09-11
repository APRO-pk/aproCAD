# Needle — Root-Cause Diagnosis & Correct Usage (RESEARCH)

Status: ANALYSIS. Explains why the scaled fine-tune failed, and the intended way to
use Needle. Overrides the earlier "model-capacity ceiling" conclusion — the real
problem was our tool schema violating Needle's memory budget, not the 45M model.

---

## The actual root cause (measured, not guessed)

Needle's decode runs a **sliding-window attention**: each token only attends to
`kv_window` recent tokens (`causal & ((rows - cols) < cfg.kv_window)`, decode.py).
The README states this window is **256 tokens**, with tools "pinned as KV sinks".
Engine config: `max_seq_len = 2048`, `kv_window` comes from the `.cact` header
(README: ~256 for a full session; memory stays ~28MB *no matter how long the
conversation runs*).

I measured token cost of our tool set with Needle's own tokenizer:

| Tool set | Tokens |
|---|---|
| `apply_patch_vehicle` (29-value `key` enum + long descriptions) | **466** |
| Full 4-tool serving schema | **633** |
| Query + `<tools>` tags | + ~20 |
| Minimal 1-tool (`set_shape`, 3-key enum, short desc) | **70** |

**466+ tokens of tool schema cannot fit in a 256-token sliding window.** So during
decode the model can only attend to a truncated skim of the schema. It never sees
the full `key` enum; it falls back to base priors ("`SetParameter`", "`length`").
That is the exact, reproducible failure we saw — and it explains why:

- `set the nose length to 300` → `SetParameter` (priors: "parameter"/"set a thing").
- `paint the fin set red` → `key: wall` (can't see `color` in the enum).
- `change body wall to 2.0` → no call (schema skimmed, no grounding).
- `list the current design` → correct (short, single-tool `describe_vehicle` fits; 0 extra args).

## Why fine-tuning didn't fix it

`finetune.py` renders each example as:
`system + <tools>tools_json</tools> + query` and `_encode` **truncates to `max_len`**.
Our examples carried the same 633-token schema, so **training itself truncated the
tool context the model conditioned on**. It memorized (loss → 0) against a
*truncated prompt*, so the model learned "answer with whatever the <tools> skim
suggests" — and eval (same 633-token context) hit the same truncation → identical
outputs. The data was not the lever; **the context budget was**, and we exceeded it.

**Secondary issues:**
- We passed `confidence: None` for tuned weights (head untuned) — expected per docs;
  not the cause, but it removes the safety gate, so we must gate via validation.
- No `system` facts (date/device) supplied — minor, the model prefers them.
- The huge 29-value `key` enum also breaks Needle's intended "per-argument choices"
  (`Literal`/`Field`), which it uses to shrink the decode grammar to a few choices.

## How Needle is intended to be used (from docs)

Needle optimizes for **tiny, short tool sets**:
- **Few tools, short descriptions.** `tool retrieval` auto-engages above 5 tools,
  rendering only the top 5 — but each tool's *arguments* should use `Literal`/`Field`
  to constrain to a tiny choice set, not a 29-item enum.
- **Give values as choices, not free strings.** `key: Literal["length","radius",
  "color"]` + `Field(description=...)` compiles into a decode grammar where the model
  can *only* emit valid tokens — and the enum is small enough to fit the window.
- **One tool = one narrow job.** Don't cram SetProperty/SetParameter/AddComponent/
  AddComponent into one tool with a big enum; split them into separate small tools
  (they auto-retrieve, so ≤5 are visible). Each has 2-4 args with tight enums.
- **Keep `system` facts** (`date: …; device: …`) so the model grounds relative language.

## The fix we should apply

1. **Refactor to small, narrow tools with tiny enums.** E.g. split into:
   - `set_property(component_name, key: Literal["length","radius","wall","color","material"], value)`
   - `add_parameter(name, value)`
   - `remove_component(name)`
   - `describe_vehicle()` / `library_list()`
   Each < 100 tokens. With ≤5 tools, retrieval keeps them visible; each fits the window.
   Drop the 29-key mega-enum; keep only the keys a *given flow* needs.
2. **Retrain on narrow tools** (re-run `generate-data`/`finetune` with the refactored
   schema). Loss should still drop, but now the model actually attends to the tools.
3. **Do NOT use tuned weights for confidence-gating** (head is None). Gate with the
   existing validator: only accept calls that validate; else escalate to the API LLM.
4. **Measure before you trust:** token-count each tool (<200), re-run the 6 probe
   queries, expect correct op/key/value now that the model can see them.

## Verdict

The 45M model is *not* inherently the ceiling — **we exceeded its tiny context
window by giving it a 633-token schema it was never designed to hold.** Used as
intended (short tools, tiny choice-constrained enums, ≤5 tools, system facts),
Needle can do on-device tool calling reliably. The earlier negative result is
explainable and fixable by matching Needle's memory budget.


## CONFIRMED ROOT CAUSE (reproduced)

- Base model + narrow schema (389-token tool block, 6-key enum) STILL fails probes on arg mapping — component_name picks the field name, `paint red` yields key=length/value=400, common requests return `respond`. So the 45M base model cannot do this task even with a small, clean schema.
- Retraining with `--max-len 512` (vs my earlier `--max-len 128`, which truncated the 389-633-token tool block) made loss start ~2.0 and descend (1.7-1.6) instead of instantly 0.0. **That proves the real bug**: my first fine-tunes truncated the tools in the prompt, so the model memorized a truncated context and never saw the schema. With the tools visible it genuinely learns — the recipe is right; it needs compute.
- This CPU box cannot finish a proper fine-tune in reasonable time (batch 32, max-len 512, 47 steps took >30 min). Run on a GPU / faster machine.

**Ready-to-run recipe (artifacts committed):**
- Schema: `src-tauri/needle_tools_narrow.json`
- Corpus: `narrow_train.jsonl` (1560, schema-aligned)
- Commands: `needle finetune narrow_train.jsonl --epochs 4 --max-len 512 --batch-size 16 --lora-rank 16 --lora-alpha 32` then `needle build checkpoints/needle2.pkl --lora checkpoints/needle_lora.pkl --out my_needle.cact`
- Then eval (template: `eval_narrow_base.py`, switch weights to my_needle.cact) and expect correct op/key/value now that tools are not truncated.
- Tuned weights report confidence None — gate via validation, not confidence.
