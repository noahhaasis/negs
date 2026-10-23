//! Best-effort persistence of Vulkan's compiled pipelines. Cache failures must
//! never prevent an export. Metal does not request or create this resource.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const MAGIC: &[u8; 8] = b"NEGSPLC1";
const MAX_CACHE_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) struct DiskPipelineCache {
    pub(crate) cache: wgpu::PipelineCache,
    path: PathBuf,
    dirty: AtomicBool,
}

impl DiskPipelineCache {
    pub(crate) fn path(info: &wgpu::AdapterInfo) -> Option<PathBuf> {
        if std::env::var_os("SPEKTRAFILM_DISABLE_PIPELINE_CACHE").is_some() {
            return None;
        }
        let key = wgpu::util::pipeline_cache_key(info)?;
        let root = std::env::var_os("SPEKTRAFILM_PIPELINE_CACHE_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(default_cache_dir)?;
        Some(root.join(format!("{key}-v1.bin")))
    }

    pub(crate) fn new(device: &wgpu::Device, path: PathBuf) -> Self {
        let data = read_cache(&path);
        tracing::debug!(
            loaded_bytes = data.as_ref().map_or(0, Vec::len),
            "Vulkan pipeline cache"
        );
        // SAFETY: the payload comes only from get_data(), stored atomically in
        // the user's local cache directory. Our envelope rejects truncated or
        // corrupted files before they reach the driver. wgpu additionally
        // validates its ABI, adapter and driver key; fallback discards stale
        // data after driver/wgpu updates. Never import downloaded cache blobs.
        let cache = unsafe {
            device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
                label: Some("negs Vulkan pipelines"),
                data: data.as_deref(),
                fallback: true,
            })
        };
        Self {
            cache,
            path,
            dirty: AtomicBool::new(false),
        }
    }

    pub(crate) fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub(crate) fn flush(&self) {
        if !self.dirty.swap(false, Ordering::Relaxed) {
            return;
        }
        if let Some(data) = self.cache.get_data() {
            if let Err(error) = write_cache(&self.path, &data) {
                tracing::debug!(%error, "could not persist Vulkan pipeline cache");
            }
        }
    }
}

impl Drop for DiskPipelineCache {
    fn drop(&mut self) {
        self.flush();
    }
}

fn default_cache_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(|p| PathBuf::from(p).join("Negs").join("Cache").join("gpu"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|v| !v.is_empty())
                    .map(|p| PathBuf::from(p).join(".cache"))
            })
            .map(|p| p.join("negs").join("gpu"))
    }
}

fn read_cache(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_CACHE_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_CACHE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() < 20 || bytes.len() as u64 > MAX_CACHE_BYTES || &bytes[..8] != MAGIC {
        return None;
    }
    let size = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    let checksum = u32::from_le_bytes(bytes[16..20].try_into().ok()?);
    let data = &bytes[20..];
    if size != data.len() as u64 || checksum != crc32fast::hash(data) {
        return None;
    }
    Some(data.to_vec())
}

fn write_cache(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if data.len() as u64 + 20 > MAX_CACHE_BYTES {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("cache needs a parent"))?;
    std::fs::create_dir_all(parent)?;
    // Unique files allow multiple helpers/exports to exit concurrently. The
    // last complete snapshot wins; no reader can observe a partial write.
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(MAGIC)?;
    temp.write_all(&(data.len() as u64).to_le_bytes())?;
    temp.write_all(&crc32fast::hash(data).to_le_bytes())?;
    temp.write_all(data)?;
    temp.flush()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_replace() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gpu/cache.bin");
        assert!(read_cache(&path).is_none());
        write_cache(&path, b"first driver payload").unwrap();
        assert_eq!(read_cache(&path).unwrap(), b"first driver payload");
        write_cache(&path, b"replacement").unwrap();
        assert_eq!(read_cache(&path).unwrap(), b"replacement");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn corrupted_or_truncated_data_never_reaches_driver() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.bin");
        write_cache(&path, b"compiled payload").unwrap();
        let valid = std::fs::read(&path).unwrap();
        for length in 0..valid.len() {
            std::fs::write(&path, &valid[..length]).unwrap();
            assert!(read_cache(&path).is_none());
        }
        for index in 0..valid.len() {
            let mut corrupt = valid.clone();
            corrupt[index] ^= 1;
            std::fs::write(&path, corrupt).unwrap();
            assert!(read_cache(&path).is_none());
        }
        let mut extended = valid;
        extended.push(0);
        std::fs::write(&path, extended).unwrap();
        assert!(read_cache(&path).is_none());
    }

    #[test]
    fn unwritable_cache_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("file");
        std::fs::write(&parent, b"not a directory").unwrap();
        assert!(write_cache(&parent.join("cache"), b"payload").is_err());
        assert!(read_cache(&parent.join("cache")).is_none());
    }
}
