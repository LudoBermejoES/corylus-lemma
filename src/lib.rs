mod enclitic;
mod error;
mod map;
mod provision;
mod state;

#[cfg(test)]
mod tests;

pub use error::LemmatizationError;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub type Result<T> = std::result::Result<T, LemmatizationError>;

/// Configuration for one language's lemmatization engine.
#[derive(Clone)]
pub struct EngineConfig {
    /// Directory where per-language .lemma.fst, .lemmas.json, .version.json are stored.
    pub data_dir: PathBuf,
    /// Language code: "en" or "es".
    pub lang: String,
    /// URL of the pinned gzipped tar artifact containing the FST + lemmas.
    /// Format: tar.gz containing {lang}.lemma.fst and {lang}.lemmas.json.
    ///
    /// TODO(task 1.4): set to the hosted GitHub-raw URL after build and upload.
    pub source_url: String,
    /// Pinned SHA-256 hex string of the artifact.
    ///
    /// TODO(task 1.4): set after computing SHA-256 of the uploaded artifact.
    pub source_sha256: String,
}

impl EngineConfig {
    pub fn default_en(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            lang: "en".into(),
            // TODO(task 1.4): replace with hosted artifact URL + SHA
            source_url: "https://raw.githubusercontent.com/LudoBermejoES/corylus-lemmatization/master/artifacts/en.lemma.tar.gz".into(),
            source_sha256: "TODO_FILL_SHA256_AFTER_HOSTING".into(),
        }
    }

    pub fn default_es(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            lang: "es".into(),
            source_url: "https://raw.githubusercontent.com/LudoBermejoES/corylus-lemmatization/master/artifacts/es.lemma.tar.gz".into(),
            source_sha256: "TODO_FILL_SHA256_AFTER_HOSTING".into(),
        }
    }
}

/// Observable state of the lemmatization engine for one language.
#[derive(Clone, Debug, PartialEq)]
pub enum LemmatizationState {
    NotInstalled,
    Downloading { downloaded: u64, total: Option<u64> },
    Indexing,
    Ready,
    Error { message: String },
}

pub(crate) struct Inner {
    pub config: EngineConfig,
    pub state: LemmatizationState,
    /// The in-memory map held behind Arc; None until Ready.
    pub loaded_map: Option<Arc<map::LoadedMap>>,
}

/// Per-language lemmatization engine. One instance per language.
/// Cheap to clone (Arc-backed). The fst::Map is held in memory after load.
#[derive(Clone)]
pub struct LemmatizationEngine {
    pub(crate) inner: Arc<Mutex<Inner>>,
}

impl LemmatizationEngine {
    pub fn new(config: EngineConfig) -> Self {
        let initial_state = if state::is_installed_for(&config) {
            LemmatizationState::NotInstalled // will be upgraded to Ready after map load
        } else {
            LemmatizationState::NotInstalled
        };
        let engine = Self {
            inner: Arc::new(Mutex::new(Inner {
                config,
                state: initial_state,
                loaded_map: None,
            })),
        };
        // Attempt to load the map if already installed
        if state::is_installed_for(&engine.inner.lock().unwrap().config) {
            let _ = provision::try_load_map(engine.inner.clone());
        }
        engine
    }

    pub fn data_dir(&self) -> PathBuf {
        self.inner.lock().unwrap().config.data_dir.clone()
    }

    pub fn set_data_dir(&self, data_dir: PathBuf) {
        let mut inner = self.inner.lock().unwrap();
        inner.config.data_dir = data_dir;
        inner.loaded_map = None;
        inner.state = if state::is_installed_for(&inner.config) {
            LemmatizationState::NotInstalled // probe will upgrade
        } else {
            LemmatizationState::NotInstalled
        };
        drop(inner);
        if state::is_installed_for(&self.inner.lock().unwrap().config) {
            let _ = provision::try_load_map(self.inner.clone());
        }
    }

    pub fn state(&self) -> LemmatizationState {
        self.inner.lock().unwrap().state.clone()
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.state(), LemmatizationState::Ready)
    }

    pub fn is_installed(&self) -> bool {
        state::is_installed_for(&self.inner.lock().unwrap().config)
    }

    /// Download → verify → unpack → load. Emits state via callback.
    pub async fn provision(
        &self,
        on_progress: impl Fn(LemmatizationState) + Send + 'static,
    ) -> Result<()> {
        provision::run(self.inner.clone(), on_progress).await
    }

    /// Resolve a single surface form to its lemma.
    /// Falls back to surface form when not installed or form not in table.
    pub fn resolve(&self, surface: &str) -> String {
        let inner = self.inner.lock().unwrap();
        let lang = inner.config.lang.clone();
        let map_arc = inner.loaded_map.clone();
        drop(inner);
        match map_arc {
            Some(m) => {
                if lang == "es" {
                    m.resolve_es(surface)
                } else {
                    m.resolve(surface)
                }
            }
            None => surface.to_string(),
        }
    }

    /// Resolve a batch of surface forms in order.
    pub fn resolve_batch(&self, tokens: &[String]) -> Vec<String> {
        let inner = self.inner.lock().unwrap();
        let lang = inner.config.lang.clone();
        let map_arc = inner.loaded_map.clone();
        drop(inner);
        match map_arc {
            Some(m) => tokens
                .iter()
                .map(|t| if lang == "es" { m.resolve_es(t) } else { m.resolve(t) })
                .collect(),
            None => tokens.to_vec(),
        }
    }

    /// Remove installed data and reset to NotInstalled.
    pub fn uninstall(&self) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let config = &inner.config;
        for path in [
            state::fst_path(config),
            state::lemmas_path(config),
            state::version_path(config),
        ] {
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
        }
        inner.loaded_map = None;
        inner.state = LemmatizationState::NotInstalled;
        Ok(())
    }
}
