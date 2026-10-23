//! The engine's processes: `spektrafilm-native` for previews and Save
//! Preview, `spektrafilm-f64` / `spektrafilm` for exports. Mirrors
//! `Engine.swift`. Every process joins the app's job object, so Windows kills
//! them if Negs dies; pipe I/O runs on worker threads, never the UI thread.

use std::io::{BufRead, BufReader, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::platform::{self, Job};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
pub const STOPPED: &str = "The image engine stopped unexpectedly. Open the image again to retry.";

#[derive(Clone, Debug)]
pub struct EnginePaths {
    pub helper: PathBuf,
    /// The reference exporter, `spektrafilm-f64`.
    pub exporter: PathBuf,
    pub data: PathBuf,
    /// The f32 command-line tool `spektrafilm`, for Fast (GPU) export. The
    /// app still starts without it; only a fast export then fails.
    pub fast_exporter: Option<PathBuf>,
}

impl EnginePaths {
    /// Each tool comes from its environment override, else next to the app's
    /// executable, else `target\release` under `SPEKTRAFILM_ROOT` (default:
    /// the working directory, then the checkout the app was built in).
    pub fn locate() -> Result<Self> {
        let exe_dir = std::env::current_exe()?.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut roots: Vec<PathBuf> = vec![];
        if let Some(root) = std::env::var_os("SPEKTRAFILM_ROOT") {
            roots.push(root.into());
        } else {
            roots.push(std::env::current_dir()?);
            // windows\target\release\Negs.exe in a checkout.
            if let Some(checkout) = exe_dir.ancestors().find(|d| d.join("data").join("profiles").is_dir()) {
                roots.push(checkout.to_path_buf());
            }
        }
        let find = |name: &str, override_var: &str, executable: bool| -> Result<PathBuf> {
            if let Some(path) = std::env::var_os(override_var) {
                return Ok(path.into());
            }
            let mut candidates = vec![exe_dir.join(name)];
            for root in &roots {
                candidates.push(if executable { root.join("target").join("release").join(name) } else { root.join(name) });
            }
            candidates.into_iter().find(|p| p.exists()).ok_or_else(|| {
                anyhow!("Missing {override_var}. Install Negs again, or set SPEKTRAFILM_ROOT to your checkout.")
            })
        };
        Ok(Self {
            helper: find("spektrafilm-native.exe", "SPEKTRAFILM_NATIVE_HELPER", true)?,
            exporter: find("spektrafilm-f64.exe", "SPEKTRAFILM_F64_CLI", true)?,
            data: find("data", "SPEKTRAFILM_DATA_DIR", false)?,
            fast_exporter: find("spektrafilm.exe", "SPEKTRAFILM_F32_CLI", true).ok(),
        })
    }
}

fn unique_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:x}-{:x}-{:x}", std::process::id(), nanos, COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Temporary files live in per-launch folders under `%TEMP%\Negs`, each beside
/// a lock file held open (and not shared for writing or deletion) while the
/// folder is in use. A folder whose lock opens was left by a crash; making a
/// new folder removes those, with the export staging files they recorded.
/// The job object already killed their processes.
pub struct ScratchSpace {
    pub directory: PathBuf,
    lock_path: PathBuf,
    lock: Mutex<Option<std::fs::File>>,
}

impl ScratchSpace {
    pub fn new() -> Result<Self> {
        let root = std::env::temp_dir().join("Negs");
        Self::in_root(&root)
    }

    pub fn in_root(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root).context("Could not create temporary files")?;
        Self::sweep(root);
        let id = unique_id();
        let lock_path = root.join(format!("{id}.lock"));
        let lock = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(1) // FILE_SHARE_READ only
            .open(&lock_path)
            .context("Could not create temporary files")?;
        let directory = root.join(&id);
        std::fs::create_dir(&directory).context("Could not create temporary files")?;
        Ok(Self { directory, lock_path, lock: Mutex::new(Some(lock)) })
    }

    /// Deletes the folder. The engine calls this when it stops.
    pub fn remove(&self) {
        if self.lock.lock().unwrap().take().is_some() {
            let _ = std::fs::remove_dir_all(&self.directory);
            let _ = std::fs::remove_file(&self.lock_path);
        }
    }

    fn sweep(root: &Path) {
        let Ok(entries) = std::fs::read_dir(root) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("lock") {
                continue;
            }
            // Opening without sharing fails while its launch still holds it.
            let Ok(file) = std::fs::OpenOptions::new().write(true).share_mode(0).open(&path) else { continue };
            let folder = path.with_extension("");
            ExportRecord::abandon(&folder);
            let _ = std::fs::remove_dir_all(&folder);
            drop(file);
            let _ = std::fs::remove_file(&path);
        }
    }
}

impl Drop for ScratchSpace {
    fn drop(&mut self) {
        self.remove();
    }
}

/// What an export puts outside its scratch folder: the staging file beside
/// the destination. Recorded so a crash can be cleaned up.
#[derive(Serialize, Deserialize)]
struct ExportRecord {
    staging: PathBuf,
}

impl ExportRecord {
    const FILE: &'static str = "export.json";

    fn abandon(folder: &Path) {
        let Ok(jobs) = std::fs::read_dir(folder) else { return };
        for job in jobs.flatten() {
            let Ok(data) = std::fs::read(job.path().join(Self::FILE)) else { continue };
            let Ok(record) = serde_json::from_slice::<ExportRecord>(&data) else { continue };
            if record.staging.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(".negs-")) {
                let _ = std::fs::remove_file(&record.staging);
            }
        }
    }
}

type Reply = Box<dyn FnOnce(Result<(u64, Value)>) + Send>;

enum Message {
    Request { command: Value, timeout: Duration, reply: Reply },
    Stop,
}

struct Helper {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

/// The helper's JSON-lines protocol on a worker thread. One request at a
/// time, in order; each has a timeout, and a helper that dies or hangs is
/// replaced by the next request.
pub struct EngineClient {
    pub paths: EnginePaths,
    pub scratch: Arc<ScratchSpace>,
    sender: Sender<Message>,
    child: Arc<Mutex<Option<u32>>>,
    stopped: Arc<AtomicBool>,
}

impl EngineClient {
    pub fn start(paths: EnginePaths) -> Result<Self> {
        let scratch = Arc::new(ScratchSpace::new()?);
        let (sender, receiver) = mpsc::channel::<Message>();
        let child = Arc::new(Mutex::new(None));
        let stopped = Arc::new(AtomicBool::new(false));
        let worker = Worker { paths: paths.clone(), scratch: scratch.clone(), helper: None, sequence: 0, pid: child.clone() };
        std::thread::Builder::new().name("engine".into()).spawn(move || worker.run(receiver))?;
        Ok(Self { paths, scratch, sender, child, stopped })
    }

    /// Sends `command`; `reply` runs on the worker thread with the request's
    /// id and result.
    pub fn request(&self, command: Value, timeout: Duration, reply: impl FnOnce(Result<(u64, Value)>) + Send + 'static) {
        if self.stopped.load(Ordering::Relaxed) {
            return;
        }
        let _ = self.sender.send(Message::Request { command, timeout, reply: Box::new(reply) });
    }

    pub fn directory(&self) -> &Path {
        &self.scratch.directory
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(pid) = *self.child.lock().unwrap() {
            kill_pid(pid);
        }
        let _ = self.sender.send(Message::Stop);
    }
}

fn kill_pid(pid: u32) {
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
    unsafe {
        if let Ok(process) = OpenProcess(PROCESS_TERMINATE, false, pid) {
            let _ = TerminateProcess(process, 1);
            let _ = windows::Win32::Foundation::CloseHandle(process);
        }
    }
}

struct Worker {
    paths: EnginePaths,
    scratch: Arc<ScratchSpace>,
    helper: Option<Helper>,
    sequence: u64,
    pid: Arc<Mutex<Option<u32>>>,
}

impl Worker {
    fn run(mut self, receiver: Receiver<Message>) {
        while let Ok(message) = receiver.recv() {
            match message {
                Message::Request { command, timeout, reply } => {
                    let result = self.exchange(command, timeout);
                    reply(result);
                }
                Message::Stop => break,
            }
        }
        self.close();
        self.scratch.remove();
    }

    fn start(&mut self) -> Result<&mut Helper> {
        if self.helper.as_mut().is_some_and(|h| matches!(h.child.try_wait(), Ok(None))) {
            return Ok(self.helper.as_mut().unwrap());
        }
        self.close();
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.scratch.directory.join("engine.log"))
            .context("Could not create the engine log")?;
        let mut child = Command::new(&self.paths.helper)
            .arg("--data-dir")
            .arg(&self.paths.data)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .with_context(|| format!("Could not start the image engine ({})", self.paths.helper.display()))?;
        Job::get().assign(&child);
        *self.pid.lock().unwrap() = Some(child.id());
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        std::thread::Builder::new().name("engine-reader".into()).spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        })?;
        Ok(self.helper.insert(Helper { child, stdin, lines }))
    }

    fn close(&mut self) {
        if let Some(mut helper) = self.helper.take() {
            let _ = helper.child.kill();
            let _ = helper.child.wait();
        }
        *self.pid.lock().unwrap() = None;
    }

    fn exchange(&mut self, mut command: Value, timeout: Duration) -> Result<(u64, Value)> {
        self.sequence += 1;
        let id = self.sequence;
        command["id"] = json!(id);
        let mut line = serde_json::to_vec(&command)?;
        line.push(b'\n');
        let helper = self.start()?;
        let reply = (|| -> Result<String> {
            helper.stdin.write_all(&line).map_err(|_| anyhow!(STOPPED))?;
            helper.stdin.flush().map_err(|_| anyhow!(STOPPED))?;
            match helper.lines.recv_timeout(timeout) {
                Ok(line) => Ok(line),
                Err(RecvTimeoutError::Disconnected) => bail!(STOPPED),
                Err(RecvTimeoutError::Timeout) => bail!(
                    "The image engine stopped responding after {} s and was restarted. The graphics driver may have reset; open the image again to retry.",
                    timeout.as_secs()
                ),
            }
        })();
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => {
                self.close();
                return Err(error);
            }
        };
        let reply: Value = serde_json::from_str(&reply).context("The image engine sent an invalid response.")?;
        if reply["id"].as_u64() != Some(id) {
            self.close();
            bail!("Image engine response is out of sequence.");
        }
        if let Some(error) = reply["error"].as_str().filter(|e| !e.is_empty()) {
            bail!("{error}");
        }
        Ok((id, reply["result"].clone()))
    }
}

/// Which engine renders an export. Both use the same parameters, crop and border.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportEngine {
    /// `spektrafilm-f64` on the CPU, the reference implementation.
    Reference,
    /// `spektrafilm` (32-bit) on the GPU through wgpu/Vulkan, like the preview.
    Fast,
}

impl ExportEngine {
    pub const ALL: [ExportEngine; 2] = [ExportEngine::Reference, ExportEngine::Fast];

    pub fn title(self) -> &'static str {
        match self {
            Self::Reference => "Reference (CPU, 64-bit)",
            Self::Fast => "Fast (GPU)",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Reference => "reference",
            Self::Fast => "fast",
        }
    }

    pub fn from_key(key: &str) -> Self {
        if key == "reference" { Self::Reference } else { Self::Fast }
    }

    /// `SPEKTRAFILM_BACKEND` for the exporter process.
    pub fn backend(self) -> &'static str {
        match self {
            Self::Reference => "cpu",
            Self::Fast => "wgpu",
        }
    }

    pub fn executable(self, paths: &EnginePaths) -> Result<PathBuf> {
        match self {
            Self::Reference => Ok(paths.exporter.clone()),
            Self::Fast => paths.fast_exporter.clone().ok_or_else(|| {
                anyhow!("Fast (GPU) export needs spektrafilm.exe next to Negs (or SPEKTRAFILM_F32_CLI). Export with Reference (CPU, 64-bit) instead, or install Negs again.")
            }),
        }
    }

    /// The exporter prints the backend that actually rendered, after the
    /// render. A fast export that didn't fit the GPU reports `CPU (rayon)`.
    pub fn ran_on_gpu_log(log: &str) -> bool {
        log.contains("Backend: wgpu")
    }

    /// Whether the helper's reported backend is the wgpu GPU backend.
    pub fn ran_on_gpu(backend: &str) -> bool {
        backend.starts_with("wgpu")
    }
}

/// An export's request, everything the exporter's command line needs.
pub struct ExportRequest {
    pub executable: PathBuf,
    pub engine: ExportEngine,
    pub data: PathBuf,
    pub scratch: PathBuf,
    pub source: PathBuf,
    pub destination: PathBuf,
    pub film: String,
    pub paper: String,
    pub params: Value,
    pub scan_film: bool,
    pub crop: Option<crate::crop::CropSettings>,
    pub border: Option<crate::border::BorderSettings>,
    pub size_arguments: Vec<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct ExportOutcome {
    /// A fast export ran on the CPU because the GPU couldn't take it.
    pub fell_back_to_cpu: bool,
}

/// A running export. It writes to a hidden sibling of the destination and
/// renames it into place on success; cancel or failure leave an existing
/// destination untouched.
#[derive(Clone, Default)]
pub struct ExportHandle {
    child: Arc<Mutex<Option<u32>>>,
    cancelled: Arc<AtomicBool>,
}

impl ExportHandle {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Some(pid) = *self.child.lock().unwrap() {
            kill_pid(pid);
        }
    }

    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("Export cancelled")
    }
}
impl std::fmt::Debug for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("Cancelled")
    }
}
impl std::error::Error for Cancelled {}

pub fn staging_path(destination: &Path) -> PathBuf {
    let ext = destination.extension().and_then(|e| e.to_str()).unwrap_or("png");
    destination.with_file_name(format!(".negs-{}.{ext}", unique_id()))
}

/// Runs the exporter to completion on the calling (worker) thread.
pub fn run_export(request: &ExportRequest, handle: &ExportHandle) -> Result<ExportOutcome> {
    let work = request.scratch.join(format!("export-{}", unique_id()));
    std::fs::create_dir_all(&work)?;
    let staging = staging_path(&request.destination);
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&work);
        let _ = std::fs::remove_file(&staging);
    };
    let result = (|| -> Result<ExportOutcome> {
        std::fs::write(work.join(ExportRecord::FILE), serde_json::to_vec(&ExportRecord { staging: staging.clone() })?)?;
        let params = work.join("params.json");
        std::fs::write(&params, serde_json::to_vec(&request.params)?)?;
        let log_path = work.join("export.log");
        let log = std::fs::File::create(&log_path)?;
        let mut command = Command::new(&request.executable);
        command
            .arg("process")
            .arg(&request.source)
            .arg("-o")
            .arg(&staging)
            .args(["--film", &request.film, "--paper", &request.paper])
            .arg("--params")
            .arg(&params)
            .arg("--data-dir")
            .arg(&request.data);
        if request.scan_film {
            command.arg("--scan-film");
        }
        if let Some(crop) = request.crop {
            let r = crop.rect;
            command.args(["--crop".to_string(), format!("{},{},{},{},{}", r.x, r.y, r.width, r.height, crop.angle)]);
        }
        if let Some(border) = request.border {
            command.args(border.arguments());
        }
        command.args(&request.size_arguments);
        command
            .env("SPEKTRAFILM_BACKEND", request.engine.backend())
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .creation_flags(CREATE_NO_WINDOW);
        if handle.cancelled() {
            return Err(Cancelled.into());
        }
        let started = SystemTime::now();
        let mut child = command.spawn().with_context(|| format!("Could not start {}", request.executable.display()))?;
        Job::get().assign(&child);
        *handle.child.lock().unwrap() = Some(child.id());
        let status = child.wait()?;
        *handle.child.lock().unwrap() = None;
        if handle.cancelled() {
            return Err(Cancelled.into());
        }
        let diagnostic = std::fs::read_to_string(&log_path).unwrap_or_default();
        if !status.success() {
            let hint = if request.engine == ExportEngine::Fast {
                " If the GPU ran out of memory, export with Reference (CPU, 64-bit) instead."
            } else {
                ""
            };
            let tail: String = if diagnostic.is_empty() {
                "Unknown exporter error".into()
            } else {
                let chars: Vec<char> = diagnostic.chars().collect();
                chars[chars.len().saturating_sub(4000)..].iter().collect()
            };
            bail!("Export failed.{hint}\n{tail}");
        }
        let gpu = ExportEngine::ran_on_gpu_log(&diagnostic);
        // Exit code 0 is not proof: a driver reset during the render can
        // still leave an image of unknown validity.
        if request.engine == ExportEngine::Fast && gpu && crate::platform_events::display_reset_since(started) {
            bail!("The graphics driver reset during the export, so the file may be damaged and was not saved. Export again, or with Reference (CPU, 64-bit).");
        }
        platform::replace_file(&staging, &request.destination)?;
        Ok(ExportOutcome { fell_back_to_cpu: request.engine == ExportEngine::Fast && !gpu })
    })();
    cleanup();
    result
}

/// How long a request may take before the helper counts as hung. Renders of
/// very large photographs on the CPU take minutes.
pub fn timeout_for(command: &str) -> Duration {
    match command {
        "catalog" => Duration::from_secs(60),
        _ => Duration::from_secs(20 * 60),
    }
}

pub fn elapsed_ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scratch_folders_of_crashed_launches_are_swept() {
        let root = std::env::temp_dir().join(format!("negs-scratch-{}", unique_id()));
        let live = ScratchSpace::in_root(&root).unwrap();
        // A crashed launch: its lock file exists but nobody holds it.
        let crashed = root.join("crashed");
        std::fs::create_dir_all(crashed.join("export-1")).unwrap();
        let staging = root.join(".negs-abandoned.png");
        std::fs::write(&staging, b"partial").unwrap();
        std::fs::write(crashed.join("export-1").join("export.json"), serde_json::to_vec(&ExportRecord { staging: staging.clone() }).unwrap()).unwrap();
        std::fs::write(root.join("crashed.lock"), b"").unwrap();
        let second = ScratchSpace::in_root(&root).unwrap();
        assert!(!crashed.exists() && !staging.exists(), "the crashed launch's files are gone");
        assert!(live.directory.exists(), "a running launch's folder stays");
        second.remove();
        assert!(!second.directory.exists());
        drop(live);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn engines_report_the_backend_that_rendered() {
        assert!(ExportEngine::ran_on_gpu_log("…\nBackend: wgpu (Vulkan, NVIDIA)\n"));
        assert!(!ExportEngine::ran_on_gpu_log("Backend: CPU (rayon)"));
        assert!(ExportEngine::ran_on_gpu("wgpu (Vulkan)") && !ExportEngine::ran_on_gpu("CPU (rayon)"));
        assert_eq!(ExportEngine::from_key("reference"), ExportEngine::Reference);
        assert_eq!(ExportEngine::from_key("anything"), ExportEngine::Fast);
    }
}
