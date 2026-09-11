#!/usr/bin/env python3
"""Generate tool-calling training data with an OpenAI-compatible API.

The `needle generate-data` CLI is OpenRouter-flavoured by default. This wrapper
points it at any OpenAI-compatible endpoint (OpenAI, OpenRouter, a local
gateway) via env vars and forwards the important flags.

Usage:
    python scripts/generate_data.py --tools schemas/needle_tools_narrow.json \
        --num-samples 1500 --out data/narrow_generated.jsonl --model gpt-4o-mini

Environment (never hard-code the key):
    OPENROUTER_API_KEY   required — the API key (works for OpenAI too)
    OPENROUTER_URL       default https://openrouter.ai/api/v1/chat/completions
                         OpenAI:  https://api.openai.com/v1/chat/completions
"""
import argparse
import os
import shutil
import subprocess
import sys


def needle_exe() -> str:
    exe = shutil.which("needle")
    if exe:
        return exe
    # Windows: the CLI may be installed outside PATH under the user site.
    cand = os.path.join(os.environ.get("APPDATA", ""), "Python", "Python314", "Scripts", "needle.exe")
    if os.path.exists(cand):
        return cand
    print("error: `needle` CLI not found on PATH. Install with: pip install cactus-needle", file=sys.stderr)
    sys.exit(2)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--tools", required=True, help="Tool schema JSON to seed generation")
    ap.add_argument("--num-samples", type=int, default=1500)
    ap.add_argument("--out", default="data/generated.jsonl")
    ap.add_argument("--model", default="gpt-4o-mini", help="Model name at the endpoint")
    ap.add_argument("--workers", type=int, default=16)
    ap.add_argument("--augment", default=None, help="Optional existing JSONL to expand instead of seeding")
    args = ap.parse_args()

    if not os.environ.get("OPENROUTER_API_KEY"):
        print("error: set OPENROUTER_API_KEY before generating data.", file=sys.stderr)
        return 2
    os.environ.setdefault("OPENROUTER_URL", "https://api.openai.com/v1/chat/completions")

    cmd = [needle_exe(), "generate-data",
           "--num-samples", str(args.num_samples),
           "--model", args.model,
           "--workers", str(args.workers),
           "--output", args.out]
    if args.augment:
        cmd += ["--augment", args.augment]
    else:
        cmd += ["--tools", args.tools]

    print("running:", " ".join(cmd))
    print("endpoint:", os.environ["OPENROUTER_URL"])
    return subprocess.call(cmd)


if __name__ == "__main__":
    raise SystemExit(main())
