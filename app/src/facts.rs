//! The camera's facts by file (`PhotoFacts`), read on a thread the first
//! time they're asked for, for the print's caption and Compare's captions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use negs_app::photo_facts::PhotoFacts;

#[derive(Clone, Default)]
pub struct Facts {
    /// `None` while being read.
    known: Arc<Mutex<HashMap<PathBuf, Option<PhotoFacts>>>>,
}

impl Facts {
    /// A file's facts once read; the first ask starts reading them, and the
    /// canvas and Compare are redrawn when they're in.
    pub fn get(&self, path: &Path) -> Option<PhotoFacts> {
        let mut known = self.known.lock().unwrap();
        if let Some(facts) = known.get(path) {
            return facts.clone();
        }
        known.insert(path.to_path_buf(), None);
        drop(known);
        let (cache, path) = (self.known.clone(), path.to_path_buf());
        std::thread::Builder::new()
            .name("photo-facts".into())
            .spawn(move || {
                let facts = PhotoFacts::read(&path).unwrap_or_default();
                cache.lock().unwrap().insert(path, Some(facts));
                negs_app::host::post(|m| m.dirty |= negs_app::model::dirty::CANVAS | negs_app::model::dirty::COMPARE | negs_app::model::dirty::STATUS);
            })
            .ok();
        None
    }
}
