//! Three-layer retrieval: numeric SQL filter (authoritative), semantic
//! cosine, FTS5 keyword. Composite score:
//! `0.5*numeric_fit + 0.35*semantic_sim + 0.10*fts_rank + 0.05*recency_use`.

use crate::db::LibraryStore;
use crate::types::{LibraryEntry, RetrievalQuery, RetrievedHit};
use apro_embed::{cosine_sim, Embedder};

pub const W_NUMERIC: f64 = 0.50;
pub const W_SEMANTIC: f64 = 0.35;
pub const W_FTS: f64 = 0.10;
pub const W_RECENCY: f64 = 0.05;

pub struct Retrieval {
    pub store: LibraryStore,
    pub embedder: Box<dyn Embedder>,
}

impl Retrieval {
    pub fn retrieve(
        &self,
        query: &RetrievalQuery,
        limit: usize,
    ) -> Result<Vec<RetrievedHit>, String> {
        // --- Layer 1: numeric SQL filter (authoritative) ---
        let filtered = self.store.numeric_filter_ids(
            query.kind,
            query.od_mm,
            query.length_mm,
            query.length_max_mm,
            query.fits_od_mm,
            query.tolerance,
            query.clearance_mm,
        )?;

        let mut dimension_mismatch = false;
        let mut candidates: Vec<LibraryEntry> = Vec::new();

        match filtered {
            Some(ids) if !ids.is_empty() => {
                for id in &ids {
                    if let Some(e) = self.store.get(id)? {
                        candidates.push(e);
                    }
                }
            }
            Some(_) => {
                // No numeric match at all: fall back to semantic layer,
                // flag the dimension mismatch per spec.
                dimension_mismatch = true;
                candidates = if let Some(k) = query.kind {
                    self.store.list_by_kind(k)?
                } else {
                    self.store.list()?
                };
            }
            None => {
                candidates = if let Some(k) = query.kind {
                    self.store.list_by_kind(k)?
                } else {
                    self.store.list()?
                };
            }
        }

        if candidates.is_empty() {
            return Ok(vec![]);
        }

        // --- Layer 2: semantic embeddings ---
        let q_emb = if query.text.trim().is_empty() {
            None
        } else {
            Some(self.embedder.embed(&query.text))
        };

        // --- Layer 3: FTS5 keyword hits ---
        let fts_hits = if query.text.trim().is_empty() {
            Vec::new()
        } else {
            self.store.fts_search(&query.text, limit * 3)?
        };
        let fts_rank_of: std::collections::HashMap<String, f64> = fts_hits
            .iter()
            .enumerate()
            .map(|(i, (id, _))| {
                let n = fts_hits.len().max(1) as f64;
                (id.clone(), (n - i as f64) / n)
            })
            .collect();

        // --- Normalization base for recency/use boost ---
        let max_use = candidates.iter().map(|c| c.use_count).max().unwrap_or(0).max(1);
        let min_created = candidates.iter().map(|c| c.created).min().unwrap_or(0);
        let max_created = candidates.iter().map(|c| c.created).max().unwrap_or(0);
        let created_span = (max_created - min_created).max(1);

        let mut hits: Vec<RetrievedHit> = Vec::new();
        for e in &candidates {
            // numeric fit: fraction of specified numeric constraints satisfied
            let numeric_fit = numeric_fit(e, query);

            // semantic similarity
            let semantic = match &q_emb {
                Some(q) => e
                    .embedding
                    .as_ref()
                    .map(|v| cosine_sim(q, v) as f64)
                    .unwrap_or(0.0),
                None => 0.5, // no query text: neutral semantic score
            };

            // fts rank (0..1)
            let fts = fts_rank_of.get(&e.id).copied().unwrap_or(0.0);

            // recency + use boost
            let recency = 0.5 * (e.use_count as f64 / max_use as f64)
                + 0.5 * ((e.created - min_created) as f64 / created_span as f64);

            let score = W_NUMERIC * numeric_fit
                + W_SEMANTIC * semantic
                + W_FTS * fts
                + W_RECENCY * recency;

            hits.push(RetrievedHit {
                entry: e.clone(),
                score,
                dimension_mismatch,
                few_shot: e.few_shot(),
            });
        }

        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        hits.truncate(limit);
        Ok(hits)
    }
}

/// Fraction (0..1) of the numeric constraints the entry satisfies.
fn numeric_fit(e: &LibraryEntry, q: &RetrievalQuery) -> f64 {
    let mut satisfied = 0.0;
    let mut total = 0.0;

    if let Some(target) = q.od_mm {
        total += 1.0;
        if let Some(od) = e.params.od_mm {
            if (od - target).abs() <= target * q.tolerance {
                satisfied += 1.0;
            }
        }
    }
    if let Some(target) = q.length_mm {
        total += 1.0;
        if let Some(len) = e.params.length_mm {
            if (len - target).abs() <= target * q.tolerance {
                satisfied += 1.0;
            }
        }
    }
    if let Some(maxlen) = q.length_max_mm {
        total += 1.0;
        if let Some(len) = e.params.length_mm {
            if len <= maxlen {
                satisfied += 1.0;
            }
        }
    }
    if let Some(fits) = q.fits_od_mm {
        total += 1.0;
        if let Some(od) = e.params.od_mm {
            if od <= fits - q.clearance_mm {
                satisfied += 1.0;
            }
        }
    }
    if let Some(k) = q.kind {
        total += 1.0;
        if e.kind == k {
            satisfied += 1.0;
        }
    }

    if total == 0.0 { 1.0 } else { satisfied / total }
}
