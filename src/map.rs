//! In-memory fst::Map backed by the compiled lemma data.
//! Loaded once at engine startup, held behind Arc, never re-opened per lookup.

use fst::Map;
use std::path::Path;
use crate::{LemmatizationError, Result};

pub struct LoadedMap {
    /// The compiled fst::Map (memory-mapped or owned bytes).
    map: Map<Vec<u8>>,
    /// Ordinal → lemma string, aligned with the fst values.
    lemmas: Vec<String>,
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

        Ok(Self { map, lemmas })
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
        // On miss, try enclitic candidates
        for candidate in crate::enclitic::enclitic_candidates(&lower) {
            if let Some(ordinal) = self.map.get(candidate.as_bytes()) {
                if let Some(lemma) = self.lemmas.get(ordinal as usize) {
                    return lemma.clone();
                }
            }
            // Also try the candidate itself as identity (if it's a known base form)
            // spaCy identity: base forms are values not keys, so if candidate is not
            // in the table it may still be a valid base form — return it as is.
            // We only do this for 2+ character candidates to avoid noise.
            if candidate.len() >= 2 && self.looks_like_base(&candidate) {
                return candidate;
            }
        }
        // Ultimate fallback: surface form
        surface.to_string()
    }

    /// Heuristic: a candidate "looks like a base form" if it is all lowercase
    /// and at least 2 chars (very conservative — true identity test would require
    /// checking the lemmas array, but we'd need a reverse index).
    fn looks_like_base(&self, word: &str) -> bool {
        word.len() >= 2 && word.chars().all(|c| c.is_lowercase() || c == '-' || c == '\'')
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.map.len()
    }
}
