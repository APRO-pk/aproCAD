//! Grammar-Constrained Generation support.
//!
//! - `schema_for_vehicle()` / `schema_for_component()` / `schema_for_patch()`:
//!   JSON Schema for the cloud path (`response_format: { type: "json_schema" }`).
//! - `generate_gbnf()`: RON-flavored GBNF grammar for the local llama.cpp path.
//!
//! Stub ops (`Shell`, `Fillet`, `Chamfer`) are excluded from the
//! schema via `#[schemars(skip)]`, so neither the JSON schema nor the GBNF
//! grammar can ever express them. The document parser still accepts them.
//! `Boolean` is supported and NOT skipped.

use serde_json::Value;

/// Ops that exist in the enum but error at evaluation. Excluded from every
/// grammar. When the kernel upgrade lands, remove them here AND from the
/// `#[schemars(skip)]` attributes in `crates/document/src/vehicle.rs`.
pub const UNIMPLEMENTED_OPS: &[&str] = &["Shell", "Fillet", "Chamfer"];

/// JSON Schema for a full `Vehicle` document.
pub fn schema_for_vehicle() -> Value {
    let schema = schemars::schema_for!(apro_document::vehicle::Vehicle);
    serde_json::to_value(schema).expect("vehicle schema serializes")
}

/// JSON Schema for a single `Component`.
pub fn schema_for_component() -> Value {
    let schema = schemars::schema_for!(apro_document::vehicle::Component);
    serde_json::to_value(schema).expect("component schema serializes")
}

/// JSON Schema for a `Patch` (structured edit).
pub fn schema_for_patch() -> Value {
    let schema = schemars::schema_for!(apro_document::patch::Patch);
    serde_json::to_value(schema).expect("patch schema serializes")
}

/// The AI agent's task plan: a fixed list of todos the loop executes one by
/// one, updating the document in real time.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename = "Plan")]
#[schemars(rename = "Plan")]
pub struct AgentPlan {
    /// One todo per distinct change the user requested; short imperative
    /// sentences (e.g. "Increase nose length to 300 mm").
    pub todos: Vec<String>,
}

/// JSON Schema for an `AgentPlan` (the loop's planning step).
pub fn schema_for_plan() -> Value {
    let schema = schemars::schema_for!(AgentPlan);
    serde_json::to_value(schema).expect("plan schema serializes")
}

// ---------------------------------------------------------------------------
// GBNF generation (RON flavor)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct GbnfError(pub String);

impl std::fmt::Display for GbnfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for GbnfError {}

pub type GbnfResult<T> = Result<T, GbnfError>;

/// Sanitize a type name into a GBNF rule name.
fn rule_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        }
    }
    if out.is_empty() {
        out.push_str("rule");
    }
    if out.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        out.insert(0, 'r');
    }
    out
}

const WS: &str = "ws";
const STRING: &str = "string";
const NUMBER: &str = "number";

struct GrammarBuilder {
    schema: Value,
    rules: std::collections::BTreeMap<String, String>,
    in_progress: Vec<String>,
}

/// Resolve a `$ref` (or plain value) against `$defs`/`definitions`. Returns
/// an owned clone so callers can hold it while mutating `self`.
fn resolve(schema: &Value, v: &Value) -> Value {
    if let Some(reference) = v.get("$ref").and_then(|r| r.as_str()) {
        if reference == "#" {
            // Self-referential schema (recursive types like `PatchList`).
            return schema.clone();
        }
        let key = reference
            .trim_start_matches("#/$defs/")
            .trim_start_matches("#/definitions/");
        for container in ["$defs", "definitions"] {
            if let Some(def) = schema.get(container).and_then(|d| d.get(key)) {
                return def.clone();
            }
        }
    }
    v.clone()
}

/// Extract a definition key from a `$ref` string.
fn ref_key(reference: &str) -> &str {
    reference
        .trim_start_matches("#/$defs/")
        .trim_start_matches("#/definitions/")
}

/// `type` keyword as a single string, ignoring nullability.
fn type_of(v: &Value) -> Option<String> {
    match v.get("type") {
        Some(t) if t.is_string() => Some(t.as_str().unwrap().to_string()),
        Some(t) if t.is_array() => t
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|x| x.as_str())
            .find(|s| *s != "null")
            .map(|s| s.to_string()),
        _ => None,
    }
}

impl GrammarBuilder {
    fn new(schema: Value) -> Self {
        GrammarBuilder {
            schema,
            rules: std::collections::BTreeMap::new(),
            in_progress: Vec::new(),
        }
    }

    fn emit(&mut self, name: &str, body: String) -> GbnfResult<()> {
        let trimmed = body.trim();
        if trimmed.is_empty() {
            return Err(GbnfError(format!("empty rule body for {name}")));
        }
        if let Some(prev) = self.rules.get(name) {
            // Two different types may own a variant with the same name (for
            // example `Path3D::Line` vs `SketchEntity::Line`). Silently keeping
            // one would hand the model the wrong field list, so this is a hard
            // error: variant rule names must be namespaced by their owner.
            if prev != trimmed {
                return Err(GbnfError(format!(
                    "rule {name} emitted twice with different bodies:\n  {prev}\n  {trimmed}"
                )));
            }
            return Ok(());
        }
        self.rules.insert(name.to_string(), trimmed.to_string());
        Ok(())
    }

    fn gen_rule(&mut self, name: &str, v: &Value, ref_hint: Option<&str>) -> GbnfResult<()> {
        if self.rules.contains_key(name) {
            return Ok(());
        }
        if self.in_progress.contains(&name.to_string()) {
            // Cycle guard: recursive types (e.g. `PatchList` -> Vec<Patch>)
            // revisit a rule while it is being generated. Rule names are
            // depth-independent, so the in-progress rule is the SAME rule the
            // outer traversal will emit; references to it may stay forward
            // (GBNF allows forward references). Skip, don't emit a stub.
            return Ok(());
        }
        self.in_progress.push(name.to_string());
        let result = self.gen_rule_inner(name, v, ref_hint);
        self.in_progress.pop();
        result
    }

    fn gen_rule_inner(&mut self, name: &str, v: &Value, ref_hint: Option<&str>) -> GbnfResult<()> {
        // Track whether this rule came through a `$ref` to a named struct:
        // RON writes those as `TypeName( ... )`.
        let ref_name = v
            .get("$ref")
            .and_then(|r| r.as_str())
            .map(|r| ref_key(r).to_string());
        let v = resolve(&self.schema, v);

        // Enum: "enum": ["a", "b"] or oneOf externally-tagged variants
        if let Some(evals) = v.get("enum").and_then(|e| e.as_array()) {
            if !evals.is_empty() {
                let alts: Vec<String> = evals
                    .iter()
                    .map(|e| match e {
                        Value::String(s) => format!("\"{}\"", s.replace('"', "\\\"")),
                        Value::Number(n) => format!("\"{}\"", n),
                        Value::Bool(b) => format!("\"{}\"", b),
                        _ => format!("{e:?}"),
                    })
                    .collect();
                return self.emit(name, alts.join(" | "));
            }
        }

        if let Some(one_of) = v.get("oneOf").and_then(|o| o.as_array()) {
            // Variant rule names are namespaced by the owning type so that two
            // enums sharing a variant name (`Path3D::Line` and
            // `SketchEntity::Line`, whose field lists differ) cannot clobber
            // each other.
            let owner = ref_name
                .or_else(|| ref_hint.map(|h| h.to_string()))
                .unwrap_or_else(|| name.to_string());
            let mut alts = Vec::new();
            for s in one_of {
                alts.push(self.variant_alt(s, &owner)?);
            }
            if alts.is_empty() {
                return Err(GbnfError(format!("oneOf with no alternatives in {name}")));
            }
            return self.emit(name, alts.join(" | "));
        }

        if let Some(any_of) = v.get("anyOf").and_then(|a| a.as_array()) {
            let non_null: Vec<&Value> = any_of
                .iter()
                .filter(|s| type_of(s).as_deref() != Some("null"))
                .collect();
            let nullable = non_null.len() != any_of.len();
            let mut alts = Vec::new();
            for s in &non_null {
                let r = format!("{name}-alt{}", alts.len());
                self.gen_rule(&r, s, None)?;
                alts.push(r);
            }
            if alts.is_empty() {
                return self.emit(name, "\"\"".to_string());
            }
            let joined = alts.join(" | ");
            if nullable {
                return self.emit(name, format!("( \"None\" | {joined} )"));
            }
            return self.emit(name, format!("( {joined} )"));
        }

        if let Some(all_of) = v.get("allOf").and_then(|a| a.as_array()) {
            let mut parts = Vec::new();
            for s in all_of {
                let r = format!("{name}-part{}", parts.len());
                self.gen_rule(&r, s, None)?;
                parts.push(r);
            }
            return self.emit(name, parts.join(" "));
        }

        let _ = ref_hint; // currently unused; kept for future naming hints

        match type_of(&v).as_deref() {
            Some("object") => {
                let props = match v.get("properties").and_then(|p| p.as_object()) {
                    Some(p) => p,
                    None => return self.emit(name, "\"()\"".to_string()),
                };
                let required: Vec<String> = v
                    .get("required")
                    .and_then(|r| r.as_array())
                    .map(|r| r.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let mut fields = Vec::new();
                for (field_name, field_schema) in props {
                    let prop_rule = format!("{name}-field-{}", rule_name(field_name));
                    self.gen_rule(&prop_rule, field_schema, None)?;
                    let pair = format!("\"{}\" {WS} \":\" {WS} {prop_rule}", field_name);
                    if required.contains(field_name) {
                        fields.push(pair);
                    } else {
                        // Optional field: may be omitted entirely (serde default)
                        fields.push(format!("( {pair} ( \",\" {WS} )? )?"));
                    }
                }
                let joined = fields.join(format!(" {WS} ").as_str());
                let body = format!("\"(\" {WS} {joined} {WS} \")\"");
                match &ref_name {
                    // Named struct through a $ref: `TypeName( ... )`
                    Some(rn) => self.emit(name, format!("\"{}\" {WS} {body}", rule_name(rn))),
                    None => self.emit(name, body),
                }
            }
            Some("array") => {
                let items = v.get("items");
                let prefix = v.get("prefixItems");
                if let Some(prefix_items) = prefix.and_then(|p| p.as_array()) {
                    // Tuple (fixed-length array): (a, b, c)
                    let mut parts = Vec::new();
                    for (i, item) in prefix_items.iter().enumerate() {
                        let r = format!("{name}-tuple{}", i);
                        self.gen_rule(&r, item, None)?;
                        parts.push(r);
                    }
                    let joined = parts.join(format!(" {WS} \",\" {WS} ").as_str());
                    self.emit(name, format!("\"(\" {WS} {joined} {WS} \")\""))
                } else if let Some(items_v) = items {
                    // serde writes a Rust fixed-size array (`[f64; 2]`) as a RON
                    // *tuple*, but schemars describes it as an `items` array
                    // with equal `minItems`/`maxItems` rather than
                    // `prefixItems`. Emitting the repeated `[...]` form here
                    // would hand the model a shape the parser rejects, so the
                    // fixed length must be honoured too.
                    let min = v.get("minItems").and_then(|m| m.as_u64());
                    let max = v.get("maxItems").and_then(|m| m.as_u64());
                    if let (Some(n), Some(m)) = (min, max) {
                        if n == m && n > 0 {
                            let mut parts = Vec::new();
                            for i in 0..n {
                                let r = format!("{name}-tuple{}", i);
                                self.gen_rule(&r, items_v, None)?;
                                parts.push(r);
                            }
                            let joined = parts.join(format!(" {WS} \",\" {WS} ").as_str());
                            return self.emit(name, format!("\"(\" {WS} {joined} {WS} \")\""));
                        }
                    }
                    let item_name = format!("{name}-item");
                    self.gen_rule(&item_name, items_v, None)?;
                    self.emit(
                        name,
                        format!("\"[\" {WS} ( {item_name} ( {WS} \",\" {WS} {item_name} )* )? {WS} \"]\""),
                    )
                } else {
                    Err(GbnfError(format!("array without items: {name}")))
                }
            }
            Some("string") => self.emit(name, STRING.to_string()),
            Some("number") | Some("integer") => self.emit(name, NUMBER.to_string()),
            Some("boolean") => self.emit(name, "\"true\" | \"false\"".to_string()),
            Some("null") => self.emit(name, "\"None\"".to_string()),
            other => Err(GbnfError(format!(
                "cannot determine type for rule {name} (type={other:?}, schema={})",
                serde_json::to_string(&v).unwrap_or_default()
            ))),
        }
    }

    /// Alternate for one variant of an externally-tagged enum
    /// (`{"title": "Variant", "type": "object", "required": ["Variant"],
    ///   "properties": {"Variant": <schema>}}`), or a string-enum entry
    /// (`{"type": "string", "enum": [...]}`).
    fn variant_alt(&mut self, s: &Value, owner: &str) -> GbnfResult<String> {
        let s = resolve(&self.schema, s);
        // `const` alternative (e.g. unit variant with a doc comment:
        // `{"type": "string", "const": "Noop"}`)
        if let Some(c) = s.get("const").and_then(|c| c.as_str()) {
            return Ok(format!("\"{}\"", c.replace('"', "\\\"")));
        }
        // String-enum alternative (unit-only Rust enum)
        if let Some(evals) = s.get("enum").and_then(|e| e.as_array()) {
            if evals.iter().all(|v| v.is_string()) {
                let alts: Vec<String> = evals
                    .iter()
                    .map(|v| format!("\"{}\"", v.as_str().unwrap().replace('"', "\\\"")))
                    .collect();
                if !alts.is_empty() {
                    return Ok(alts.join(" | "));
                }
            }
        }
        if let Some(props) = s.get("properties").and_then(|p| p.as_object()) {
            if let Some((variant_name, inner)) = props.iter().next() {
                let inner = resolve(&self.schema, inner);
                if inner.get("$ref").is_some() {
                    // Newtype variant with a named params struct:
                    // `Name(Params(...))`
                    let ref_str = inner["$ref"].as_str().unwrap();
                    let key = ref_key(ref_str);
                    let params_rule = rule_name(key);
                    self.gen_rule(&params_rule, &inner, Some(key))?;
                    Ok(format!("\"{}\" {WS} \"(\" {WS} {params_rule} {WS} \")\"", variant_name))
                } else {
                    // Struct variant: `Name(field: v, ...)`
                    let inner_rule = format!("{}-{}", rule_name(owner), rule_name(variant_name));
                    self.gen_rule(&inner_rule, &inner, None)?;
                    Ok(format!("\"{}\" {WS} {inner_rule}", variant_name))
                }
            } else {
                // Unit variant
                Ok(format!("\"{}\"", s.get("title").and_then(|t| t.as_str()).unwrap_or("Unit")))
            }
        } else {
            Err(GbnfError(format!(
                "enum alternative is not an object: {}",
                serde_json::to_string(&s).unwrap_or_default()
            )))
        }
    }

    fn finish(mut self) -> GbnfResult<String> {
        self.emit(WS, "( [ \\n\\t ] | \" \" )*".into())?;
        self.emit(STRING, "\"\\\"\" [^\\\"\\\\\\n]* \"\\\"\"".into())?;
        self.emit(NUMBER, "-? [0-9] [0-9]* ( \".\" [0-9] [0-9]* )? ( [eE] [+-]? [0-9] [0-9]* )?".into())?;

        let mut out = String::new();
        for (name, body) in &self.rules {
            out.push_str(&format!("{name} ::= {body}\n"));
        }
        Ok(out)
    }
}

/// Generate a GBNF grammar for the given JSON schema value, with `root` as
/// the entry rule. Grammar output is RON syntax (the local model emits RON
/// directly).
pub fn generate_gbnf(schema_json: &Value, root: &str) -> GbnfResult<String> {
    let mut b = GrammarBuilder::new(schema_json.clone());
    // Only *object* roots are RON named structs (e.g. `Vehicle( ... )`) and
    // need the `"Name" ws (...)` wrapper. Enum roots (e.g. `Patch`) are bare
    // variants in RON (`SetProperty(...)`) â€” wrapping them would force
    // invalid output like `Patch(SetProperty(...))`.
    let is_object = schema_json.get("type").and_then(|t| t.as_str()) == Some("object")
        || schema_json.get("properties").is_some();
    if is_object {
        if let Some(title) = schema_json.get("title").and_then(|t| t.as_str()) {
            if !title.is_empty() {
                let inner = format!("{root}-inner");
                b.gen_rule(&inner, schema_json, None)?;
                b.emit(root, format!("\"{}\" {WS} {inner}", title))?;
                return b.finish();
            }
        }
    }
    b.gen_rule(root, schema_json, None)?;
    b.finish()
}

/// Verify the GBNF string is structurally valid: every referenced rule is
/// defined, rules are defined exactly once, and the root rule exists.
pub fn validate_gbnf(gbnf: &str, root: &str) -> GbnfResult<()> {
    let mut defs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for line in gbnf.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, "::=").collect();
        if parts.len() != 2 {
            return Err(GbnfError(format!("malformed rule line: {line}")));
        }
        let name = parts[0].trim().to_string();
        if defs.contains_key(&name) {
            return Err(GbnfError(format!("rule {name} defined twice")));
        }
        defs.insert(name, parts[1].trim().to_string());
    }
    if !defs.contains_key(root) {
        return Err(GbnfError(format!("root rule {root} not defined")));
    }
    for (name, body) in &defs {
        let stripped = strip_literals(body);
        for tok in stripped.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-')) {
            // Skip empty tokens and bare operators like "-" from "-?".
            if tok.is_empty() || tok == name || !tok.contains(|c: char| c.is_ascii_alphanumeric()) {
                continue;
            }
            if !defs.contains_key(tok) {
                return Err(GbnfError(format!(
                    "rule {name} references undefined rule {tok}"
                )));
            }
        }
    }
    Ok(())
}

/// Strip quoted strings and character classes from a GBNF body so remaining
/// tokens are rule references.
fn strip_literals(body: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '"' => {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            '[' => {
                i += 1;
                while i < chars.len() && chars[i] != ']' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_json_contains(schema: &serde_json::Value, needle: &str) -> bool {
        let s = serde_json::to_string(schema).unwrap_or_default();
        s.contains(needle)
    }

    #[test]
    fn boolean_is_in_vehicle_schema() {
        let schema = schema_for_vehicle();
        assert!(schema_json_contains(&schema, "Boolean"), "vehicle schema must allow Boolean op");
        assert!(schema_json_contains(&schema, "BooleanKind"), "vehicle schema must include BooleanKind");
        assert!(schema_json_contains(&schema, "SolidRef"), "vehicle schema must include SolidRef");
    }

#[test]
    fn stub_ops_not_in_vehicle_schema() {
        let schema = schema_for_vehicle();
        for op in UNIMPLEMENTED_OPS {
            assert!(!schema_json_contains(&schema, op), "stub op {op} must not appear in schema");
        }
    }

    #[test]
    fn parameters_in_vehicle_schema() {
        let schema = schema_for_vehicle();
        assert!(schema_json_contains(&schema, "parameters"), "vehicle schema must include the parameters block");
        assert!(schema_json_contains(&schema, "Parameter"), "vehicle schema must include Parameter entries");
    }

    #[test]
    fn color_in_component_schema() {
        let schema = schema_for_component();
        assert!(schema_json_contains(&schema, "color"), "component schema must include the color field");
    }

    #[test]
    fn machined_features_in_schema() {
        let schema = schema_for_vehicle();
        for op in ["Hole", "BoltCircle", "RectPattern"] {
            assert!(schema_json_contains(&schema, op), "solid op {op} must be in the vehicle schema");
        }
    }

    #[test]
    fn sketch_types_in_vehicle_schema() {
        // The sketch data model must reach both AI grammar paths, otherwise the
        // local model can never author a sketch and the cloud schema rejects it.
        let schema = schema_for_vehicle();
        for needle in ["Sketch", "SketchParams", "SketchEntity", "SketchPlane", "Reference"] {
            assert!(schema_json_contains(&schema, needle), "vehicle schema must include {needle}");
        }
        for entity in ["Line", "Rectangle", "Circle", "Arc", "Spline"] {
            assert!(schema_json_contains(&schema, entity), "sketch entity {entity} must be in the schema");
        }
    }

    #[test]
    fn sketch_component_gets_a_gbnf_rule() {
        // `generate_gbnf` walks the schema generically; if a sketch type were
        // skipped or unnamed it would silently vanish from the grammar.
        let schema = schema_for_vehicle();
        let gbnf = generate_gbnf(&schema, "Vehicle").expect("grammar generates");
        for needle in ["\"Sketch\"", "SketchEntity-Line", "SketchEntity-Circle", "SketchEntity-Arc",
                       "SketchEntity-Spline", "SketchEntity-Rectangle"] {
            assert!(gbnf.contains(needle), "GBNF must mention {needle}");
        }
        validate_gbnf(&gbnf, "Vehicle").expect("generated grammar must be structurally valid");
    }

    #[test]
    fn sketch_entity_variants_do_not_collide_with_path3d() {
        // `Path3D::Line` and `SketchEntity::Line` share a variant name but not
        // a field list (3D point vs 2D point). Rule names are namespaced by
        // owning type, so each must get its own rule with the right arity.
        let schema = schema_for_vehicle();
        let gbnf = generate_gbnf(&schema, "Vehicle").expect("grammar generates");

        let rule_for = |name: &str| -> String {
            gbnf.lines()
                .find(|l| l.starts_with(&format!("{name} ::=")))
                .unwrap_or_else(|| panic!("missing rule {name}"))
                .to_string()
        };

        // Both variants exist as separate rules...
        let sketch_line = rule_for("SketchEntity-Line");
        let path_line = rule_for("Path3D-Line");
        assert!(sketch_line.contains("\"start\"") && sketch_line.contains("\"end\""), "{sketch_line}");
        assert!(path_line.contains("\"start\"") && path_line.contains("\"end\""), "{path_line}");

        // ...and a 2D sketch point is a two-slot tuple while a 3D sweep path
        // point is a three-slot tuple.
        let sketch_pt = rule_for("SketchEntity-Line-field-start");
        assert!(sketch_pt.starts_with("SketchEntity-Line-field-start ::= \"(\""), "2D point must be a tuple: {sketch_pt}");
        assert!(sketch_pt.contains("tuple0") && sketch_pt.contains("tuple1"), "{sketch_pt}");
        assert!(!sketch_pt.contains("tuple2"), "2D point must not have a third slot: {sketch_pt}");

        let path_pt = rule_for("Path3D-Line-field-start");
        assert!(path_pt.contains("tuple2"), "3D point must have a third slot: {path_pt}");

        validate_gbnf(&gbnf, "Vehicle").expect("grammar stays structurally valid");
    }

    #[test]
    fn sketch_plane_identifiers_are_offered() {
        // Sketches author `plane: XY` (a bare identifier), so the plane rule
        // must offer the variant names rather than a quoted string enum.
        let schema = schema_for_vehicle();
        let gbnf = generate_gbnf(&schema, "Vehicle").expect("grammar generates");
        let rule = gbnf
            .lines()
            .find(|l| {
                l.contains("\"XY\"") && l.contains("\"XZ\"") && l.contains("\"YZ\"")
            })
            .unwrap_or_else(|| panic!("no grammar rule offering the XY/XZ/YZ plane alternatives"));
        for p in ["XY", "XZ", "YZ"] {
            assert!(rule.contains(&format!("\"{p}\"")), "plane rule must offer {p}: {rule}");
        }
    }
}

