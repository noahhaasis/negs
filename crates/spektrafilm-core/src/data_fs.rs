//! Reads the shipped data files (profiles, spectral LUTs, filter databases).
//!
//! Natively these come from disk. `wasm32-unknown-unknown` has no filesystem,
//! so in the browser the page registers the files' bytes under the paths the
//! engine opens. A read of an unregistered file fails and is remembered, so
//! the page can fetch what a render asked for and render again.
use std::io;
use std::path::Path;

#[cfg(target_arch = "wasm32")]
mod store {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Arc, RwLock};

    pub static FILES: RwLock<Option<HashMap<PathBuf, Arc<[u8]>>>> = RwLock::new(None);
    pub static MISSING: RwLock<Vec<PathBuf>> = RwLock::new(Vec::new());
}

/// Make `bytes` readable at `path` (browser only).
#[cfg(target_arch = "wasm32")]
pub fn register(path: impl Into<std::path::PathBuf>, bytes: Vec<u8>) {
    let mut files = store::FILES.write().unwrap();
    files.get_or_insert_with(Default::default).insert(path.into(), bytes.into());
}

/// The unregistered paths read since the last call (browser only).
#[cfg(target_arch = "wasm32")]
pub fn take_missing() -> Vec<std::path::PathBuf> {
    std::mem::take(&mut store::MISSING.write().unwrap())
}

pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    #[cfg(target_arch = "wasm32")]
    {
        let files = store::FILES.read().unwrap();
        if let Some(bytes) = files.as_ref().and_then(|files| files.get(path)) {
            return Ok(bytes.to_vec());
        }
        store::MISSING.write().unwrap().push(path.to_owned());
        Err(io::Error::new(io::ErrorKind::NotFound, "not registered"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    std::fs::read(path)
}

/// Whether `path` is a readable file. In the browser that means registered;
/// an unregistered path is remembered as missing, as for [`read`], so the page
/// can fetch it and ask again.
pub fn is_file(path: &Path) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        let files = store::FILES.read().unwrap();
        if files.as_ref().is_some_and(|files| files.contains_key(path)) {
            return true;
        }
        store::MISSING.write().unwrap().push(path.to_owned());
        false
    }
    #[cfg(not(target_arch = "wasm32"))]
    path.is_file()
}

pub fn read_to_string(path: &Path) -> io::Result<String> {
    String::from_utf8(read(path)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
