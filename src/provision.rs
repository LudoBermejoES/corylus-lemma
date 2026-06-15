use std::sync::{Arc, Mutex};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

use crate::{
    Inner, LemmatizationError, LemmatizationState, Result,
    state::{self, VersionFile, SCHEMA_VERSION, FST_FORMAT_VERSION},
    map::LoadedMap,
};

pub async fn run(
    inner: Arc<Mutex<Inner>>,
    on_progress: impl Fn(LemmatizationState) + Send + 'static,
) -> Result<()> {
    {
        let guard = inner.lock().unwrap();
        if state::is_installed_for(&guard.config) {
            info!("[lemmatization] already installed for {}", guard.config.lang);
            // Try to load the map if not yet loaded
            drop(guard);
            return try_load_map(inner);
        }
        // Guard: do not start if already downloading/indexing
        match &guard.state {
            LemmatizationState::Downloading { .. } | LemmatizationState::Indexing => {
                info!("[lemmatization] provision already in flight for {}", guard.config.lang);
                return Ok(());
            }
            _ => {}
        }
        std::fs::create_dir_all(&guard.config.data_dir)?;
    }

    let (url, sha256_expected, lang) = {
        let g = inner.lock().unwrap();
        (
            g.config.source_url.clone(),
            g.config.source_sha256.clone(),
            g.config.lang.clone(),
        )
    };

    let part_path = state::part_path(&inner.lock().unwrap().config);
    let _lock_path = state::lock_path(&inner.lock().unwrap().config);

    // --- Download ---
    info!("[lemmatization] downloading {} from {}", lang, url);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.map_err(LemmatizationError::Http)?;
    let total = resp.content_length();

    set_state(&inner, LemmatizationState::Downloading { downloaded: 0, total });
    on_progress(LemmatizationState::Downloading { downloaded: 0, total });

    let mut file = tokio::fs::File::create(&part_path).await?;
    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;
    let mut buf: Vec<u8> = Vec::new();

    use futures_util::StreamExt;
    let mut byte_stream = resp.bytes_stream();

    while let Some(chunk) = byte_stream.next().await {
        let chunk = chunk.map_err(LemmatizationError::Http)?;
        hasher.update(&chunk);
        downloaded += chunk.len() as u64;
        buf.extend_from_slice(&chunk);
        file.write_all(&chunk).await?;
        let s = LemmatizationState::Downloading { downloaded, total };
        set_state(&inner, s.clone());
        on_progress(s);
    }
    file.flush().await?;
    drop(file);

    // --- Verify checksum ---
    let actual = format!("{:x}", hasher.finalize());
    if actual != sha256_expected {
        let _ = std::fs::remove_file(&part_path);
        warn!("[lemmatization] checksum mismatch for {}: expected {} got {}", lang, sha256_expected, actual);
        let err = LemmatizationError::ChecksumMismatch {
            expected: sha256_expected,
            actual,
        };
        set_state(&inner, LemmatizationState::Error { message: err.to_string() });
        return Err(err);
    }
    info!("[lemmatization] checksum ok for {}", lang);

    // --- Index: unpack tarball containing {lang}.lemma.fst and {lang}.lemmas.json ---
    set_state(&inner, LemmatizationState::Indexing);
    on_progress(LemmatizationState::Indexing);

    let config = inner.lock().unwrap().config.data_dir.clone();
    unpack_tar_gz(&buf, &config).map_err(|e| LemmatizationError::Fst(e.to_string()))?;

    // --- Write version file ---
    let ver_path = state::version_path(&inner.lock().unwrap().config);
    let version = VersionFile {
        lang: lang.clone(),
        source_sha256: sha256_expected,
        schema_version: SCHEMA_VERSION,
        fst_format_version: FST_FORMAT_VERSION,
    };
    std::fs::write(&ver_path, serde_json::to_string_pretty(&version).unwrap())?;

    let _ = std::fs::remove_file(&part_path);

    // --- Load the map into memory ---
    drop(inner.lock().unwrap()); // release before try_load_map acquires
    try_load_map(inner.clone())?;
    on_progress(LemmatizationState::Ready);
    info!("[lemmatization] provision complete for {}", lang);
    Ok(())
}

fn unpack_tar_gz(data: &[u8], dest_dir: &std::path::Path) -> std::io::Result<()> {
    let cursor = std::io::Cursor::new(data);
    let gz = flate2::read::GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(gz);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();
        let filename = path.file_name().unwrap_or_default().to_os_string();
        let dest = dest_dir.join(filename);
        let mut out = std::fs::File::create(&dest)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}

pub fn try_load_map(inner: Arc<Mutex<Inner>>) -> Result<()> {
    let config_clone = {
        let g = inner.lock().unwrap();
        g.config.data_dir.clone()
    };
    let _ = config_clone; // suppress warning

    let fst_path = {
        let g = inner.lock().unwrap();
        state::fst_path(&g.config)
    };
    let lemmas_path = {
        let g = inner.lock().unwrap();
        state::lemmas_path(&g.config)
    };
    let lang = inner.lock().unwrap().config.lang.clone();

    match LoadedMap::load(&fst_path, &lemmas_path) {
        Ok(loaded) => {
            let mut g = inner.lock().unwrap();
            g.loaded_map = Some(Arc::new(loaded));
            g.state = LemmatizationState::Ready;
            info!("[lemmatization] map loaded for {}", lang);
            Ok(())
        }
        Err(e) => {
            let mut g = inner.lock().unwrap();
            g.state = LemmatizationState::Error { message: e.to_string() };
            Err(e)
        }
    }
}

fn set_state(inner: &Arc<Mutex<Inner>>, state: LemmatizationState) {
    inner.lock().unwrap().state = state;
}
