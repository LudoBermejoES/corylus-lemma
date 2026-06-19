//! In-memory fst::Map backed by the compiled lemma data.
//! Loaded once at engine startup, held behind Arc, never re-opened per lookup.

use fst::Map;
use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;
use crate::{LemmatizationError, Result};

pub struct LoadedMap {
    /// The compiled fst::Map (memory-mapped or owned bytes).
    map: Map<Vec<u8>>,
    /// Ordinal → lemma string, aligned with the fst values.
    lemmas: Vec<String>,
    /// Lazily-built set of lemma strings for O(1) base-form membership checks.
    lemma_set: OnceLock<HashSet<String>>,
}

impl LoadedMap {
    pub fn load(fst_path: &Path, lemmas_path: &Path) -> Result<Self> {
        let fst_bytes = std::fs::read(fst_path)
            .map_err(|e| LemmatizationError::CorruptMap(format!("read fst: {e}")))?;
        let map = Map::new(fst_bytes)
            .map_err(|e| LemmatizationError::CorruptMap(format!("parse fst: {e}")))?;

        let lemmas_raw = std::fs::read_to_string(lemmas_path)
            .map_err(|e| LemmatizationError::CorruptMap(format!("read lemmas: {e}")))?;
        let lemmas: Vec<String> = serde_json::from_str(&lemmas_raw)
            .map_err(|e| LemmatizationError::CorruptMap(format!("parse lemmas: {e}")))?;

        Ok(Self { map, lemmas, lemma_set: OnceLock::new() })
    }

    /// Resolve a surface form to its lemma.
    /// Rule: lowercase → fst lookup → lemma via ordinal; identity fallback on miss.
    pub fn resolve(&self, surface: &str) -> String {
        let lower = surface.to_lowercase();
        if let Some(ordinal) = self.map.get(lower.as_bytes()) {
            if let Some(lemma) = self.lemmas.get(ordinal as usize) {
                return lemma.clone();
            }
        }
        surface.to_string()
    }

    /// Resolve with Spanish enclitic stripping on miss.
    pub fn resolve_es(&self, surface: &str) -> String {
        let lower = surface.to_lowercase();
        // First try direct lookup
        if let Some(ordinal) = self.map.get(lower.as_bytes()) {
            if let Some(lemma) = self.lemmas.get(ordinal as usize) {
                return lemma.clone();
            }
        }
        // On miss, try enclitic candidates. A candidate is only accepted if it is
        // a REAL word: either an inflected key in the map, or a known base form
        // (a value in `lemmas`). The previous heuristic accepted any 2+ char
        // lowercase stem, which corrupted ordinary words that merely end in a
        // clitic-looking sequence — "malo" → "ma", "pelo" → "pe", "solo" → "so".
        for candidate in crate::enclitic::enclitic_candidates(&lower) {
            if let Some(ordinal) = self.map.get(candidate.as_bytes()) {
                if let Some(lemma) = self.lemmas.get(ordinal as usize) {
                    return lemma.clone();
                }
            }
            // spaCy identity: base forms are values, not keys. Accept the candidate
            // only if it is actually a known lemma — never a speculative stem.
            if self.is_known_lemma(&candidate) {
                return candidate;
            }
        }
        // Ultimate fallback: surface form
        surface.to_string()
    }

    /// True if `word` is a known base form (a value in the lemma table).
    fn is_known_lemma(&self, word: &str) -> bool {
        self.lemma_set
            .get_or_init(|| self.lemmas.iter().cloned().collect())
            .contains(word)
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.map.len()
    }
}
