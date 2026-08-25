//! SQLite storage for the component library.
//!
//! Layout:
//! ```text
//! <library dir>/
//!   index.sqlite       metadata, params, embeddings (BLOB), FTS5 table
//!   components/        <ulid>.ron snippets (kept for round-trip sanity)
//! ```

use crate::types::{EntryKind, LibraryEntry, ParamVector, Source};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

pub const DEFAULT_LIBRARY_DIR: &str = "APRO-CAD/library";

fn kind_to_str(k: EntryKind) -> &'static str {
    k.as_str()
}

fn source_to_str(s: Source) -> &'static str {
    s.as_str()
}

fn f32_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

fn blob_to_f32(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

/// On-disk library storage handle.
pub struct LibraryStore {
    conn: Connection,
    pub dir: PathBuf,
}

impl LibraryStore {
    pub fn open(dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create library dir {dir:?}: {e}"))?;
        std::fs::create_dir_all(dir.join("components")).map_err(|e| format!("cannot create components dir: {e}"))?;
        let conn = Connection::open(dir.join("index.sqlite"))
            .map_err(|e| format!("cannot open library index: {e}"))?;
        let store = LibraryStore { conn, dir: dir.to_path_buf() };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), String> {
        self.conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS entries (
                 id            TEXT PRIMARY KEY,
                 name          TEXT NOT NULL,
                 description   TEXT NOT NULL DEFAULT '',
                 tags          TEXT NOT NULL DEFAULT '[]',
                 kind          TEXT NOT NULL,
                 ron           TEXT NOT NULL,
                 od_mm         REAL,
                 id_mm         REAL,
                 length_mm     REAL,
                 mass_g        REAL,
                 throat_mm     REAL,
                 expansion     REAL,
                 material      TEXT,
                 embedding     BLOB,
                 mass_props    TEXT,
                 source        TEXT NOT NULL,
                 created       INTEGER NOT NULL,
                 use_count     INTEGER NOT NULL DEFAULT 0
             );
             CREATE VIRTUAL TABLE IF NOT EXISTS entries_fts USING fts5(
                 name, description, content='entries', content_rowid='rowid'
             );
             CREATE TRIGGER IF NOT EXISTS entries_ai AFTER INSERT ON entries BEGIN
                 INSERT INTO entries_fts(rowid, name, description)
                 VALUES (new.rowid, new.name, new.description);
             END;
             CREATE TRIGGER IF NOT EXISTS entries_ad AFTER DELETE ON entries BEGIN
                 INSERT INTO entries_fts(entries_fts, rowid, name, description)
                 VALUES ('delete', old.rowid, old.name, old.description);
             END;
             CREATE TRIGGER IF NOT EXISTS entries_au AFTER UPDATE ON entries BEGIN
                 INSERT INTO entries_fts(entries_fts, rowid, name, description)
                 VALUES ('delete', old.rowid, old.name, old.description);
                 INSERT INTO entries_fts(rowid, name, description)
                 VALUES (new.rowid, new.name, new.description);
             END;
             ",
        )
        .map_err(|e| format!("schema init failed: {e}"))?;

        // Backfill FTS if table was created fresh
        let fts_count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM entries_fts", [], |r| r.get(0))
            .unwrap_or(0);
        let ent_count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM entries", [], |r| r.get(0))
            .unwrap_or(0);
        if fts_count == 0 && ent_count > 0 {
            self.rebuild_fts()?;
        }
        Ok(())
    }

    pub fn rebuild_fts(&self) -> Result<(), String> {
        self.conn
            .execute_batch("INSERT INTO entries_fts(rowid, name, description) SELECT rowid, name, description FROM entries;")
            .map_err(|e| format!("fts rebuild failed: {e}"))
    }

    pub fn insert(&self, entry: &LibraryEntry) -> Result<(), String> {
        let tags_json = serde_json::to_string(&entry.tags).map_err(|e| e.to_string())?;
        let mass_props_json = entry
            .mass_props
            .as_ref()
            .map(|m| serde_json::to_string(m).unwrap_or_else(|_| "{}".into()));
        let emb = entry.embedding.as_ref().map(|v| f32_to_blob(v));

        self.conn
            .execute(
                "INSERT OR REPLACE INTO entries
                 (id, name, description, tags, kind, ron, od_mm, id_mm, length_mm,
                  mass_g, throat_mm, expansion, material, embedding, mass_props, source, created, use_count)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                params![
                    entry.id,
                    entry.name,
                    entry.description,
                    tags_json,
                    kind_to_str(entry.kind),
                    entry.ron,
                    entry.params.od_mm,
                    entry.params.id_mm,
                    entry.params.length_mm,
                    entry.params.mass_g,
                    entry.params.throat_mm,
                    entry.params.expansion_ratio,
                    entry.params.material,
                    emb,
                    mass_props_json,
                    source_to_str(entry.source),
                    entry.created,
                    entry.use_count,
                ],
            )
            .map_err(|e| format!("insert failed: {e}"))?;

        // Write the .ron snippet to disk for round-trip sanity
        let path = self.dir.join("components").join(format!("{}.ron", entry.id));
        std::fs::write(&path, &entry.ron).map_err(|e| format!("cannot write snippet: {e}"))?;

        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<LibraryEntry>, String> {
        self.conn
            .query_row(
                "SELECT id,name,description,tags,kind,ron,od_mm,id_mm,length_mm,mass_g,
                        throat_mm,expansion,material,embedding,mass_props,source,created,use_count
                 FROM entries WHERE id=?1",
                [id],
                row_to_entry,
            )
            .optional()
            .map_err(|e| format!("get failed: {e}"))
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM entries WHERE id=?1", [id])
            .map_err(|e| format!("delete failed: {e}"))?;
        let path = self.dir.join("components").join(format!("{id}.ron"));
        let _ = std::fs::remove_file(path);
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<LibraryEntry>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id,name,description,tags,kind,ron,od_mm,id_mm,length_mm,mass_g,
                      throat_mm,expansion,material,embedding,mass_props,source,created,use_count
                      FROM entries ORDER BY created DESC")
            .map_err(|e| format!("list prepare failed: {e}"))?;
        let rows = stmt
            .query_map([], row_to_entry)
            .map_err(|e| format!("list query failed: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("row error: {e}"))?);
        }
        Ok(out)
    }

    pub fn list_by_kind(&self, kind: EntryKind) -> Result<Vec<LibraryEntry>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id,name,description,tags,kind,ron,od_mm,id_mm,length_mm,mass_g,
                      throat_mm,expansion,material,embedding,mass_props,source,created,use_count
                      FROM entries WHERE kind=?1 ORDER BY created DESC")
            .map_err(|e| format!("list prepare failed: {e}"))?;
        let rows = stmt
            .query_map([kind.as_str()], row_to_entry)
            .map_err(|e| format!("list query failed: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("row error: {e}"))?);
        }
        Ok(out)
    }

    pub fn count(&self) -> usize {
        self.conn
            .query_row("SELECT count(*) FROM entries", [], |r| r.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    pub fn bump_use_counts(&self, ids: &[String]) -> Result<(), String> {
        for id in ids {
            self.conn
                .execute("UPDATE entries SET use_count = use_count + 1 WHERE id=?1", [id])
                .map_err(|e| format!("use_count update failed: {e}"))?;
        }
        Ok(())
    }

    /// Update user-manageable metadata (name, description, tags). Geometry,
    /// params and embeddings are untouched; the FTS trigger reindexes.
    pub fn update_metadata(
        &self,
        id: &str,
        name: &str,
        description: &str,
        tags: &[String],
    ) -> Result<(), String> {
        let tags_json = serde_json::to_string(tags).map_err(|e| e.to_string())?;
        let n = self
            .conn
            .execute(
                "UPDATE entries SET name=?2, description=?3, tags=?4 WHERE id=?1",
                params![id, name, description, tags_json],
            )
            .map_err(|e| format!("metadata update failed: {e}"))?;
        if n == 0 {
            return Err(format!("entry '{id}' not found"));
        }
        Ok(())
    }

    pub fn update_embedding(&self, id: &str, embedding: &[f32]) -> Result<(), String> {
        self.conn
            .execute("UPDATE entries SET embedding=?2 WHERE id=?1", params![id, f32_to_blob(embedding)])
            .map(|_| ())
            .map_err(|e| format!("embedding update failed: {e}"))
    }

    /// Numeric-filtered ids: kind + od within tolerance + optional length bounds.
    /// Returns None if the numeric filter would be a no-op (nothing specified).
    pub fn numeric_filter_ids(
        &self,
        kind: Option<EntryKind>,
        od_mm: Option<f64>,
        length_mm: Option<f64>,
        length_max_mm: Option<f64>,
        fits_od_mm: Option<f64>,
        tolerance: f64,
        clearance_mm: f64,
    ) -> Result<Option<Vec<String>>, String> {
        let mut where_clauses: Vec<String> = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(k) = kind {
            where_clauses.push("kind = ?".into());
            params_vec.push(Box::new(k.as_str().to_string()));
        }
        if let Some(od) = od_mm {
            let lo = od * (1.0 - tolerance);
            let hi = od * (1.0 + tolerance);
            where_clauses.push("od_mm BETWEEN ? AND ?".into());
            params_vec.push(Box::new(lo));
            params_vec.push(Box::new(hi));
        }
        if let Some(len) = length_mm {
            let lo = len * (1.0 - tolerance);
            let hi = len * (1.0 + tolerance);
            where_clauses.push("length_mm BETWEEN ? AND ?".into());
            params_vec.push(Box::new(lo));
            params_vec.push(Box::new(hi));
        }
        if let Some(maxlen) = length_max_mm {
            where_clauses.push("length_mm <= ?".into());
            params_vec.push(Box::new(maxlen));
        }
        if let Some(fits) = fits_od_mm {
            where_clauses.push("od_mm <= ?".into());
            params_vec.push(Box::new(fits - clearance_mm));
        }

        if where_clauses.is_empty() {
            return Ok(None);
        }

        let sql = format!(
            "SELECT id FROM entries WHERE {}",
            where_clauses.join(" AND ")
        );
        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| format!("numeric filter prepare failed: {e}"))?;

        let params_ref: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(params_ref.as_slice(), |r| r.get::<_, String>(0))
            .map_err(|e| format!("numeric filter query failed: {e}"))?;

        let mut ids = Vec::new();
        for r in rows {
            ids.push(r.map_err(|e| format!("row error: {e}"))?);
        }
        Ok(Some(ids))
    }

    /// FTS5 keyword search over name/description. Returns (id, rank).
    pub fn fts_search(&self, query: &str, limit: usize) -> Result<Vec<(String, f64)>, String> {
        // FTS5 needs sanitized input: strip quotes and operators
        let cleaned: String = query
            .chars()
            .filter(|c| !matches!(c, '"' | '\'' | '(' | ')' | '*' | ':' | '-' | '^' | '~'))
            .collect();
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            return Ok(vec![]);
        }
        let terms: Vec<String> = cleaned.split_whitespace().map(|t| format!("{t}*")).collect();
        let fts_query = terms.join(" AND ");
        if fts_query.is_empty() {
            return Ok(vec![]);
        }
        let sql = format!(
            "SELECT rowid, rank FROM entries_fts WHERE entries_fts MATCH ?1 ORDER BY rank LIMIT ?2"
        );
        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| format!("fts prepare failed: {e}"))?;
        let rows = stmt
            .query_map(params![fts_query, limit as i64], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))
            })
            .map_err(|e| format!("fts query failed: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            let (rowid, rank) = r.map_err(|e| format!("row error: {e}"))?;
            // rowid -> id
            if let Some(id) = self
                .conn
                .query_row("SELECT id FROM entries WHERE rowid=?1", [rowid], |r| r.get(0))
                .ok()
            {
                out.push((id, -rank)); // rank is negated; higher = better
            }
        }
        Ok(out)
    }

    pub fn all_with_embeddings(&self) -> Result<Vec<(String, Vec<f32>)>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, embedding FROM entries WHERE embedding IS NOT NULL")
            .map_err(|e| format!("embed prepare failed: {e}"))?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)))
            .map_err(|e| format!("embed query failed: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            let (id, blob) = r.map_err(|e| format!("row error: {e}"))?;
            out.push((id, blob_to_f32(&blob)));
        }
        Ok(out)
    }
}

fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<LibraryEntry> {
    let tags_json: String = row.get(3)?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    let emb_blob: Option<Vec<u8>> = row.get(13)?;
    let mass_props_json: Option<String> = row.get(14)?;
    let mass_props = mass_props_json
        .and_then(|s| serde_json::from_str(&s).ok());
    Ok(LibraryEntry {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        tags,
        kind: parse_kind(&row.get::<_, String>(4)?),
        ron: row.get(5)?,
        params: ParamVector {
            od_mm: row.get(6)?,
            id_mm: row.get(7)?,
            length_mm: row.get(8)?,
            mass_g: row.get(9)?,
            throat_mm: row.get(10)?,
            expansion_ratio: row.get(11)?,
            material: row.get(12)?,
            custom: Default::default(),
        },
        embedding: emb_blob.map(|b| blob_to_f32(&b)),
        mass_props,
        source: parse_source(&row.get::<_, String>(15)?),
        created: row.get(16)?,
        use_count: row.get::<_, i64>(17)? as u32,
    })
}

fn parse_kind(s: &str) -> EntryKind {
    match s {
        "NoseCone" => EntryKind::NoseCone,
        "BodyTube" => EntryKind::BodyTube,
        "Tank" => EntryKind::Tank,
        "Nozzle" => EntryKind::Nozzle,
        "FinSet" => EntryKind::FinSet,
        "Transition" => EntryKind::Transition,
        "Solid" => EntryKind::Solid,
        _ => EntryKind::Assembly,
    }
}

fn parse_source(s: &str) -> Source {
    match s {
        "Builtin" => Source::Builtin,
        "AIGenerated" => Source::AIGenerated,
        _ => Source::UserSaved,
    }
}
