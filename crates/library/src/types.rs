use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What kind of part an entry is. Mirrors `ComponentKind` + Assembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryKind {
    NoseCone,
    BodyTube,
    Tank,
    Nozzle,
    FinSet,
    Transition,
    Solid,
    Assembly,
}

impl EntryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntryKind::NoseCone => "NoseCone",
            EntryKind::BodyTube => "BodyTube",
            EntryKind::Tank => "Tank",
            EntryKind::Nozzle => "Nozzle",
            EntryKind::FinSet => "FinSet",
            EntryKind::Transition => "Transition",
            EntryKind::Solid => "Solid",
            EntryKind::Assembly => "Assembly",
        }
    }
}

impl std::fmt::Display for EntryKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where an entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    UserSaved,
    Builtin,
    AIGenerated,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::UserSaved => "UserSaved",
            Source::Builtin => "Builtin",
            Source::AIGenerated => "AIGenerated",
        }
    }
}

/// Numeric parameter layer. All mm / grams / dimensionless.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParamVector {
    pub od_mm: Option<f64>,
    pub id_mm: Option<f64>,
    pub length_mm: Option<f64>,
    pub mass_g: Option<f64>,
    pub throat_mm: Option<f64>,
    pub expansion_ratio: Option<f64>,
    pub material: Option<String>,
    pub custom: BTreeMap<String, f64>,
}

/// A single library entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub kind: EntryKind,
    pub ron: String,
    pub params: ParamVector,
    /// 384-dim (or embedder-dim) vector, serialized as flat f32.
    pub embedding: Option<Vec<f32>>,
    pub mass_props: Option<apro_massprops::MassProperties>,
    pub source: Source,
    pub created: i64,
    pub use_count: u32,
}

/// A retrieval result with its composite score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedHit {
    pub entry: LibraryEntry,
    pub score: f64,
    /// True when no entry matched the numeric filter and the semantic layer
    /// had to run unfiltered.
    pub dimension_mismatch: bool,
    /// Pre-rendered few-shot snippet for prompt injection.
    pub few_shot: String,
}

/// Input for a retrieval call.
#[derive(Debug, Clone)]
pub struct RetrievalQuery {
    /// Free-text semantic query (embed against this).
    pub text: String,
    /// Restrict to a kind. None = all.
    pub kind: Option<EntryKind>,
    /// Target outer diameter (mm). Numeric filter at tolerance.
    pub od_mm: Option<f64>,
    /// Target length max (mm). `od_mm <= value` style bound.
    pub length_max_mm: Option<f64>,
    /// Target length exact (mm). Numeric filter at tolerance.
    pub length_mm: Option<f64>,
    /// "fits inside" target: od must be <= fits_od_mm - clearance.
    pub fits_od_mm: Option<f64>,
    /// Relative tolerance for numeric filters (default 0.02).
    pub tolerance: f64,
    /// Clearance used by fits-inside queries (default 0.5 mm).
    pub clearance_mm: f64,
}

impl Default for RetrievalQuery {
    fn default() -> Self {
        RetrievalQuery {
            text: String::new(),
            kind: None,
            od_mm: None,
            length_max_mm: None,
            length_mm: None,
            fits_od_mm: None,
            tolerance: 0.02,
            clearance_mm: 0.5,
        }
    }
}

impl LibraryEntry {
    /// Text used for semantic embedding: name + description + tags.
    pub fn embed_text(&self) -> String {
        let mut s = self.name.clone();
        if !self.description.is_empty() {
            s.push_str(" ");
            s.push_str(&self.description);
        }
        if !self.tags.is_empty() {
            s.push_str(" ");
            s.push_str(&self.tags.join(" "));
        }
        s
    }

    /// Render the entry as a few-shot snippet for prompt injection.
    pub fn few_shot(&self) -> String {
        let mut out = format!("Name: {}\nDescription: {}\nKind: {}\n", self.name, self.description, self.kind);
        if let Some(p) = &self.params.od_mm {
            out.push_str(&format!("OD: {:.1} mm\n", p));
        }
        if let Some(p) = &self.params.length_mm {
            out.push_str(&format!("Length: {:.1} mm\n", p));
        }
        if let Some(p) = &self.params.throat_mm {
            out.push_str(&format!("Throat: {:.1} mm\n", p));
        }
        if let Some(p) = &self.params.expansion_ratio {
            out.push_str(&format!("Expansion ratio: {:.1}\n", p));
        }
        if let Some(m) = &self.params.material {
            out.push_str(&format!("Material: {}\n", m));
        }
        out.push_str("RON:\n```ron\n");
        out.push_str(&self.ron);
        out.push_str("\n```");
        out
    }
}
