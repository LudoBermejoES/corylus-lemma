//! Build binary: reads {lang}.form_index.json (form → ordinal, sorted) and
//! writes {lang}.lemma.fst using the fst crate's MapBuilder.
//!
//! Usage: build_lemma_fst <lang> <form_index.json> <output.fst>
//!
//! Determinism guarantees:
//!   - Input must already be in lexicographic byte order (produced by build_fst.py).
//!   - fst::MapBuilder requires keys inserted in sorted order and enforces this.
//!   - No HashMap iteration.

use fst::MapBuilder;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::BufWriter;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("Usage: {} <lang> <form_index.json> <output.fst>", args[0]);
        std::process::exit(1);
    }
    let lang = &args[1];
    let index_path = &args[2];
    let output_path = &args[3];

    eprintln!("[build_lemma_fst] lang={lang} reading {index_path}");

    let raw = fs::read_to_string(index_path)
        .unwrap_or_else(|e| { eprintln!("Failed to read {index_path}: {e}"); std::process::exit(1); });

    // Parse into BTreeMap to guarantee sorted iteration (JSON object key order is not guaranteed)
    let map: BTreeMap<String, u64> = serde_json::from_str(&raw)
        .unwrap_or_else(|e| { eprintln!("JSON parse error: {e}"); std::process::exit(1); });

    eprintln!("[build_lemma_fst] building FST for {} with {} entries", lang, map.len());

    let out_file = fs::File::create(output_path)
        .unwrap_or_else(|e| { eprintln!("Cannot create {output_path}: {e}"); std::process::exit(1); });

    let mut builder = MapBuilder::new(BufWriter::new(out_file))
        .unwrap_or_else(|e| { eprintln!("MapBuilder::new failed: {e}"); std::process::exit(1); });

    for (form, ordinal) in &map {
        builder.insert(form.as_bytes(), *ordinal)
            .unwrap_or_else(|e| { eprintln!("Insert failed for '{form}': {e}"); std::process::exit(1); });
    }

    builder.finish()
        .unwrap_or_else(|e| { eprintln!("MapBuilder::finish failed: {e}"); std::process::exit(1); });

    eprintln!("[build_lemma_fst] wrote {output_path}");
}
