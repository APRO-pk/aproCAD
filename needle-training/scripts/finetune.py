#!/usr/bin/env python3
"""Fine-tune Needle (LoRA) on our tool-calling corpus, GPU-aware.

CRITICAL: --max-len MUST be large enough to hold the FULL tool schema block plus
the query and answer. If it is too small the tools get truncated in the prompt
and the model "learns" without ever seeing the schemas (loss collapses to 0.0
and eval is garbage). For the narrow schema (~389 tokens) use >= 512.

Usage:
    python scripts/finetune.py --data data/narrow_train.jsonl --epochs 4

Environment:
    NEEDLE_EXE   optional override for the `needle` CLI path
"""
import argparse
import os
import shutil
import subprocess
import sys


def needle_exe() -> str:
    if os.environ.get("NEEDLE_EXE"):
        return os.environ["NEEDLE_EXE"]
    exe = shutil.which("needle")
    if exe:
        return exe
    cand = os.path.join(os.environ.get("APPDATA", ""), "Python", "Python314", "Scripts", "needle.exe")
    if os.path.exists(cand):
        return cand
    print("error: `needle` CLI not found. Install with: pip install cactus-needle", file=sys.stderr)
    sys.exit(2)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", default="data/narrow_train.jsonl")
    ap.add_argument("--epochs", type=int, default=4)
    # 512 fits the narrow schema + query + answer without truncation.
    ap.add_argument("--max-len", type=int, default=512)
    ap.add_argument("--batch-size", type=int, default=16, help="GPU: 32-64 is fine; CPU: 8-16")
    ap.add_argument("--lora-rank", type=int, default=16)
    ap.add_argument("--lora-alpha", type=int, default=32)
    ap.add_argument("--val-split", type=float, default=0.05)
    ap.add_argument("--out", default=None, help="Adapter path (default checkpoints/needle_lora.pkl)")
    args = ap.parse_args()

    cmd = [needle_exe(), "finetune", args.data,
           "--epochs", str(args.epochs),
           "--max-len", str(args.max_len),
           "--batch-size", str(args.batch_size),
           "--lora-rank", str(args.lora_rank),
           "--lora-alpha", str(args.lora_alpha),
           "--val-split", str(args.val_split)]
    if args.out:
        cmd += ["--out", args.out]
    print("running:", " ".join(cmd))
    print("note: watch the loss — it should DESCEND from ~2 toward <0.3 over epochs.")
    print("      If it is 0.0000 immediately, your --max-len is truncating the tools.")
    return subprocess.call(cmd)


if __name__ == "__main__":
    raise SystemExit(main())
