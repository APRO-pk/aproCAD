import json
import os

_HERE = os.path.dirname(os.path.abspath(__file__))
TOOLS = json.load(open(os.path.join(_HERE, "..", "schemas", "needle_tools_narrow.json"), encoding="utf-8"))

def row(query, name, args, reasoning=""):
    return {"query": query, "tools": TOOLS, "answers": [{"name": name, "arguments": args}], "reasoning": reasoning}

def setprop(query, comp, key, value):
    return row(query, "set_property", {"component_name": comp, "key": key, "value": value})

def setthroat(query, comp, value):
    return row(query, "set_throat", {"component_name": comp, "value": value})

def setcircle(query, comp, count, pitch, hole, depth):
    return row(query, "set_circle", {"component_name": comp, "count": count, "pitch_diameter": pitch, "hole_diameter": hole, "depth": depth})

def addparam(query, name, value):
    return row(query, "add_parameter", {"name": name, "value": value})

def describe(query):
    return row(query, "describe_vehicle", {})

cases = []
add = cases.append

# Hand-authored high-value examples with the NARROW tools.
add(setprop("set the nose length to 300", "Nose", "length", "300.0"))
add(setprop("make the nose radius 50", "Nose", "radius", "50.0"))
add(setprop("body wall 2", "Body", "wall", "2.0"))
add(setprop("make the body material steel", "Body", "material", "Steel-4130"))
add(setprop("paint the nose red", "Nose", "color", "red"))
add(setprop("paint the body green", "Body", "color", "green"))
add(setprop("change nose length to 300", "Nose", "length", "300.0"))
add(setprop("why not set body radius 30", "Body", "radius", "30.0"))
add(setthroat("make the nozzle throat 32", "Nozzle", "32.0"))
add(setthroat("nozzle throat radius 32", "Nozzle", "32.0"))
add(setcircle("set a 6 hole bolt circle dia 60 hole 8 depth 8 on the fins", "Fins", "6", "60.0", "8.0", "8.0"))
add(setcircle("bolt circle 4 holes pitch 90 hole 6 on the plate", "Body", "4", "90.0", "6.0", "6.0"))
add(addparam("add a parameter body_od = 98", "body_od", "98.0"))
add(addparam("parameter wall = 2", "wall", "2.0"))
add(describe("list the current design"))
add(describe("what components are there?"))

# Systematic sweep to guarantee coverage of each narrow tool/key.
for comp in ["Nose", "Body", "Fins", "Nozzle"]:
    for key in ["length", "radius", "wall", "color", "material"]:
        v = "red" if key == "color" else ("Al-6061-T6" if key == "material" else "30.0")
        add(setprop(f"set {comp} {key} to {v}", comp, key, v))
        add(setprop(f"make {comp} {key} {v}", comp, key, v))
for comp in ["Nozzle"]:
    add(setthroat(f"set {comp} throat to 32", comp, "32.0"))
    add(setthroat(f"{comp} throat 40", comp, "40.0"))
for comp in ["Fins", "Body"]:
    add(setcircle(f"{comp} bolt circle 6 dia 60 hole 8", comp, "6", "60.0", "8.0", "8.0"))

out = os.path.join(_HERE, "..", "data", "seed_narrow.jsonl")
with open(out, "w", encoding="utf-8") as f:
    for x in cases:
        f.write(json.dumps(x, ensure_ascii=False) + "\n")
print("seed_narrow examples:", len(cases))
