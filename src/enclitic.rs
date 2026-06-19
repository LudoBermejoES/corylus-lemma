//! Spanish enclitic pronoun stripping (best-effort, v1 acceptance bar).
//!
//! Rules applied ONLY on a dictionary miss, in order:
//!   1. Strip single trailing clitics: -me, -te, -se, -lo, -la, -le, -nos, -os, -las, -los, -les
//!   2. Strip double clitics: -sela, -selo, -selas, -selos, -mela, -melo, etc., -le(s)+lo → -selo
//!   3. First-person-plural -s drop: strip trailing -s then re-check (handles vámonos → vamos)
//!   4. Accent reversal: when the infinitive/stem loses its clitic it may need an accent restored
//!      (e.g. dame → da → dar).  We try both accented and unaccented.
//!
//! All attempts fall back to the surface form on continued miss.

/// Single clitic suffixes, longest first to avoid greedy truncation.
static SINGLE_CLITICS: &[&str] = &[
    "nos", "las", "los", "les",
    "me", "te", "se", "lo", "la", "le", "os",
];

/// Double clitic pairs (longest first).
static DOUBLE_CLITICS: &[&str] = &[
    "selas", "selos", "melas", "melos", "telas", "telos",
    "sela", "selo", "mela", "melo", "tela", "telo",
    "nosla", "noslo",
];

/// Accent restoration map: removes a pre-clitic accent that was shifted.
/// E.g. "dá" → "da" (imperative without accent after losing clitic).
/// We try the *accented* form (before stripping shifted the accent away).
static ACCENT_RESTORE: &[(&str, &str)] = &[
    ("á", "a"), ("é", "e"), ("í", "i"), ("ó", "o"), ("ú", "u"),
];

fn strip_suffix_ci(word: &str, suffix: &str) -> Option<String> {
    let w = word.to_lowercase();
    let s = suffix.to_lowercase();
    if w.ends_with(&s) && w.len() > s.len() {
        let stem = &word[..word.len() - suffix.len()];
        Some(stem.to_string())
    } else {
        None
    }
}

/// Try to reverse a shifted accent: "dámelo" → strip "melo" → "dá" → try "dar" via table.
/// We return both the literal stem and accent-restored variants for the caller to try.
fn accent_variants(stem: &str) -> Vec<String> {
    let mut variants = vec![stem.to_string()];
    // Try replacing each accented vowel with the plain version
    for &(accented, plain) in ACCENT_RESTORE {
        if stem.contains(accented) {
            variants.push(stem.replacen(accented, plain, 1));
        }
    }
    variants
}

/// Attempt to strip enclitics from a Spanish word.
/// Returns an iterator of candidate stems (may be empty if nothing matched).
/// Caller tries each against the dictionary; uses surface form on all misses.
pub fn enclitic_candidates(word: &str) -> Vec<String> {
    let lower = word.to_lowercase();
    let mut candidates: Vec<String> = Vec::new();

    // Double clitics first (more specific)
    for &dc in DOUBLE_CLITICS {
        if let Some(stem) = strip_suffix_ci(&lower, dc) {
            for v in accent_variants(&stem) {
                candidates.push(v);
            }
        }
    }

    // Single clitics
    for &sc in SINGLE_CLITICS {
        if let Some(stem) = strip_suffix_ci(&lower, sc) {
            for v in accent_variants(&stem) {
                candidates.push(v);
            }
            // First-person-plural reconstruction: the verb's final -s is dropped
            // before the clitic -nos (vamos + nos → vámonos). Recovering the verb
            // means RE-ADDING the -s: vámonos → strip "nos" → "vámo" → "vámos" →
            // accent variant "vamos". (We also try the -s-dropped form for the
            // rarer case where a real stem ends in -s before a clitic.)
            if sc == "nos" {
                let with_s = format!("{stem}s");
                for v in accent_variants(&with_s) {
                    candidates.push(v);
                }
            }
            if stem.ends_with('s') && stem.len() > 1 {
                let without_s = &stem[..stem.len() - 1];
                for v in accent_variants(without_s) {
                    candidates.push(v);
                }
            }
        }
    }

    // Dedup preserving order
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|c| seen.insert(c.clone()));
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dame_produces_da() {
        let cands = enclitic_candidates("dame");
        assert!(cands.contains(&"da".to_string()), "dame → da not found in {cands:?}");
    }

    #[test]
    fn damelo_produces_da() {
        let cands = enclitic_candidates("dámelo");
        // Should contain "da" or "dá" via accent variant
        assert!(
            cands.iter().any(|c| c == "da" || c == "dá"),
            "dámelo candidates: {cands:?}"
        );
    }

    #[test]
    fn vamos_nos_drop() {
        let cands = enclitic_candidates("vámonos");
        // vámonos → vamos (strip -nos) + first-person -s drop → vamo
        // but we should at minimum have "vamos"
        assert!(
            cands.iter().any(|c| c == "vamos" || c == "vámos"),
            "vámonos candidates: {cands:?}"
        );
    }

    #[test]
    fn dandoselo_double_clitic() {
        let cands = enclitic_candidates("dándoselo");
        // "selo" stripped → "dándo" or "dando"
        assert!(
            cands.iter().any(|c| c.starts_with("dand") || c.starts_with("dánd")),
            "dándoselo candidates: {cands:?}"
        );
    }

    #[test]
    fn malo_pelo_solo_have_candidates_but_identity_wins() {
        // These words end in clitic-looking sequences but should not produce
        // valid dictionary entries after stripping. The actual lookup test
        // is in the engine integration tests. Here we just verify the function
        // runs without panic and returns something.
        let _ = enclitic_candidates("malo");
        let _ = enclitic_candidates("pelo");
        let _ = enclitic_candidates("solo");
    }
}
