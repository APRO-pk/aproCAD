//! Component library for APRO CAD.
//!
//! Three-layer retrieval index (numeric SQL filter, semantic embeddings,
//! FTS5 keyword) with the composite score from the AI Systems Spec:
//! `0.5*numeric_fit + 0.35*semantic_sim + 0.10*fts_rank + 0.05*recency_use`.

pub mod builtins;
pub mod db;
pub mod ingest;
pub mod retrieval;
pub mod types;

pub use db::LibraryStore;
pub use retrieval::Retrieval;
pub use types::*;

use apro_embed::{Embedder, HashEmbedder};

/// Default library root: `%APPDATA%/APRO-CAD/library` on Windows.
pub fn default_library_dir() -> std::path::PathBuf {
    let base = std::env::var("APPDATA")
        .map(std::path::PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(std::path::PathBuf::from))
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    base.join("APRO-CAD").join("library")
}

/// Facade combining store + embedder.
pub struct Library {
    pub retrieval: Retrieval,
}

impl Library {
    pub fn open(dir: Option<std::path::PathBuf>, embedder: Option<Box<dyn Embedder>>) -> Result<Self, String> {
        let dir = dir.unwrap_or_else(default_library_dir);
        let store = LibraryStore::open(&dir)?;
        let embedder: Box<dyn Embedder> = embedder.unwrap_or_else(|| Box::new(HashEmbedder::default()));
        Ok(Library {
            retrieval: Retrieval { store, embedder },
        })
    }

    pub fn store(&self) -> &LibraryStore {
        &self.retrieval.store
    }

    /// Save a component from a vehicle RON.
    pub fn save_component(&self, input: &ingest::IngestInput) -> Result<String, String> {
        ingest::save_component(&self.retrieval.store, self.retrieval.embedder.as_ref(), input)
    }

    /// Save a raw single-component RON.
    pub fn save_component_ron(
        &self,
        name: String,
        description: String,
        tags: Vec<String>,
        component_ron: &str,
        source: Source,
    ) -> Result<String, String> {
        ingest::save_component_ron(
            &self.retrieval.store,
            self.retrieval.embedder.as_ref(),
            name,
            description,
            tags,
            component_ron,
            source,
        )
    }

    /// Seed the builtin corpus if the library is empty. Returns count added.
    pub fn seed_builtins(&self) -> Result<usize, String> {
        if self.store().count() > 0 {
            return Ok(0);
        }
        let mut added = 0;
        for def in builtins::builtin_defs() {
            let comp_ron = ron::to_string(&def.component)
                .map_err(|e| format!("builtin serialize failed: {e}"))?;
            match self.save_component_ron(
                def.name.clone(),
                def.description.clone(),
                def.tags.clone(),
                &comp_ron,
                Source::Builtin,
            ) {
                Ok(_) => added += 1,
                Err(e) => eprintln!("[library] builtin '{}' skipped: {e}", def.name),
            }
        }
        Ok(added)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_library() -> Library {
        let dir = tempfile::tempdir().unwrap();
        Library::open(Some(dir.keep()), None).unwrap()
    }

    fn sample_vehicle_ron() -> String {
        // Single BodyTube component vehicle
        r#"Vehicle(name: "Test", units: Millimeters, components: [
            Component(name: "Tube", material: "", visible: true,
                transform: Transform(position: (0.0, 0.0, 0.0), rotation: (0.0, 0.0, 0.0)),
                kind: BodyTube(BodyTubeParams(length: 500.0, radius: 25.0, wall: 2.0, material: "Fiberglass")))
        ])"#.into()
    }

    #[test]
    fn test_open_and_save() {
        let lib = test_library();
        let id = lib
            .save_component(&ingest::IngestInput {
                vehicle_ron: sample_vehicle_ron(),
                component_name: "Tube".into(),
                description: "Test tube".into(),
                tags: vec!["tube".into()],
                source: Source::UserSaved,
            })
            .unwrap();
        let e = lib.store().get(&id).unwrap().unwrap();
        assert_eq!(e.kind, EntryKind::BodyTube);
        assert_eq!(e.params.od_mm.unwrap(), 50.0);
        assert_eq!(e.params.length_mm.unwrap(), 500.0);
        assert!(e.mass_props.is_some());
        assert!(e.embedding.is_some());
    }

    #[test]
    fn test_retrieve_numeric() {
        let lib = test_library();
        lib.save_component(&ingest::IngestInput {
            vehicle_ron: sample_vehicle_ron(),
            component_name: "Tube".into(),
            description: "Test tube".into(),
            tags: vec!["tube".into()],
            source: Source::UserSaved,
        })
        .unwrap();

        let hits = lib
            .retrieval
            .retrieve(&RetrievalQuery { od_mm: Some(50.0), ..Default::default() }, 5)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].dimension_mismatch);
        assert!(hits[0].score > 0.5, "score={}", hits[0].score);
    }

    #[test]
    fn test_retrieve_dimension_mismatch() {
        let lib = test_library();
        lib.save_component(&ingest::IngestInput {
            vehicle_ron: sample_vehicle_ron(),
            component_name: "Tube".into(),
            description: "Test tube".into(),
            tags: vec!["tube".into()],
            source: Source::UserSaved,
        })
        .unwrap();

        // No entry with OD 200mm: fallback + mismatch flag
        let hits = lib
            .retrieval
            .retrieve(&RetrievalQuery { od_mm: Some(200.0), ..Default::default() }, 5)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].dimension_mismatch);
    }

    #[test]
    fn test_retrieve_semantic_fts() {
        let lib = test_library();
        lib.save_component(&ingest::IngestInput {
            vehicle_ron: sample_vehicle_ron(),
            component_name: "Tube".into(),
            description: "Fiberglass airframe tube".into(),
            tags: vec!["body tube".into(), "54mm".into()],
            source: Source::UserSaved,
        })
        .unwrap();
        let hits = lib
            .retrieval
            .retrieve(&RetrievalQuery { text: "fiberglass airframe tube".into(), ..Default::default() }, 5)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].score > 0.3, "score={}", hits[0].score);
    }

    #[test]
    fn test_delete() {
        let lib = test_library();
        let id = lib
            .save_component(&ingest::IngestInput {
                vehicle_ron: sample_vehicle_ron(),
                component_name: "Tube".into(),
                description: "".into(),
                tags: vec![],
                source: Source::UserSaved,
            })
            .unwrap();
        lib.store().delete(&id).unwrap();
        assert!(lib.store().get(&id).unwrap().is_none());
    }

    #[test]
    fn test_seed_builtins() {
        let lib = test_library();
        let n = lib.seed_builtins().unwrap();
        assert!(n >= 40, "seeded {n} builtins");
        let entries = lib.store().list().unwrap();
        assert_eq!(entries.len(), n);
        // every entry has a computed mass + valid ron
        for e in &entries {
            assert!(e.mass_props.is_some(), "{} missing mass", e.name);
            assert!(e.params.mass_g.is_some(), "{} missing mass_g", e.name);
            let comp: Result<apro_document::vehicle::Component, _> = ron::from_str(&e.ron);
            assert!(comp.is_ok(), "{} invalid ron", e.name);
        }
    }

    #[test]
    fn test_seed_builtins_idempotent() {
        let lib = test_library();
        lib.seed_builtins().unwrap();
        let n = lib.seed_builtins().unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn test_fts_search() {
        let lib = test_library();
        lib.seed_builtins().unwrap();
        let hits = lib.store().fts_search("nose cone", 10).unwrap();
        assert!(!hits.is_empty());
    }

    #[test]
    fn test_save_component_ron_raw() {
        let lib = test_library();
        let comp_ron = r#"Component(name: "N", material: "", visible: true,
            transform: Transform(position: (0.0, 0.0, 0.0), rotation: (0.0, 0.0, 0.0)),
            kind: NoseCone(NoseConeParams(profile: VonKarman, length: 200.0, base_radius: 25.0, wall: 3.0, material: "Fiberglass")))"#;
        let id = lib
            .save_component_ron("My Nose".into(), "desc".into(), vec!["nose".into()], comp_ron, Source::AIGenerated)
            .unwrap();
        let e = lib.store().get(&id).unwrap().unwrap();
        assert_eq!(e.kind, EntryKind::NoseCone);
        assert_eq!(e.params.od_mm.unwrap(), 50.0);
    }

    #[test]
    fn test_bump_use_counts() {
        let lib = test_library();
        let id = lib
            .save_component(&ingest::IngestInput {
                vehicle_ron: sample_vehicle_ron(),
                component_name: "Tube".into(),
                description: "".into(),
                tags: vec![],
                source: Source::UserSaved,
            })
            .unwrap();
        lib.store().bump_use_counts(&[id.clone()]).unwrap();
        let e = lib.store().get(&id).unwrap().unwrap();
        assert_eq!(e.use_count, 1);
    }
}
