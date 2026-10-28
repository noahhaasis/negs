//! One bounded, lossless developed frame. A serial writer orders invalidation
//! and publication; the GTK thread never reads or writes pixel files.
use crate::{
    engine::{Frame, Paths},
    model::State,
    parameters,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    hash::{Hash, Hasher},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::UNIX_EPOCH,
};
const MAX_BYTES: u64 = 8192 * 8192 * 4;
#[derive(Serialize, Deserialize)]
struct Header {
    key: String,
    width: i32,
    height: i32,
    result: Value,
    checksum: u64,
}
enum Work {
    Invalidate,
    Save(u64, String, Arc<Frame>),
    Flush(mpsc::Sender<()>),
}
pub struct Cache {
    sender: mpsc::SyncSender<Work>,
    epoch: Arc<AtomicU64>,
    pub file: PathBuf,
}
impl Cache {
    pub fn new(file: PathBuf) -> Self {
        let (sender, receiver) = mpsc::sync_channel(2);
        let target = file.clone();
        let epoch = Arc::new(AtomicU64::new(0));
        let worker_epoch = epoch.clone();
        std::thread::spawn(move || {
            let mut seen = 0;
            for work in receiver {
                let current = worker_epoch.load(Ordering::Acquire);
                if current != seen {
                    let _ = fs::remove_file(&target);
                    seen = current;
                }
                match work {
                    Work::Invalidate => {
                        let _ = fs::remove_file(&target);
                    }
                    Work::Save(epoch, key, frame) => {
                        if epoch == current {
                            let _ = write(&target, key, &frame);
                        }
                        if epoch != worker_epoch.load(Ordering::Acquire) {
                            let _ = fs::remove_file(&target);
                        }
                    }
                    Work::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        Self {
            sender,
            file,
            epoch,
        }
    }
    pub fn invalidate(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        let _ = self.sender.try_send(Work::Invalidate);
    }
    pub fn save(&self, key: String, frame: Arc<Frame>) {
        let _ = self
            .sender
            .try_send(Work::Save(self.epoch.load(Ordering::Acquire), key, frame));
    }
    pub fn flush(&self) {
        let (tx, rx) = mpsc::channel();
        if self.sender.send(Work::Flush(tx)).is_ok() {
            let _ = rx.recv();
        }
    }
}
pub fn location() -> PathBuf {
    std::env::var_os("NEGS_PROFILE_DIR")
        .map(|p| PathBuf::from(p).join("cache"))
        .unwrap_or_else(|| gtk::glib::user_cache_dir().join("negs"))
        .join("rendered-preview.cache")
}
fn checksum(bytes: &[u8]) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hash);
    hash.finish()
}
fn stamp(path: &Path) -> Option<Value> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::metadata(path).ok()?;
    Some(json!([
        path.to_string_lossy(),
        meta.len(),
        meta.modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_nanos()
            .to_string(),
        meta.dev(),
        meta.ino()
    ]))
}
pub fn key(source: &Path, state: &State, defaults: &Value, paths: &Paths) -> Option<String> {
    fn scan(path: &Path, files: &mut Vec<Value>, visited: &mut BTreeSet<PathBuf>) -> Option<()> {
        if files.len() > 10_000 {
            return None;
        }
        if path.is_dir() {
            if !visited.insert(path.canonicalize().ok()?) {
                return Some(());
            }
            let mut entries = fs::read_dir(path)
                .ok()?
                .collect::<std::io::Result<Vec<_>>>()
                .ok()?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                scan(&entry.path(), files, visited)?;
            }
        } else {
            files.push(stamp(path)?);
        }
        Some(())
    }
    let mut files = vec![stamp(source)?];
    if let Some(payload) = package_identity(paths) {
        files.push(payload);
    } else {
        files.push(stamp(&paths.helper)?);
        scan(&paths.data, &mut files, &mut BTreeSet::new())?;
    }
    for path in std::iter::once(PathBuf::from("/usr/lib/libopenblas.so.0")).chain(
        std::env::var_os("LD_LIBRARY_PATH")
            .into_iter()
            .flat_map(|p| {
                std::env::split_paths(&p)
                    .map(|p| p.join("libopenblas.so.0"))
                    .collect::<Vec<_>>()
            }),
    ) {
        if path.is_file() {
            files.push(stamp(&path)?);
        }
    }
    let variables = [
        "SPEKTRAFILM_BACKEND",
        "SPEKTRAFILM_WGPU_WORKGROUP_SIZE",
        "SPEKTRAFILM_WGPU_FORCE_STAGING",
        "SPEKTRAFILM_WGPU_MEMORY_LIMIT_MB",
        "VK_ICD_FILENAMES",
        "VK_DRIVER_FILES",
        "DRI_PRIME",
    ]
    .map(|name| (name, std::env::var(name).ok()));
    // A driver/GPU change can alter GPU grain. Inspect small local identities
    // on this worker rather than constructing a Vulkan device during startup.
    let mut hardware = Vec::new();
    for path in ["/proc/driver/nvidia/version", "/proc/sys/kernel/osrelease"] {
        hardware.push((path.to_owned(), fs::read_to_string(path).ok()));
    }
    // Flatpak GL extensions and other loaders' ICDs live under XDG_DATA_DIRS.
    let mut directories: Vec<PathBuf> = ["/sys/class/drm", "/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d"]
        .map(PathBuf::from)
        .into();
    for variable in ["XDG_DATA_DIRS", "XDG_CONFIG_DIRS"] {
        if let Some(value) = std::env::var_os(variable) {
            for path in std::env::split_paths(&value).take(64) {
                let path = path.join("vulkan/icd.d");
                if !directories.contains(&path) {
                    directories.push(path);
                }
            }
        }
    }
    for directory in &directories {
        let directory = directory.as_path();
        if let Ok(entries) = fs::read_dir(directory) {
            let mut entries: Vec<_> = entries.flatten().take(256).map(|e| e.path()).collect();
            entries.sort();
            for entry in entries {
                if directory == Path::new("/sys/class/drm") {
                    for name in ["vendor", "device", "revision", "subsystem_device"] {
                        let path = entry.join("device").join(name);
                        if let Ok(value) = fs::read_to_string(&path) {
                            hardware.push((path.to_string_lossy().into_owned(), Some(value)));
                        }
                    }
                } else if let Some(value) = stamp(&entry) {
                    files.push(value);
                }
            }
        }
    }
    serde_json::to_string(&json!({"schema":2,"files":files,"hardware":hardware,"render":identity(state,defaults),"environment":variables})).ok()
}
/// A packaged build's immutable payload is identified by its build-time
/// digest, so an AppImage keeps cache hits across mount paths and inodes.
fn package_identity(paths: &Paths) -> Option<Value> {
    let root = PathBuf::from(std::env::var_os("NEGS_PACKAGE_ROOT")?);
    if !paths.helper.starts_with(&root) || !paths.data.starts_with(&root) {
        return None;
    }
    let bytes = fs::read(root.join("share/negs/payload.json")).ok()?;
    let payload: Value = serde_json::from_slice(&bytes).ok()?;
    payload.get("digest")?.as_str()?;
    Some(payload)
}
pub fn identity(state: &State, defaults: &Value) -> Value {
    let params = parameters::runtime(state, defaults);
    let extent =
        (!params["io"]["photo_long_side_px"].is_null()).then(|| state.crop.map(|c| [c[2], c[3]]));
    json!({"params":params,"extent":extent,
        "film":state.choices.get("Film ID").cloned().unwrap_or_else(||crate::catalog::selected(state,false).id.clone()),
        "paper":state.choices.get("Paper ID").cloned().unwrap_or_else(||crate::catalog::selected(state,true).id.clone()),
        "limit":crate::inspector::flag(state,"Limit preview to 12 MP",false)})
}
pub fn read(file: &Path, key: &str) -> Option<Arc<Frame>> {
    let size = fs::metadata(file).ok()?.len();
    if size > MAX_BYTES + 1_048_576 {
        return None;
    }
    let mut input = std::io::BufReader::new(fs::File::open(file).ok()?);
    use std::io::BufRead;
    let mut header = Vec::new();
    (&mut input)
        .take(1_048_576)
        .read_until(b'\n', &mut header)
        .ok()?;
    if header.last() != Some(&b'\n') {
        return None;
    }
    let header: Header = serde_json::from_slice(&header).ok()?;
    if header.key != key
        || !(1..=8192).contains(&header.width)
        || !(1..=8192).contains(&header.height)
    {
        return None;
    }
    let bytes = header.width as u64 * header.height as u64 * 4;
    if bytes > MAX_BYTES || input.buffer().len() as u64 > MAX_BYTES {
        return None;
    }
    let mut pixels = Vec::new();
    input.take(bytes + 1).read_to_end(&mut pixels).ok()?;
    if pixels.len() as u64 != bytes || checksum(&pixels) != header.checksum {
        return None;
    }
    for name in [
        "source_width",
        "source_height",
        "photo_width",
        "photo_height",
        "working_width",
        "working_height",
    ] {
        if !header.result[name]
            .as_f64()
            .is_some_and(|v| v.is_finite() && v >= 1. && v <= u32::MAX as f64)
        {
            return None;
        }
    }
    Some(Arc::new(Frame {
        width: header.width,
        height: header.height,
        result: header.result,
        pixels: pixels.into(),
    }))
}
fn write(file: &Path, key: String, frame: &Frame) -> std::io::Result<()> {
    if !(1..=8192).contains(&frame.width)
        || !(1..=8192).contains(&frame.height)
        || frame.pixels.len() != frame.width as usize * frame.height as usize * 4
    {
        return Err(std::io::Error::other("Invalid cached frame"));
    }
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let stage = file.with_extension("tmp");
    let result = (|| {
        let mut output = fs::File::create(&stage)?;
        serde_json::to_writer(
            &mut output,
            &Header {
                key,
                width: frame.width,
                height: frame.height,
                result: frame.result.clone(),
                checksum: checksum(&frame.pixels),
            },
        )?;
        output.write_all(b"\n")?;
        output.write_all(&frame.pixels)?;
        output.flush()?;
        drop(output);
        fs::rename(&stage, file)
    })();
    let _ = fs::remove_file(stage);
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lossless_frame_rejects_wrong_identity_and_corruption() {
        let scratch = crate::engine::Scratch::new().unwrap();
        let file = scratch.directory.join("cache");
        let frame = Arc::new(Frame {
            width: 1,
            height: 1,
            pixels: vec![20, 30, 40, 255].into(),
            result: json!({"source_width":1,"source_height":1,"photo_width":1,"photo_height":1,"working_width":1,"working_height":1}),
        });
        let cache = Cache::new(file.clone());
        cache.save("identity".into(), frame.clone());
        cache.flush();
        assert_eq!(read(&file, "identity").unwrap().pixels, frame.pixels);
        assert!(read(&file, "changed").is_none());
        let mut bytes = fs::read(&file).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        fs::write(&file, bytes).unwrap();
        assert!(read(&file, "identity").is_none());
        cache.save("identity".into(), frame);
        cache.invalidate();
        cache.flush();
        assert!(!file.exists());
    }
}
