//! Calibrated pipelines shared by the main render, the thumbnails, the tone
//! graph and exposure placement (ticket 29), keyed by
//! `preview_pipeline_cache_key`. One build of a stock serves all of them, and
//! going back to a film or paper picked a moment ago doesn't build it again.
use anyhow::Result;
use spektrafilm_core::pipeline::Pipeline;

/// Every film with its paper, and every paper with the current film, as the
/// thumbnail grid had before; about 1 MB each (mostly the f64 TC LUT).
pub const CAPACITY: usize = 40;
/// The main render's most recent keys, which scrolling the thumbnail grid
/// can't evict: an A/B between two stocks stays built.
pub const PINNED: usize = 4;

#[derive(Default)]
pub struct Pipelines {
    /// Least recently used first.
    entries: Vec<(String, Pipeline)>,
    /// The main render's keys, most recent last.
    pinned: Vec<String>,
}

impl Pipelines {
    /// The pipeline for `key`, built with `build` on a miss, and whether it
    /// was cached. Marks it most recently used; with `pin`, also as one of the
    /// main render's.
    pub fn get_or_build(
        &mut self,
        key: &str,
        pin: bool,
        build: impl FnOnce() -> Result<Pipeline>,
    ) -> Result<(&Pipeline, bool)> {
        let cached = self.entries.iter().position(|(k, _)| k == key);
        let entry = match cached {
            Some(index) => self.entries.remove(index),
            None => (key.to_owned(), build()?),
        };
        if pin {
            self.pinned.retain(|k| k != key);
            if self.pinned.len() >= PINNED {
                self.pinned.remove(0);
            }
            self.pinned.push(key.to_owned());
        }
        self.entries.push(entry);
        self.evict(CAPACITY);
        Ok((&self.entries.last().unwrap().1, cached.is_some()))
    }

    /// The cached pipeline for `key`, without building or reordering.
    pub fn peek(&self, key: &str) -> Option<&Pipeline> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, p)| p)
    }

    /// Drops the oldest unpinned entries until at most `keep` remain
    /// (pinned entries always stay).
    fn evict(&mut self, keep: usize) {
        while self.entries.len() > keep {
            let Some(index) = self.entries.iter().position(|(k, _)| !self.pinned.contains(k)) else { break };
            self.entries.remove(index);
        }
    }

    /// Releases everything but the main render's pinned pipelines (the
    /// thumbnail grid closed).
    pub fn release_unpinned(&mut self) {
        self.evict(0);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the main render has built anything.
    #[cfg(test)]
    pub fn has_main(&self) -> bool {
        !self.pinned.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spektrafilm_core::{params::RuntimeParams, profile};

    /// An uncalibrated pipeline: the cache never looks inside.
    fn stub() -> Pipeline {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let film = profile::load_profile_by_name(&data, "kodak_gold_200").unwrap();
        let paper = profile::load_profile_by_name(&data, "kodak_portra_endura").unwrap();
        Pipeline::new(film, paper, RuntimeParams::default())
    }

    #[test]
    fn thumbnails_cannot_evict_the_main_renders_recent_stocks() {
        let mut cache = Pipelines::default();
        for key in ["a", "b"] {
            cache.get_or_build(key, true, || Ok(stub())).unwrap();
        }
        for i in 0..CAPACITY * 2 {
            cache.get_or_build(&format!("t{i}"), false, || Ok(stub())).unwrap();
        }
        assert_eq!(cache.len(), CAPACITY);
        assert!(cache.peek("a").is_some() && cache.peek("b").is_some());
        let (_, cached) = cache.get_or_build("a", true, || panic!("rebuilt")).unwrap();
        assert!(cached);
        cache.release_unpinned();
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn only_the_last_main_keys_stay_pinned() {
        let mut cache = Pipelines::default();
        for i in 0..PINNED + 2 {
            cache.get_or_build(&format!("m{i}"), true, || Ok(stub())).unwrap();
        }
        cache.release_unpinned();
        assert_eq!(cache.len(), PINNED);
        assert!(cache.peek("m0").is_none() && cache.peek(&format!("m{}", PINNED + 1)).is_some());
    }
}
