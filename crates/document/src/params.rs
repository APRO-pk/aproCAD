//! Named parameters and equations.
//!
//! A `Vehicle` may carry a `Parameters(...)` block: a list of named values
//! where each value is either a literal number or a quoted expression string
//! over previously defined parameter names, e.g.
//!
//! ```ron
//! Parameters(
//!     body_od: 98.0,
//!     wall: 2.0,
//!     body_id: "body_od - 2 * wall",
//!     fin_root: "body_od * 1.8",
//! )
//! ```
//!
//! Expressions are evaluated by a small hand-rolled parser (`eval_expr`) over
//! the resolved parameter environment. The resolver builds a dependency DAG
//! and reports cycles and unknown references instead of guessing.

use std::collections::HashMap;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A parameter value: either a literal number or an expression string.
#[derive(Debug, Clone, PartialEq, JsonSchema)]
#[serde(untagged)]
pub enum Expr {
    Number(f64),
    Expression(String),
}

impl Expr {
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Expr::Number(n) => Some(*n),
            Expr::Expression(_) => None,
        }
    }

    pub fn as_expr(&self) -> Option<&str> {
        match self {
            Expr::Expression(s) => Some(s),
            Expr::Number(_) => None,
        }
    }
}

impl Serialize for Expr {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Expr::Number(n) => n.serialize(s),
            Expr::Expression(e) => e.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for Expr {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct ExprVisitor;
        impl serde::de::Visitor<'_> for ExprVisitor {
            type Value = Expr;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a number or a quoted expression string")
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Expr, E> {
                Ok(Expr::Number(v))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Expr, E> {
                Ok(Expr::Number(v as f64))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Expr, E> {
                Ok(Expr::Number(v as f64))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Expr, E> {
                Ok(Expr::Expression(v.to_string()))
            }
        }
        d.deserialize_any(ExprVisitor)
    }
}

/// A named parameter entry in a `Parameters(...)` block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Parameter {
    pub name: String,
    pub value: Expr,
}

/// The resolved parameter environment: name -> value.
pub type ParamEnv = HashMap<String, f64>;

/// A map key read via `deserialize_identifier`. RON struct-like maps
/// (`Name(field: value, ...)`) only expose keys through the identifier
/// deserializer, so plain `String` keys fail with "expected identifier".
struct IdentString(String);

impl<'de> Deserialize<'de> for IdentString {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = IdentString;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an identifier")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<IdentString, E> {
                Ok(IdentString(v.to_string()))
            }
        }
        d.deserialize_identifier(V)
    }
}

/// Deserialize the `Vehicle.parameters` field.
///
/// Accepts three forms so both the hand-written spec style and the generated
/// list style round-trip:
/// - `parameters: None`
/// - `parameters: Some([Parameter(name: "a", value: 1.0), ...])` (list form)
/// - `parameters: Some(Parameters(a: 1.0, b: "a * 2"))` (spec block form)
pub fn de_params_opt<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<Parameter>>, D::Error> {
    struct ParamVisitor;

    impl<'de> serde::de::Visitor<'de> for ParamVisitor {
        type Value = Option<Vec<Parameter>>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("`None`, a list of `Parameter` entries, or a `Parameters(...)` block")
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D2: Deserializer<'de>>(self, d: D2) -> Result<Self::Value, D2::Error> {
            // Re-dispatch: the inner value is either a `[...]` list or a
            // `Parameters(...)` block.
            struct Inner;
            impl<'de> serde::de::Visitor<'de> for Inner {
                type Value = Option<Vec<Parameter>>;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("a list or a `Parameters(...)` block")
                }
                fn visit_seq<A: serde::de::SeqAccess<'de>>(
                    self,
                    mut seq: A,
                ) -> Result<Self::Value, A::Error> {
                    let mut out = Vec::new();
                    while let Some(p) = seq.next_element::<Parameter>()? {
                        out.push(p);
                    }
                    Ok(Some(out))
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut map: A,
                ) -> Result<Self::Value, A::Error> {
                    let mut out = Vec::new();
                    while let Some(key) = map.next_key::<IdentString>()? {
                        let value = map.next_value::<Expr>()?;
                        out.push(Parameter { name: key.0, value });
                    }
                    Ok(Some(out))
                }
            }
            d.deserialize_any(Inner)
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut out = Vec::new();
            while let Some(p) = seq.next_element::<Parameter>()? {
                out.push(p);
            }
            Ok(Some(out))
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut out = Vec::new();
            while let Some(key) = map.next_key::<String>()? {
                let value = map.next_value::<Expr>()?;
                out.push(Parameter { name: key, value });
            }
            Ok(Some(out))
        }
    }

    d.deserialize_any(ParamVisitor)
}

/// Errors that prevent a parameter block from resolving.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamError {
    /// A parameter appears more than once.
    DuplicateName(String),
    /// An expression refers to a parameter that does not exist.
    UnknownReference { param: String, reference: String },
    /// Parameters reference each other in a loop.
    Cycle(Vec<String>),
    /// The expression failed to parse or evaluate.
    Eval { param: String, message: String },
}

impl std::fmt::Display for ParamError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ParamError::DuplicateName(n) => write!(f, "duplicate parameter name '{n}'"),
            ParamError::UnknownReference { param, reference } => {
                write!(f, "parameter '{param}' references unknown parameter '{reference}'")
            }
            ParamError::Cycle(names) => {
                write!(f, "circular parameter dependency: {}", names.join(" -> "))
            }
            ParamError::Eval { param, message } => {
                write!(f, "parameter '{param}': {message}")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Expression evaluator (hand-rolled recursive descent)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    LParen,
    RParen,
    Comma,
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '+' => { out.push(Token::Plus); i += 1; }
            '-' => { out.push(Token::Minus); i += 1; }
            '*' => { out.push(Token::Star); i += 1; }
            '/' => { out.push(Token::Slash); i += 1; }
            '^' => { out.push(Token::Caret); i += 1; }
            '(' => { out.push(Token::LParen); i += 1; }
            ')' => { out.push(Token::RParen); i += 1; }
            ',' => { out.push(Token::Comma); i += 1; }
            c if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                match text.parse::<f64>() {
                    Ok(n) => out.push(Token::Number(n)),
                    Err(_) => return Err(format!("invalid number '{text}' at position {start}")),
                }
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                out.push(Token::Ident(text));
            }
            other => return Err(format!("unexpected character '{other}' at position {i}")),
        }
    }
    Ok(out)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    env: &'a ParamEnv,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<&Token> {
        let t = self.tokens.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn parse_expr(&mut self) -> Result<f64, String> {
        let mut acc = self.parse_term()?;
        while let Some(t) = self.peek() {
            match t {
                Token::Plus => { self.next(); acc += self.parse_term()?; }
                Token::Minus => { self.next(); acc -= self.parse_term()?; }
                _ => break,
            }
        }
        Ok(acc)
    }

    fn parse_term(&mut self) -> Result<f64, String> {
        let mut acc = self.parse_factor()?;
        while let Some(t) = self.peek() {
            match t {
                Token::Star => { self.next(); acc *= self.parse_factor()?; }
                Token::Slash => {
                    self.next();
                    let d = self.parse_factor()?;
                    if d == 0.0 {
                        return Err("division by zero".into());
                    }
                    acc /= d;
                }
                _ => break,
            }
        }
        Ok(acc)
    }

    fn parse_factor(&mut self) -> Result<f64, String> {
        let base = self.parse_unary()?;
        if let Some(Token::Caret) = self.peek() {
            self.next();
            let exp = self.parse_factor()?; // right-associative
            return Ok(base.powf(exp));
        }
        Ok(base)
    }

    fn parse_unary(&mut self) -> Result<f64, String> {
        if let Some(Token::Minus) = self.peek() {
            self.next();
            return Ok(-self.parse_unary()?);
        }
        self.parse_atom()
    }

    fn parse_atom(&mut self) -> Result<f64, String> {
        let tok = self.next().cloned();
        match tok {
            Some(Token::Number(n)) => Ok(n),
            Some(Token::LParen) => {
                let v = self.parse_expr()?;
                match self.next() {
                    Some(Token::RParen) => Ok(v),
                    _ => Err("expected ')'".into()),
                }
            }
            Some(Token::Ident(name)) => {
                // Function call or constant?
                if let Some(Token::LParen) = self.peek() {
                    self.next();
                    let args = self.parse_args()?;
                    return apply_function(&name, &args);
                }
                match name.as_str() {
                    "pi" => Ok(std::f64::consts::PI),
                    "e" => Ok(std::f64::consts::E),
                    _ => match self.env.get(&name) {
                        Some(&v) => Ok(v),
                        None => Err(format!("unknown name '{name}'")),
                    },
                }
            }
            _ => Err("expected a number or '('".into()),
        }
    }

    fn parse_args(&mut self) -> Result<Vec<f64>, String> {
        let mut args = Vec::new();
        if let Some(Token::RParen) = self.peek() {
            self.next();
            return Ok(args);
        }
        loop {
            args.push(self.parse_expr()?);
            match self.next() {
                Some(Token::Comma) => continue,
                Some(Token::RParen) => return Ok(args),
                _ => return Err("expected ',' or ')' in function call".into()),
            }
        }
    }
}

fn apply_function(name: &str, args: &[f64]) -> Result<f64, String> {
    match name {
        "abs" => {
            check_arity(name, args, 1)?;
            Ok(args[0].abs())
        }
        "sqrt" => {
            check_arity(name, args, 1)?;
            if args[0] < 0.0 {
                return Err("sqrt of negative value".into());
            }
            Ok(args[0].sqrt())
        }
        "sin" => {
            check_arity(name, args, 1)?;
            Ok(args[0].sin())
        }
        "cos" => {
            check_arity(name, args, 1)?;
            Ok(args[0].cos())
        }
        "tan" => {
            check_arity(name, args, 1)?;
            Ok(args[0].tan())
        }
        "min" => {
            check_arity(name, args, 2)?;
            Ok(args[0].min(args[1]))
        }
        "max" => {
            check_arity(name, args, 2)?;
            Ok(args[0].max(args[1]))
        }
        "pow" => {
            check_arity(name, args, 2)?;
            Ok(args[0].powf(args[1]))
        }
        _ => Err(format!("unknown function '{name}'")),
    }
}

fn check_arity(name: &str, args: &[f64], expected: usize) -> Result<(), String> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(format!("function '{name}' expects {expected} argument(s), got {}", args.len()))
    }
}

/// Collect the names referenced by an expression string (for the DAG).
pub fn referenced_names(expr: &str) -> Result<Vec<String>, String> {
    let tokens = tokenize(expr)?;
    let mut names = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        if let Token::Ident(name) = t {
            let is_function_call = matches!(tokens.get(i + 1), Some(Token::LParen));
            let is_const = matches!(name.as_str(), "pi" | "e");
            if !is_function_call && !is_const {
                names.push(name.clone());
            }
        }
    }
    Ok(names)
}

/// Evaluate an expression string against a parameter environment.
pub fn eval_expr(expr: &str, env: &ParamEnv) -> Result<f64, String> {
    let tokens = tokenize(expr)?;
    let mut p = Parser { tokens: &tokens, pos: 0, env };
    let v = p.parse_expr()?;
    if p.pos != p.tokens.len() {
        return Err("unexpected trailing tokens".into());
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// Parameter block resolver
// ---------------------------------------------------------------------------

/// Resolve a parameter block into an ordered list of (name, value), or a
/// [`ParamError`] describing the first failure (cycle, unknown reference,
/// duplicate, parse/eval error).
pub fn resolve_parameters(parameters: &[Parameter]) -> Result<Vec<(String, f64)>, ParamError> {
    // Duplicate detection.
    let mut seen = std::collections::HashSet::new();
    for p in parameters {
        if !seen.insert(p.name.as_str()) {
            return Err(ParamError::DuplicateName(p.name.clone()));
        }
    }

    let n = parameters.len();
    // Build adjacency: param i depends on param j if j's name appears in i's expr.
    let name_index: HashMap<&str, usize> = parameters
        .iter()
        .enumerate()
        .map(|(i, p)| (p.name.as_str(), i))
        .collect();
    let mut deps: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, p) in parameters.iter().enumerate() {
        if let Expr::Expression(e) = &p.value {
            let names = referenced_names(e)
                .map_err(|m| ParamError::Eval { param: p.name.clone(), message: m })?;
            for name in names {
                match name_index.get(name.as_str()) {
                    Some(&j) => deps[i].push(j),
                    None => return Err(ParamError::UnknownReference {
                        param: p.name.clone(),
                        reference: name,
                    }),
                }
            }
        }
    }

    // Kahn's algorithm over the dependency DAG: indeg[i] = number of
    // prerequisites of param i; dependents[j] = params that reference j.
    let mut indeg = vec![0usize; n];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        indeg[i] = deps[i].len();
        for &j in &deps[i] {
            dependents[j].push(i);
        }
    }
    let mut ready: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    let mut order = Vec::with_capacity(n);
    while let Some(i) = ready.pop() {
        order.push(i);
        for &k in &dependents[i] {
            indeg[k] -= 1;
            if indeg[k] == 0 {
                ready.push(k);
            }
        }
    }
    if order.len() != n {
        let remaining: Vec<String> = (0..n)
            .filter(|&i| indeg[i] > 0)
            .map(|i| parameters[i].name.clone())
            .collect();
        return Err(ParamError::Cycle(remaining));
    }

    // Evaluate in dependency order.
    let mut env: ParamEnv = HashMap::new();
    let mut resolved = Vec::with_capacity(n);
    for i in &order {
        let p = &parameters[*i];
        let value = match &p.value {
            Expr::Number(v) => *v,
            Expr::Expression(e) => eval_expr(e, &env)
                .map_err(|m| ParamError::Eval { param: p.name.clone(), message: m })?,
        };
        env.insert(p.name.clone(), value);
        resolved.push((p.name.clone(), value));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(items: &[(&str, Expr)]) -> Vec<Parameter> {
        items.iter().map(|(n, v)| Parameter { name: n.to_string(), value: v.clone() }).collect()
    }

    #[test]
    fn resolve_literal_and_equation() {
        let ps = params(&[
            ("body_od", Expr::Number(98.0)),
            ("wall", Expr::Number(2.0)),
            ("body_id", Expr::Expression("body_od - 2 * wall".into())),
            ("fin_root", Expr::Expression("body_od * 1.8".into())),
        ]);
        let resolved = resolve_parameters(&ps).unwrap();
        let map: HashMap<String, f64> = resolved.iter().cloned().collect();
        assert_eq!(map["body_od"], 98.0);
        assert_eq!(map["body_id"], 94.0);
        assert!((map["fin_root"] - 176.4).abs() < 1e-9);
    }

    #[test]
    fn resolve_forward_reference() {
        // Equations may reference parameters defined later in the block.
        let ps = params(&[
            ("nose_len", Expr::Expression("body_od * 5.0".into())),
            ("body_od", Expr::Number(98.0)),
        ]);
        let resolved = resolve_parameters(&ps).unwrap();
        let map: HashMap<String, f64> = resolved.iter().cloned().collect();
        assert_eq!(map["nose_len"], 490.0);
    }

    #[test]
    fn resolve_cycle_detected() {
        let ps = params(&[
            ("a", Expr::Expression("b + 1".into())),
            ("b", Expr::Expression("a + 1".into())),
        ]);
        assert!(matches!(resolve_parameters(&ps), Err(ParamError::Cycle(_))));
    }

    #[test]
    fn resolve_unknown_reference_detected() {
        let ps = params(&[("a", Expr::Expression("missing + 1".into()))]);
        assert!(matches!(
            resolve_parameters(&ps),
            Err(ParamError::UnknownReference { reference, .. }) if reference == "missing"
        ));
    }

    #[test]
    fn resolve_duplicate_detected() {
        let ps = params(&[
            ("a", Expr::Number(1.0)),
            ("a", Expr::Number(2.0)),
        ]);
        assert!(matches!(resolve_parameters(&ps), Err(ParamError::DuplicateName(_))));
    }

    #[test]
    fn resolve_eval_error_reported() {
        let ps = params(&[("a", Expr::Expression("1 / 0".into()))]);
        assert!(matches!(resolve_parameters(&ps), Err(ParamError::Eval { .. })));
    }

    #[test]
    fn expr_math_and_functions() {
        let env: ParamEnv = HashMap::new();
        assert_eq!(eval_expr("2 + 3 * 4", &env).unwrap(), 14.0);
        assert_eq!(eval_expr("(2 + 3) * 4", &env).unwrap(), 20.0);
        assert_eq!(eval_expr("2 ^ 3 ^ 2", &env).unwrap(), 512.0);
        assert_eq!(eval_expr("-5 + 10", &env).unwrap(), 5.0);
        assert_eq!(eval_expr("sqrt(9) + abs(-3)", &env).unwrap(), 6.0);
        assert_eq!(eval_expr("max(1, 5) + min(2, 3)", &env).unwrap(), 7.0);
        assert_eq!(eval_expr("pi", &env).unwrap(), std::f64::consts::PI);
        assert!(eval_expr("1 / 0", &env).is_err());
        assert!(eval_expr("2 +", &env).is_err());
        assert!(eval_expr("foo(1)", &env).is_err(), "unknown function must error");
    }

    #[test]
    fn expr_with_environment() {
        let mut env: ParamEnv = HashMap::new();
        env.insert("wall".into(), 2.0);
        env.insert("od".into(), 98.0);
        assert_eq!(eval_expr("od - 2 * wall", &env).unwrap(), 94.0);
        assert!(eval_expr("od - 2 * nope", &env).is_err(), "unknown name must error");
    }

    #[test]
    fn serde_number_and_string_roundtrip() {
        let ron_in = r#"Params(
            body_od: 98.0,
            body_id: "body_od - 2 * wall",
        )"#;
        #[derive(serde::Deserialize)]
        struct Params {
            body_od: Expr,
            body_id: Expr,
        }
        let p: Params = ron::from_str(ron_in).unwrap();
        assert_eq!(p.body_od.as_number(), Some(98.0));
        assert_eq!(p.body_id.as_expr(), Some("body_od - 2 * wall"));
        let out = ron::to_string(&p.body_od).unwrap();
        assert_eq!(out, "98.0");
        let out = ron::to_string(&p.body_id).unwrap();
        assert_eq!(out, "\"body_od - 2 * wall\"");
    }

    #[test]
    fn serde_parameter_list_roundtrip() {
        let ps = params(&[
            ("body_od", Expr::Number(98.0)),
            ("body_id", Expr::Expression("body_od - 2 * wall".into())),
        ]);
        let ron_out = ron::to_string(&ps).unwrap();
        let back: Vec<Parameter> = ron::from_str(&ron_out).unwrap();
        assert_eq!(back, ps);
    }

    #[test]
    fn vehicle_parses_spec_block_form() {
        let ron_in = r#"Vehicle(
            name: "Rocket",
            units: Millimeters,
            parameters: Some(Parameters(
                body_od: 98.0,
                wall: 2.0,
                body_id: "body_od - 2 * wall",
            )),
            components: [],
        )"#;
        let v: crate::vehicle::Vehicle = ron::from_str(ron_in).unwrap();
        let ps = v.parameters.unwrap();
        assert_eq!(ps.len(), 3);
        assert_eq!(ps[0].name, "body_od");
        assert_eq!(ps[0].value.as_number(), Some(98.0));
        assert_eq!(ps[2].name, "body_id");
        assert_eq!(ps[2].value.as_expr(), Some("body_od - 2 * wall"));
        let resolved = resolve_parameters(&ps).unwrap();
        let map: HashMap<String, f64> = resolved.iter().cloned().collect();
        assert_eq!(map["body_id"], 94.0);
    }

    #[test]
    fn vehicle_parses_list_form_and_none() {
        let ron_in = r#"Vehicle(
            name: "Rocket",
            units: Millimeters,
            parameters: Some([
                Parameter(name: "body_od", value: 98.0),
                Parameter(name: "body_id", value: "body_od * 0.9"),
            ]),
            components: [],
        )"#;
        let v: crate::vehicle::Vehicle = ron::from_str(ron_in).unwrap();
        let ps = v.parameters.unwrap();
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[1].value.as_expr(), Some("body_od * 0.9"));

        let ron_none = r#"Vehicle(name: "X", units: Millimeters, components: [])"#;
        let v: crate::vehicle::Vehicle = ron::from_str(ron_none).unwrap();
        assert!(v.parameters.is_none());
    }

    #[test]
    fn vehicle_roundtrip_preserves_parameters() {
        let ps = params(&[
            ("body_od", Expr::Number(98.0)),
            ("body_id", Expr::Expression("body_od - 2 * wall".into())),
        ]);
        let v = crate::vehicle::Vehicle { uid: None,
            name: "R".into(),
            units: crate::vehicle::Units::Millimeters,
            parameters: Some(ps),
            components: vec![],
        };
        let ron_out = ron::to_string(&v).unwrap();
        let back: crate::vehicle::Vehicle = ron::from_str(&ron_out).unwrap();
        assert_eq!(back.parameters.unwrap().len(), 2);
    }
}