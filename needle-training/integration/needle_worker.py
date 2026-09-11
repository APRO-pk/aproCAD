#!/usr/bin/env python3
# APRO CAD Needle worker — JSON-lines stdin/stdout sidecar.
# Keeps a warm Needle agent alive across requests so the Rust host can call
# complete() cheaply. Spawned by the Tauri command `needle_run`.
#
# Protocol: one JSON object per line.
#   request : {"id": int, "query": str, "tools": [schema,...], "reset": bool=false}
#   response: {"id": int, "ok": bool, "response": {..needle json..} | null,
#              "error": str|null, "ms": float}
#
# NOTE: for the spike the worker does NOT execute the tool call — it returns
# the model's `complete()` response as-is. In production, the Rust host reads
# function_calls, executes them against its own surface, and feeds the result
# back via a follow-up complete().

import sys, json, time
import needle

# Cache one agent keyed by a fingerprint over the tool schemas (reuse across
# turns as long as the toolset is unchanged).
_agent = None
_agent_key = None

def get_agent(tools, key):
    global _agent, _agent_key
    if key == _agent_key and _agent is not None:
        return _agent
    _agent = needle.Needle(tools=tools, system="device: desktop; locale: en-US")
    _agent_key = key
    return _agent

def fingerprint(tools):
    import hashlib
    return hashlib.sha256(json.dumps(tools, sort_keys=True).encode()).hexdigest()

def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except Exception as e:
            print(json.dumps({"id": None, "ok": False, "response": None, "error": "bad request: %s" % e, "ms": 0.0}), flush=True)
            continue
        rid = req.get("id")
        query = req.get("query", "")
        tools = req.get("tools", [])
        reset = req.get("reset", False)
        try:
            agent = get_agent(tools, fingerprint(tools))
            if reset:
                agent.reset()
            t0 = time.time()
            resp = agent.complete(query, max_new_tokens=128)
            ms = (time.time() - t0) * 1000.0
            print(json.dumps({"id": rid, "ok": True, "response": resp, "error": None, "ms": ms}), flush=True)
        except Exception as e:
            print(json.dumps({"id": rid, "ok": False, "response": None, "error": str(e), "ms": 0.0}), flush=True)

if __name__ == "__main__":
    main()
