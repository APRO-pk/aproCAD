#!/usr/bin/env python3
"""Merge a LoRA adapter into the base checkpoint and export a `.cact`.

Usage:
    python scripts/build_cact.py --out my_needle.cact
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
    ap.add_argument("--base", default="checkpoints/needle2.pkl")
    ap.add_argument("--lora", default="checkpoints/needle_lora.pkl")
    ap.add_argument("--out", default="my_needle.cact")
    ap.add_argument("--bits", type=int, default=None, help="Optional quantization bits (e.g. 2)")
    args = ap.parse_args()

    cmd = [needle_exe(), "build", args.base, "--lora", args.lora, "--out", args.out]
    if args.bits:
        cmd += ["--bits", str(args.bits)]
    print("running:", " ".join(cmd))
    return subprocess.call(cmd)


if __name__ == "__main__":
    raise SystemExit(main())
