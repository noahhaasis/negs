//! Persistent serial engine protocol. Call blocking methods on a worker, never GTK.
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub type Result<T> = std::result::Result<T, String>;
const MAX_LINE: u64 = 4 * 1024 * 1024;
const STOPPED: &str = "The image engine stopped. Open the photograph again to retry.";

#[derive(Clone)]
pub struct Paths {
    pub helper: PathBuf,
    pub fast: PathBuf,
    pub reference: PathBuf,
    pub data: PathBuf,
}
impl Paths {
    pub fn locate() -> Result<Self> {
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let directory = executable.parent().unwrap_or(Path::new("."));
        let root = std::env::var_os("SPEKTRAFILM_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."));
        let find = |name: &str, variable: &str| {
            std::env::var_os(variable)
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    [
                        directory.join(name),
                        directory.join("../lib/negs").join(name),
                        root.join("target/release").join(name),
                    ]
                    .into_iter()
                    .find(|p| p.is_file())
                    .unwrap_or_else(|| directory.join(name))
                })
        };
        let data = std::env::var_os("SPEKTRAFILM_DATA_DIR")
            .or_else(|| std::env::var_os("NEGS_DATA_DIR"))
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                [
                    directory.join("../share/negs/data"),
                    directory.join("../data"),
                    root.join("data"),
                ]
                .into_iter()
                .find(|p| p.join("profiles").is_dir())
                .unwrap_or_else(|| root.join("data"))
            });
        Ok(Self {
            helper: find("spektrafilm-native", "SPEKTRAFILM_NATIVE_HELPER"),
            fast: find("spektrafilm", "SPEKTRAFILM_F32_CLI"),
            reference: find("spektrafilm-f64", "SPEKTRAFILM_F64_CLI"),
            data,
        })
    }
}

pub fn unique() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{:x}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

pub struct Scratch {
    pub directory: PathBuf,
    _lock: File,
}
impl Scratch {
    pub fn new() -> Result<Self> {
        unsafe extern "C" {
            fn geteuid() -> u32;
        }
        // SAFETY: geteuid is a read-only libc system call with no arguments.
        let user = unsafe { geteuid() };
        let root = std::env::var_os("XDG_RUNTIME_DIR")
            .map(|p| PathBuf::from(p).join("negs"))
            .unwrap_or_else(|| std::env::temp_dir().join(format!("negs-{user}")));
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&root)
            .map_err(|e| e.to_string())?;
        // Crash leftovers are app-owned launch directories with an unlocked file.
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten().take(1024) {
                let directory = entry.path();
                if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                if let Ok(lock) = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(directory.join("lock"))
                    && lock.try_lock().is_ok()
                {
                    // Staging paths are recorded before an export starts.
                    if let Ok(bytes) = fs::read(directory.join("exports.json"))
                        && bytes.len() < 1_048_576
                        && let Ok(paths) = serde_json::from_slice::<Vec<PathBuf>>(&bytes)
                    {
                        for path in paths {
                            if path
                                .file_name()
                                .is_some_and(|n| n.to_string_lossy().starts_with(".negs-"))
                            {
                                let _ = fs::remove_file(path);
                            }
                        }
                    }
                    let _ = fs::remove_dir_all(directory);
                }
            }
        }
        let directory = root.join(unique());
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .map_err(|e| e.to_string())?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(directory.join(".lock.pending"))
            .map_err(|e| e.to_string())?;
        lock.try_lock().map_err(|e| e.to_string())?;
        // Publish the lock only after acquiring it. Cleanup skips directories
        // without this name, so a concurrent constructor cannot mistake a
        // just-created, not-yet-locked file for an abandoned session.
        fs::rename(directory.join(".lock.pending"), directory.join("lock"))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            directory,
            _lock: lock,
        })
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

struct Connection {
    stdin: ChildStdin,
    lines: mpsc::Receiver<Result<String>>,
    // Linux PDEATHSIG follows the thread that forks, not just its process.
    // Keep a dedicated spawn thread alive for this persistent connection.
    _spawn_thread: mpsc::Sender<()>,
}
#[derive(Default)]
struct Worker {
    connection: Option<Connection>,
    sequence: u64,
}

pub struct Engine {
    pub paths: Paths,
    pub scratch: Scratch,
    worker: Mutex<Worker>,
    child: Mutex<Option<Child>>,
    stopped: AtomicBool,
}
impl Engine {
    pub fn new() -> Result<Self> {
        Self::with_paths(Paths::locate()?)
    }
    fn with_paths(paths: Paths) -> Result<Self> {
        Ok(Self {
            paths,
            scratch: Scratch::new()?,
            worker: Mutex::new(Worker::default()),
            child: Mutex::new(None),
            stopped: AtomicBool::new(false),
        })
    }
    fn connect(&self) -> Result<Connection> {
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.scratch.directory.join("engine.log"))
            .map_err(|e| e.to_string())?;
        let mut command = Command::new(&self.paths.helper);
        command
            .arg("--data-dir")
            .arg(&self.paths.data)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(log);
        owned_child(&mut command);
        let mut guard = self.child.lock().map_err(|e| e.to_string())?;
        if self.stopped.load(Ordering::Acquire) {
            return Err(STOPPED.into());
        }
        let (spawned, receiver) = mpsc::sync_channel(1);
        let (spawn_thread, lifetime) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            if spawned.send(command.spawn()).is_ok() {
                let _ = lifetime.recv();
            }
        });
        let mut child = receiver
            .recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("Cannot start {}: {e}", self.paths.helper.display()))?;
        let stdin = child.stdin.take().ok_or("Missing helper input")?;
        let stdout = child.stdout.take().ok_or("Missing helper output")?;
        *guard = Some(child);
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                let result = (&mut reader).take(MAX_LINE + 1).read_line(&mut line);
                match result {
                    Ok(0) => {
                        let _ = sender.send(Err(STOPPED.into()));
                        break;
                    }
                    Ok(_) if line.len() as u64 > MAX_LINE || !line.ends_with('\n') => {
                        let _ = sender.send(Err("Invalid engine response length".into()));
                        break;
                    }
                    Ok(_) => {
                        if sender.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(Err(error.to_string()));
                        break;
                    }
                }
            }
        });
        Ok(Connection {
            stdin,
            lines,
            _spawn_thread: spawn_thread,
        })
    }
    /// Serial request, bounded by timeout. An engine/protocol failure resets the
    /// connection; a valid engine error preserves it and its retained output.
    pub fn request(&self, command: Value, timeout: Duration) -> Result<(u64, Value)> {
        self.request_inner(command, timeout, None)
    }
    pub fn request_for_job(
        &self,
        command: Value,
        timeout: Duration,
        job: &Job,
    ) -> Result<(u64, Value)> {
        self.request_inner(command, timeout, Some(job))
    }
    fn request_inner(
        &self,
        mut command: Value,
        timeout: Duration,
        job: Option<&Job>,
    ) -> Result<(u64, Value)> {
        let cancelled = || job.is_some_and(Job::cancelled);
        let mut worker = loop {
            if cancelled() {
                return Err("Export cancelled".into());
            }
            match self.worker.try_lock() {
                Ok(worker) => break worker,
                Err(std::sync::TryLockError::Poisoned(error)) => return Err(error.to_string()),
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(20))
                }
            }
        };
        if cancelled() {
            return Err("Export cancelled".into());
        }
        if self.stopped.load(Ordering::Acquire) {
            return Err(STOPPED.into());
        }
        worker.sequence = worker
            .sequence
            .checked_add(1)
            .ok_or("Engine request ID overflow")?;
        let id = worker.sequence;
        command["id"] = json!(id);
        if worker.connection.is_none() {
            worker.connection = Some(self.connect()?);
        }
        let connection = worker.connection.as_mut().unwrap();
        let exchange = (|| {
            if cancelled() {
                return Err("Export cancelled".into());
            }
            serde_json::to_writer(&mut connection.stdin, &command).map_err(|e| e.to_string())?;
            connection
                .stdin
                .write_all(b"\n")
                .and_then(|_| connection.stdin.flush())
                .map_err(|e| e.to_string())?;
            let deadline = std::time::Instant::now() + timeout;
            let line = loop {
                if cancelled() {
                    return Err("Export cancelled".into());
                }
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return Err("The image engine did not respond in time.".into());
                }
                match connection
                    .lines
                    .recv_timeout(remaining.min(Duration::from_millis(50)))
                {
                    Ok(line) => break line?,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return Err(STOPPED.into()),
                }
            };
            let reply: Value =
                serde_json::from_str(&line).map_err(|e| format!("Invalid engine response: {e}"))?;
            if reply.get("id").and_then(Value::as_u64) != Some(id) {
                return Err("The image engine returned an unexpected request ID.".into());
            }
            if reply.get("result").is_none() && reply.get("error").and_then(Value::as_str).is_none()
            {
                return Err("The image engine returned an incomplete response.".into());
            }
            Ok(reply)
        })();
        match exchange {
            Ok(reply) => {
                if let Some(error) = reply.get("error").and_then(Value::as_str) {
                    Err(error.into())
                } else {
                    Ok((id, reply["result"].clone()))
                }
            }
            Err(error) => {
                worker.connection.take();
                self.kill();
                Err(error)
            }
        }
    }
    fn kill(&self) {
        if let Ok(mut guard) = self.child.lock()
            && let Some(mut child) = guard.take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        if let Ok(mut guard) = self.child.lock()
            && let Some(mut child) = guard.take()
        {
            let _ = child.kill();
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Close-on-parent-death protects against a GUI crash during long computation.
/// Only libc syscalls run between fork and exec; no allocation or locks.
pub fn owned_child(command: &mut Command) {
    unsafe extern "C" {
        fn prctl(option: i32, ...) -> i32;
        fn getppid() -> i32;
    }
    let parent = std::process::id() as i32;
    command.process_group(0);
    // SAFETY: prctl/getppid are thin system-call wrappers; the closure does not
    // access shared Rust state. Check the fork/prctl race before exec.
    unsafe {
        command.pre_exec(move || {
            if prctl(1, 9, 0, 0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if getppid() != parent {
                return Err(std::io::Error::other("Parent exited"));
            }
            Ok(())
        });
    }
}

pub struct Frame {
    pub width: i32,
    pub height: i32,
    pub pixels: std::sync::Arc<[u8]>,
    pub result: Value,
}
impl Frame {
    pub fn read(path: &Path, result: Value) -> Result<Self> {
        let side = |key| {
            result
                .get(key)
                .and_then(Value::as_u64)
                .filter(|n| (1..=8192).contains(n))
                .ok_or_else(|| "Invalid preview dimensions".to_string())
        };
        let (width, height) = (side("width")?, side("height")?);
        let expected = width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(4))
            .ok_or("Preview size overflow")?;
        if fs::metadata(path).map_err(|e| e.to_string())?.len() != expected {
            return Err("The engine returned an invalid preview buffer.".into());
        }
        let pixels = fs::read(path).map_err(|e| e.to_string())?;
        if pixels.len() as u64 != expected {
            return Err("The engine preview buffer changed while reading.".into());
        }
        Ok(Self {
            width: width as i32,
            height: height as i32,
            pixels: pixels.into(),
            result,
        })
    }
}

pub fn same_file(source: &Path, destination: &Path) -> bool {
    match (fs::metadata(source), fs::metadata(destination)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}
pub fn tail(path: &Path) -> String {
    use std::io::{Seek, SeekFrom};
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let _ = file
        .seek(SeekFrom::End(-4000))
        .or_else(|_| file.seek(SeekFrom::Start(0)));
    let mut bytes = Vec::new();
    let _ = file.take(4000).read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Atomic publication/cancellation state: cancellation wins before commit;
/// after commit begins the operation finishes and reports its actual result.
pub struct Job {
    phase: std::sync::atomic::AtomicU8,
    child: Mutex<Option<Child>>,
    pub using_helper: AtomicBool,
}
impl Job {
    pub fn new() -> Self {
        Self {
            phase: std::sync::atomic::AtomicU8::new(0),
            child: Mutex::new(None),
            using_helper: AtomicBool::new(false),
        }
    }
    pub fn cancel(&self) {
        if self
            .phase
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            && let Ok(mut child) = self.child.lock()
            && let Some(child) = child.as_mut()
        {
            let _ = child.kill();
        }
    }
    pub fn cancelled(&self) -> bool {
        self.phase.load(Ordering::Acquire) == 1
    }
    pub fn run(&self, command: &mut Command, log: &Path, timeout: Duration) -> Result<()> {
        if self.cancelled() {
            return Err("Export cancelled".into());
        }
        let output = File::create(log).map_err(|e| e.to_string())?;
        let error = output.try_clone().map_err(|e| e.to_string())?;
        command.stdin(Stdio::null()).stdout(output).stderr(error);
        owned_child(command);
        *self.child.lock().map_err(|e| e.to_string())? =
            Some(command.spawn().map_err(|e| e.to_string())?);
        let started = std::time::Instant::now();
        loop {
            let mut child = self.child.lock().map_err(|e| e.to_string())?;
            if let Some(status) = child
                .as_mut()
                .ok_or("Exporter stopped")?
                .try_wait()
                .map_err(|e| e.to_string())?
            {
                child.take();
                if self.cancelled() {
                    return Err("Export cancelled".into());
                }
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("Exporter exited with {status}\n{}", tail(log)))
                };
            }
            if self.cancelled() || started.elapsed() > timeout {
                if let Some(mut child) = child.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return Err(if self.cancelled() {
                    "Export cancelled".into()
                } else {
                    "Export timed out".into()
                });
            }
            drop(child);
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    pub fn publish(&self, stage: &Path, output: &Path, source: Option<&Path>) -> Result<()> {
        if self
            .phase
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("Export cancelled".into());
        }
        if source.is_some_and(|p| same_file(p, output)) {
            return Err("Choose a different filename to preserve the original photograph.".into());
        }
        if !stage.is_file() || fs::metadata(stage).map_err(|e| e.to_string())?.len() == 0 {
            return Err("The exporter produced no file.".into());
        }
        fs::rename(stage, output)
            .map_err(|e| format!("Could not finish saving {}: {e}", output.display()))?;
        self.phase.store(3, Ordering::Release);
        Ok(())
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        if let Ok(child) = self.child.get_mut()
            && let Some(mut child) = child.take()
        {
            let _ = child.kill();
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_stops_and_reaps_a_running_exporter() {
        let scratch = Scratch::new().unwrap();
        let log = scratch.directory.join("child.log");
        let job = std::sync::Arc::new(Job::new());
        let running = job.clone();
        let thread = std::thread::spawn(move || {
            let mut command = Command::new("python3");
            command.args(["-c", "import time; time.sleep(30)"]);
            running.run(&mut command, &log, Duration::from_secs(5))
        });
        for _ in 0..1000 {
            if job.child.lock().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(job.child.lock().unwrap().is_some());
        job.cancel();
        assert_eq!(thread.join().unwrap().unwrap_err(), "Export cancelled");
        assert!(job.child.lock().unwrap().is_none());
    }
    #[test]
    fn preview_rejects_malformed_dimensions_and_payloads() {
        let scratch = Scratch::new().unwrap();
        let path = scratch.directory.join("preview.rgba");
        fs::write(&path, [20, 30, 40, 255]).unwrap();
        assert!(Frame::read(&path, json!({"width":1,"height":1})).is_ok());
        for result in [
            json!({"width":0,"height":1}),
            json!({"width":8193,"height":1}),
            json!({"width":2,"height":1}),
            json!({"width":-1,"height":1}),
        ] {
            assert!(Frame::read(&path, result).is_err());
        }
    }
    #[test]
    fn source_protection_follows_hard_links() {
        let scratch = Scratch::new().unwrap();
        let source = scratch.directory.join("source");
        let alias = scratch.directory.join("alias");
        fs::write(&source, [1]).unwrap();
        fs::hard_link(&source, &alias).unwrap();
        assert!(same_file(&source, &alias));
        assert!(!same_file(&source, &scratch.directory.join("different")));
    }
    #[test]
    fn concurrent_scratch_creation_keeps_live_directories() {
        let barrier = std::sync::Barrier::new(16);
        let scratches = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..16)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        Scratch::new().unwrap()
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert!(
            scratches
                .iter()
                .all(|scratch| scratch.directory.join("lock").is_file())
        );
    }

    #[test]
    fn helper_survives_the_thread_that_submitted_its_first_request() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new().unwrap();
        let helper = scratch.directory.join("mock-helper");
        fs::write(&helper, "#!/usr/bin/python3\nimport json,sys\nfor line in sys.stdin:\n r=json.loads(line)\n print(json.dumps({'id':r['id'],'result':{'protocol':1}}),flush=True)\n").unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
        let engine = Engine::with_paths(Paths {
            helper: helper.clone(),
            fast: helper.clone(),
            reference: helper,
            data: scratch.directory.clone(),
        })
        .unwrap();
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    engine
                        .request(json!({"command":"catalog"}), Duration::from_secs(2))
                        .unwrap()
                })
                .join()
                .unwrap();
        });
        std::thread::sleep(Duration::from_millis(30));
        assert!(
            engine
                .request(json!({"command":"catalog"}), Duration::from_secs(2))
                .is_ok()
        );
    }
    #[test]
    fn cancelled_queued_request_never_connects_or_interrupts_the_worker() {
        let engine = Engine::new().unwrap();
        let lock = engine.worker.lock().unwrap();
        let job = Job::new();
        std::thread::scope(|scope| {
            let queued = scope.spawn(|| {
                engine.request_for_job(json!({"command":"catalog"}), Duration::from_secs(1), &job)
            });
            job.cancel();
            assert_eq!(queued.join().unwrap().unwrap_err(), "Export cancelled");
        });
        assert!(engine.child.lock().unwrap().is_none());
        assert_eq!(lock.sequence, 0);
    }
    #[test]
    fn serial_protocol_restarts_after_eof_bad_id_and_timeout() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new().unwrap();
        let helper = scratch.directory.join("mock-helper");
        fs::write(
            &helper,
            r#"#!/usr/bin/python3
import json, sys, time
for line in sys.stdin:
    r = json.loads(line)
    c = r['command']
    if c == 'eof': sys.exit(3)
    if c == 'slow': time.sleep(2)
    reply = {'id': r['id'] + (1 if c == 'bad_id' else 0)}
    reply.update({'error': 'expected error'} if c == 'error' else {'result': {'protocol': 1}})
    print(json.dumps(reply), flush=True)
"#,
        )
        .unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
        let engine = Engine::with_paths(Paths {
            helper: helper.clone(),
            fast: helper.clone(),
            reference: helper,
            data: scratch.directory.clone(),
        })
        .unwrap();
        for command in ["error", "eof", "bad_id", "slow"] {
            assert!(
                engine
                    .request(json!({"command":command}), Duration::from_millis(150))
                    .is_err()
            );
            assert_eq!(
                engine
                    .request(json!({"command":"catalog"}), Duration::from_secs(3))
                    .unwrap()
                    .1["protocol"],
                1
            );
        }
        engine.stop();
        assert!(
            engine
                .request(json!({"command":"catalog"}), Duration::from_secs(1))
                .is_err()
        );
    }
}
