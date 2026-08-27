import json

# Train on the SAME schema we serve at inference — no mismatch.
TOOLS = json.load(open(r"C:\Users\henry\Documents\Projects\New Projects\TestCAD\src-tauri\needle_tools.json", encoding="utf-8"))

# Enum of valid property keys from the serving schema (for sweeping).
KEYS = [
    "length", "base_radius", "radius", "wall", "thickness", "chamber_radius",
    "throat_radius", "expansion_ratio", "percent_bell", "count", "pitch_diameter",
    "hole_diameter", "spacing_x", "spacing_y", "diameter", "depth", "color",
    "material", "profile", "cylindrical_length", "root_chord", "tip_chord", "span",
    "sweep", "pos_x", "pos_y", "pos_z", "rot_x_deg", "visible",
]
COMPS = ["Nose", "Body", "Fins", "Nozzle", "Tank"]

def row(query, answer_name, args, reasoning=""):
    return {"query": query, "tools": TOOLS,
            "answers": [{"name": answer_name, "arguments": args}],
            "reasoning": reasoning}

def patch(query, comp, key, value, reasoning="", op="SetProperty"):
    return row(query, "apply_patch_vehicle",
               {"component_name": comp, "operation": op, "key": key, "value": value},
               reasoning)

def param(query, name, value, reasoning=""):
    return row(query, "apply_patch_vehicle",
               {"component_name": "Vehicle", "operation": "SetParameter", "key": name, "value": value},
               reasoning)

def describe(query, reasoning=""):
    return row(query, "describe_vehicle", {}, reasoning)

cases = []
add = cases.append

# --- Hand-authored high-value examples (correct operation each) ---
add(patch("make the nose longer", "Nose", "length", "300.0", "'longer' -> length"))
add(patch("increase nose length to 300", "Nose", "length", "300.0"))
add(patch("set nose base radius to 50", "Nose", "base_radius", "50.0"))
add(patch("make the body wall 2", "Body", "wall", "2.0"))
add(patch("body radius 30", "Body", "radius", "30.0"))
add(patch("set the fin count to 4", "Fins", "count", "4"))
add(patch("fin thickness 3", "Fins", "thickness", "3.0"))
add(patch("make the nozzle throat 32", "Nozzle", "throat_radius", "32.0"))
add(patch("nozzle chamber radius 72", "Nozzle", "chamber_radius", "72.0"))
add(patch("nozzle expansion ratio 20", "Nozzle", "expansion_ratio", "20.0"))
add(patch("percent bell 80 for the nozzle", "Nozzle", "percent_bell", "80.0"))
add(patch("paint the nose red", "Nose", "color", "red", "'red' -> color key"))
add(patch("make the body steel", "Body", "material", "Steel-4130"))
add(patch("set tank length to 100", "Tank", "cylindrical_length", "100.0"))
add(patch("hide the nose cone", "Nose", "visible", "false", "'hide' -> visible false"))
add(patch("rotate the fins 90 degrees", "Fins", "rot_x_deg", "90.0"))
add(patch("move the nozzle up 50", "Nozzle", "pos_z", "50.0"))
add(patch("set root chord to 80 on the fins", "Fins", "root_chord", "80.0"))
add(patch("nose cone profile ogive", "Nose", "profile", "Ogive"))
add(patch("bolt circle pitch diameter 60", "Fins", "pitch_diameter", "60.0"))
add(patch("hole diameter 8 on the plate", "Body", "hole_diameter", "8.0"))
add(patch("pattern spacing x 20", "Body", "spacing_x", "20.0"))
add(patch("set the body x position to 5", "Body", "pos_x", "5.0"))
add(patch("set the tank width to 40", "Tank", "diameter", "40.0", "width -> diameter"))

# --- Parameter / equation (SetParameter) ---
add(param("add a parameter body_od = 98", "body_od", "98.0"))
add(param("define body_id as body_od - 2*wall", "body_id", "body_od - 2 * wall", "equation"))
add(param("make wall a parameter at 2", "wall", "2.0"))
add(param("define fin_root as body_od * 1.8", "fin_root", "body_od * 1.8", "equation"))
add(param("parameter nose_len = body_od * 5", "nose_len", "body_od * 5", "equation"))

# --- Describe / inventory ---
add(describe("list the current design"))
add(describe("what are all the components?"))
add(describe("show me the design inventory"))
add(describe("what is in the assembly?"))

# --- Systematic sweep across every property key x component x phrasing ---
NUMERIC = {"color": "red", "material": "Al-6061-T6", "profile": "Ogive", "visible": "false"}
def val_for(key):
    if key in NUMERIC:
        return NUMERIC[key]
    return "4" if key == "count" else "30.0"

for key in KEYS:
    v = val_for(key)
    for c in COMPS:
        add(patch(f"set {c} {key.replace('_',' ')} to {v}", c, key, v))
        add(patch(f"make {c} {key.replace('_',' ')} {v}", c, key, v))
        if key in ("length", "radius", "wall", "span", "thickness"):
            add(patch(f"increase {c} {key.replace('_',' ')} by 5", c, key, v))

out = "needle_train.jsonl"
with open(out, "w", encoding="utf-8") as f:
    for x in cases:
        assert x.get("query") and x.get("answers") and x["tools"] == TOOLS, "bad row: " + repr(x)[:80]
        f.write(json.dumps(x, ensure_ascii=False) + "\n")
print("wrote", len(cases), "examples (schema-aligned) to", out)
