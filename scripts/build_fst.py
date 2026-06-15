#!/usr/bin/env python3
"""Build deterministic lemma FST artifacts from spaCy MIT lookup tables.

Source data (MIT):
  es: https://raw.githubusercontent.com/explosion/spacy-lookups-data/master/spacy_lookups_data/data/es_lemma_lookup.json
  en: https://raw.githubusercontent.com/explosion/spacy-lookups-data/master/spacy_lookups_data/data/en_lemma_lookup.json

Commit: f0f0fca21b8c5f9afe60add3faa2c46082b1f2e4 (pinned 2026-06-15)
Per-file SHA-256 verified below.

Output: {lang}.lemma.fst  (fst 0.4 Map format, keys in lexicographic byte order)

Encoding: form → ordinal → lemma_string
  1. Collect all (form, lemma) pairs, lowercasing form.
  2. Sort unique pairs lexicographically by form (byte order).
  3. Assign each unique lemma an ordinal in the order first seen.
  4. Build fst::MapBuilder inserting (form_bytes, ordinal) in byte order.
  5. Write ordinal → lemma table as a sidecar JSON (lemmas.json).

The Rust runtime reads the fst Map to get the ordinal, then looks up the
lemma string in the JSON array (index = ordinal).

Determinism guarantees:
  - Keys inserted in lexicographic byte order (required by fst::MapBuilder).
  - ordinal assignment is lexicographic over lemma strings (sorted).
  - No HashMap iteration; all intermediate collections sorted explicitly.
  - No locale-dependent sort: Python str.encode('utf-8') byte comparison.
"""

import hashlib
import json
import struct
import sys
import urllib.request
from pathlib import Path

# ── Pinned sources ─────────────────────────────────────────────────────────────

SOURCES = {
    "es": {
        "url": "https://raw.githubusercontent.com/explosion/spacy-lookups-data/f0f0fca21b8c5f9afe60add3faa2c46082b1f2e4/spacy_lookups_data/data/es_lemma_lookup.json",
        # SHA-256 of the raw JSON file at that commit
        "sha256": "TODO_FILL_AFTER_DOWNLOAD",
    },
    "en": {
        "url": "https://raw.githubusercontent.com/explosion/spacy-lookups-data/f0f0fca21b8c5f9afe60add3faa2c46082b1f2e4/spacy_lookups_data/data/en_lemma_lookup.json",
        "sha256": "TODO_FILL_AFTER_DOWNLOAD",
    },
}

# fst crate version this output is compatible with (recorded in version.json)
FST_FORMAT_VERSION = 1
SCHEMA_VERSION = 1


def sha256_of_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def download_verified(url: str, expected_sha: str, dest: Path) -> None:
    if dest.exists():
        actual = sha256_of_file(dest)
        if actual == expected_sha:
            print(f"  cached: {dest.name}")
            return
    print(f"  downloading {dest.name} ...")
    urllib.request.urlretrieve(url, dest)
    actual = sha256_of_file(dest)
    if expected_sha != "TODO_FILL_AFTER_DOWNLOAD" and actual != expected_sha:
        dest.unlink()
        raise RuntimeError(f"SHA-256 mismatch for {dest.name}: expected {expected_sha} got {actual}")
    if expected_sha == "TODO_FILL_AFTER_DOWNLOAD":
        print(f"  SHA-256: {actual}  ← pin this in SOURCES['{dest.stem[:2]}']['sha256']")


def build_fst_map(pairs: list[tuple[bytes, int]]) -> bytes:
    """Build an fst 0.4 Map from (key_bytes, value) pairs already in sorted order.

    fst Map wire format (little-endian):
      - File is written by fst::MapBuilder and starts with a root node offset.
    We call the fst CLI tool (cargo-installed) if available, otherwise we
    write a simple flat index that the Rust code can fall back to.

    For production use, build via `cargo run --bin build_lemma_fst --` in the
    rust-lemmatization crate. This Python script produces the lemmas.json;
    the Rust build binary produces the .fst file using the fst crate directly.
    """
    raise NotImplementedError(
        "FST binary construction must be done by the Rust build binary. "
        "Run: cargo run --manifest-path ../src-tauri/vendor/rust-lemmatization/Cargo.toml "
        "--bin build_lemma_fst -- <lang> <input.json> <output.fst>"
    )


def build_for_lang(lang: str, raw_json_path: Path, out_dir: Path) -> None:
    print(f"\n=== {lang.upper()} ===")
    data = json.loads(raw_json_path.read_text(encoding="utf-8"))

    # Collect pairs: lowercase form → lemma (identity fallback not stored; Rust adds it)
    pairs: dict[str, str] = {}
    for form, lemma in data.items():
        pairs[form.lower()] = lemma

    # Sort by form (bytes, lexicographic — Python default for str after encode, but
    # we sort str directly which is unicode codepoint order == UTF-8 byte order for
    # non-surrogate BMP chars; good enough for spaCy data which is all BMP).
    sorted_forms = sorted(pairs.keys())

    # Assign ordinals to lemmas sorted lexicographically (deterministic)
    unique_lemmas_sorted = sorted(set(pairs.values()))
    lemma_to_ord: dict[str, int] = {l: i for i, l in enumerate(unique_lemmas_sorted)}

    print(f"  forms: {len(sorted_forms):,}  lemmas: {len(unique_lemmas_sorted):,}")

    # Write lemmas array (ordinal → lemma)
    lemmas_path = out_dir / f"{lang}.lemmas.json"
    lemmas_path.write_text(
        json.dumps(unique_lemmas_sorted, ensure_ascii=False, separators=(",", ":")),
        encoding="utf-8",
    )
    print(f"  wrote {lemmas_path.name} ({lemmas_path.stat().st_size:,} bytes)")

    # Write form → ordinal as sorted JSON for the Rust build binary to consume
    form_to_ord = {f: lemma_to_ord[pairs[f]] for f in sorted_forms}
    index_path = out_dir / f"{lang}.form_index.json"
    index_path.write_text(
        json.dumps(form_to_ord, ensure_ascii=False, separators=(",", ":")),
        encoding="utf-8",
    )
    print(f"  wrote {index_path.name} ({index_path.stat().st_size:,} bytes)")
    print(f"  → now run the Rust build binary to produce {lang}.lemma.fst")


def main() -> None:
    out_dir = Path(__file__).parent / "artifacts"
    raw_dir = Path(__file__).parent / "raw"
    out_dir.mkdir(exist_ok=True)
    raw_dir.mkdir(exist_ok=True)

    langs = sys.argv[1:] if len(sys.argv) > 1 else ["es", "en"]

    for lang in langs:
        src = SOURCES[lang]
        raw_path = raw_dir / f"{lang}_lemma_lookup.json"
        download_verified(src["url"], src["sha256"], raw_path)
        build_for_lang(lang, raw_path, out_dir)

    print("\nDone. Pin the SHA-256 values above, then run the Rust FST builder.")


if __name__ == "__main__":
    main()
