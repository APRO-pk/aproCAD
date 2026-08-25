//! Text embedding layer for APRO CAD.
//!
//! Provides a pluggable [`Embedder`] trait with two implementations:
//!
//! - [`HashEmbedder`] — deterministic, dependency-free hashing embedder.
//!   Always available; used as the default so the library works offline and
//!   tests are deterministic. Not as semantically strong as a real model.
//! - [`FastEmbedEmbedder`] (feature `fastembed`) — real transformer model
//!   (BAAI/bge-small-en-v1.5, 384-dim) via `fastembed-rs`. Bundle the model
//!   files; do not download at runtime.

/// An embedder maps text to a fixed-dimension unit vector.
pub trait Embedder: Send + Sync {
    /// Dimension of produced vectors.
    fn dim(&self) -> usize;
    /// Embed a single text into a normalized vector.
    fn embed(&self, text: &str) -> Vec<f32>;
    /// Embed a batch of texts.
    fn embed_batch(&self, texts: &[String]) -> Vec<Vec<f32>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
}

// ---------------------------------------------------------------------------
// HashEmbedder — deterministic fallback
// ---------------------------------------------------------------------------

/// Deterministic hashing embedder.
///
/// Builds a sparse bag-of-words vector from whitespace tokens and
/// character n-grams (2..=4), then normalizes to unit length. Cheap,
/// stable across runs, no model files. Dimension default 256.
pub struct HashEmbedder {
    dim: usize,
}

impl HashEmbedder {
    pub fn new(dim: usize) -> Self {
        HashEmbedder { dim }
    }
}

impl Default for HashEmbedder {
    fn default() -> Self {
        HashEmbedder::new(256)
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

impl HashEmbedder {
    fn vectorize(&self, text: &str) -> Vec<f32> {
        let mut vec = vec![0.0f32; self.dim];
        let lower = text.to_lowercase();

        // Whitespace tokens
        for tok in lower.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty()) {
            let h = fnv1a(tok.as_bytes()) % self.dim as u64;
            vec[h as usize] += 1.0;
        }

        // Character n-grams 2..=4 for robustness against single-token typos
        let bytes = lower.as_bytes();
        for n in 2..=4 {
            if bytes.len() < n { continue; }
            for i in 0..=(bytes.len() - n) {
                let h = fnv1a(&bytes[i..i + n]) % self.dim as u64;
                vec[h as usize] += 0.5;
            }
        }

        // L2 normalize
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-9 {
            for v in &mut vec { *v /= norm; }
        }
        vec
    }
}

impl Embedder for HashEmbedder {
    fn dim(&self) -> usize {
        self.dim
    }
    fn embed(&self, text: &str) -> Vec<f32> {
        self.vectorize(text)
    }
}

// ---------------------------------------------------------------------------
// FastEmbedEmbedder — real transformer model (feature "fastembed")
// ---------------------------------------------------------------------------

#[cfg(feature = "fastembed")]
pub struct FastEmbedEmbedder {
    model: std::sync::Mutex<fastembed::TextEmbedding>,
}

#[cfg(feature = "fastembed")]
impl FastEmbedEmbedder {
    /// Create from bundled model files. `cache_dir` is where the ONNX
    /// model + tokenizer live (bundle them; do not download at runtime).
    pub fn new(cache_dir: &std::path::Path) -> Result<Self, String> {
        use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
        let model = TextEmbedding::try_new(
            TextInitOptions::new(EmbeddingModel::BGESmallENv15)
                .with_cache_dir(cache_dir.to_path_buf()),
        )
        .map_err(|e| format!("failed to init embedding model: {e}"))?;
        Ok(FastEmbedEmbedder { model: std::sync::Mutex::new(model) })
    }
}

#[cfg(feature = "fastembed")]
impl Embedder for FastEmbedEmbedder {
    fn dim(&self) -> usize {
        // bge-small-en-v1.5 -> 384
        384
    }
    fn embed(&self, text: &str) -> Vec<f32> {
        let mut model = self.model.lock().unwrap();
        let prefix = format!("query: {text}");
        match model.embed(vec![prefix], None) {
            Ok(mut v) => v.pop().unwrap_or_default(),
            Err(_) => vec![],
        }
    }
    fn embed_batch(&self, texts: &[String]) -> Vec<Vec<f32>> {
        let mut model = self.model.lock().unwrap();
        let prefixed: Vec<String> = texts.iter().map(|t| format!("passage: {t}")).collect();
        match model.embed(prefixed, None) {
            Ok(v) => v,
            Err(_) => vec![],
        }
    }
}

// ---------------------------------------------------------------------------
// Cosine similarity
// ---------------------------------------------------------------------------

/// Cosine similarity between two vectors. Returns 0.0 for zero/empty vectors.
pub fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() { return 0.0; }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    let denom = (na * nb).sqrt();
    if denom < 1e-12 { 0.0 } else { dot / denom }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_embedder_dim() {
        let e = HashEmbedder::default();
        assert_eq!(e.dim(), 256);
    }

    #[test]
    fn test_hash_embedder_normalized() {
        let e = HashEmbedder::default();
        let v = e.embed("body tube 54mm fiberglass");
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-3, "norm={norm}");
    }

    #[test]
    fn test_hash_embedder_deterministic() {
        let e = HashEmbedder::default();
        let a = e.embed("nose cone ogive 75mm");
        let b = e.embed("nose cone ogive 75mm");
        assert_eq!(a, b);
    }

    #[test]
    fn test_similar_texts_more_similar() {
        let e = HashEmbedder::default();
        let a = e.embed("bell nozzle 75mm inconel");
        let b = e.embed("bell nozzle 75mm inconel thrust chamber");
        let c = e.embed("four fins balsa wood glider");
        let sim_ab = cosine_sim(&a, &b);
        let sim_ac = cosine_sim(&a, &c);
        assert!(sim_ab > sim_ac, "sim_ab={sim_ab} sim_ac={sim_ac}");
    }

    #[test]
    fn test_cosine_zero_vec() {
        assert_eq!(cosine_sim(&[], &[]), 0.0);
        assert_eq!(cosine_sim(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
    }

    #[test]
    fn test_embed_batch() {
        let e = HashEmbedder::default();
        let v = e.embed_batch(&["a".into(), "b".into()]);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].len(), 256);
    }

    #[test]
    fn test_unrelated_texts_low_similarity() {
        let e = HashEmbedder::default();
        let a = e.embed("tank dome ellipsoidal");
        let b = e.embed("rocket engine");
        let sim = cosine_sim(&a, &b);
        assert!(sim < 0.9, "sim={sim}");
    }

    #[test]
    fn test_shared_tokens_raise_similarity() {
        let e = HashEmbedder::default();
        let a = e.embed("nozzle bell throat expansion");
        let b = e.embed("nozzle bell chamber");
        let c = e.embed("fin set airfoil naca");
        let s_ab = cosine_sim(&a, &b);
        let s_ac = cosine_sim(&a, &c);
        assert!(s_ab > s_ac);
    }
}
