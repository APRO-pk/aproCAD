#!/usr/bin/env python3
"""Probe-evaluate a Needle model (base or tuned) against CAD intents.

Prints OK/BAD per probe so you can see whether the model maps intent to the
right tool + arguments. Use this after every fine-tune to confirm progress.

Usage:
    # base model
    python scripts/evaluate.py
    # tuned model
    python scripts/evaluate.py --weights my_needle.cact
    # the bigger legacy schema (expect it to FAIL — tools exceed the window)
    python scripts/evaluate.py --tools schemas/needle_tools.json
"""
import argparse
import json
import os
import sys


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--tools", default="schemas/needle_tools_narrow.json")
    ap.add_argument("--weights", default=None, help="Path to a .cact; omit for the base model")
    args = ap.parse_args()

    try:
        import needle
    except ImportError:
        print("error: pip install cactus-needle", file=sys.stderr)
        return 2

    tools = json.load(open(args.tools, encoding="utf-8"))
    agent = needle.Needle(tools=tools, system="device: desktop; locale: en-US",
                          **({"weights": args.weights} if args.weights else {}))

    # (query, expected tool, expected component or None, expected key or None)
    probes = [
        ("set the nose length to 300", "set_property", "Nose", "length"),
        ("change body wall to 2.0", "set_property", "Body", "wall"),
        ("paint the nose red", "set_property", "Nose", "color"),
        ("make the body material steel", "set_property", "Body", "material"),
        ("make the nozzle throat 32", "set_throat", "Nozzle", None),
        ("set a 6 hole bolt circle dia 60 hole 8 on the fins", "set_circle", "Fins", None),
        ("add a parameter body_od = 98", "add_parameter", None, None),
        ("list the current design", "describe_vehicle", None, None),
    ]

    passed = 0
    for q, exp_tool, exp_comp, exp_key in probes:
        r = agent.complete(q, max_new_tokens=128)
        calls = r.get("function_calls", [])
        if calls:
            args_ = calls[0]["arguments"]
            ok = (calls[0]["name"] == exp_tool
                  and (exp_comp is None or args_.get("component_name") == exp_comp)
                  and (exp_key is None or args_.get("key") == exp_key))
            got = f"{calls[0]['name']} {json.dumps(args_)}"
        else:
            ok = False
            got = f"(no call) type={r.get('type')}"
        passed += 1 if ok else 0
        print(f"{'OK ' if ok else 'BAD'}  {q}\n          -> {got}")

    print(f"\n{passed}/{len(probes)} probes correct")
    return 0 if passed == len(probes) else 1


if __name__ == "__main__":
    raise SystemExit(main())
