use crate::net::Http;
use sha1::{Digest, Sha1};
use sha2::Sha256;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hash {
    Sha1(String),
    Sha256(String),
}

#[derive(Clone, Debug)]
pub struct Job {
    pub url: String,
    pub path: PathBuf,
    pub hash: Option<Hash>,
    pub size: Option<u64>,
    pub executable: bool,
}

impl Job {
    pub fn new(url: impl Into<String>, path: PathBuf) -> Self {
        Self { url: url.into(), path, hash: None, size: None, executable: false }
    }

    pub fn sha1(mut self, sha1: Option<&str>) -> Self {
        self.hash = sha1.filter(|s| !s.is_empty()).map(|s| Hash::Sha1(s.to_ascii_lowercase()));
        self
    }

    pub fn size(mut self, size: Option<u64>) -> Self {
        self.size = size.filter(|s| *s > 0);
        self
    }
}

#[derive(Default)]
pub struct Progress {
    pub done_bytes: AtomicU64,
    pub total_bytes: AtomicU64,
    pub done_files: AtomicUsize,
    pub total_files: AtomicUsize,
    pub cancel: AtomicBool,
    pub current: Mutex<Vec<String>>,
}

impl Progress {
    pub fn reset(&self) {
        self.done_bytes.store(0, Ordering::Relaxed);
        self.total_bytes.store(0, Ordering::Relaxed);
        self.done_files.store(0, Ordering::Relaxed);
        self.total_files.store(0, Ordering::Relaxed);
        self.current.lock().unwrap().clear();
    }

    pub fn begin(&self, name: &str) {
        let mut cur = self.current.lock().unwrap();
        if !cur.iter().any(|n| n == name) {
            cur.push(name.to_string());
        }
    }

    pub fn end(&self, name: &str) {
        self.current.lock().unwrap().retain(|n| n != name);
    }

    pub fn current_label(&self) -> String {
        let cur = self.current.lock().unwrap();
        match cur.len() {
            0 => String::new(),
            1 => cur[0].clone(),
            n => format!("{} and {} more", cur[0], n - 1),
        }
    }

    pub fn fraction(&self) -> f32 {
        let total = self.total_bytes.load(Ordering::Relaxed);
        if total == 0 {
            let files = self.total_files.load(Ordering::Relaxed);
            if files == 0 {
                return 0.0;
            }
            return self.done_files.load(Ordering::Relaxed) as f32 / files as f32;
        }
        (self.done_bytes.load(Ordering::Relaxed) as f64 / total as f64).min(1.0) as f32
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 15) as usize] as char);
    }
    s
}

enum Hasher {
    Sha1(Sha1),
    Sha256(Sha256),
}

impl Hasher {
    fn for_hash(hash: &Hash) -> Self {
        match hash {
            Hash::Sha1(_) => Hasher::Sha1(Sha1::new()),
            Hash::Sha256(_) => Hasher::Sha256(Sha256::new()),
        }
    }

    fn update(&mut self, data: &[u8]) {
        match self {
            Hasher::Sha1(h) => h.update(data),
            Hasher::Sha256(h) => h.update(data),
        }
    }

    fn matches(self, hash: &Hash) -> bool {
        let got = match self {
            Hasher::Sha1(h) => hex(&h.finalize()),
            Hasher::Sha256(h) => hex(&h.finalize()),
        };
        match hash {
            Hash::Sha1(want) | Hash::Sha256(want) => got.eq_ignore_ascii_case(want),
        }
    }
}

pub fn file_matches(path: &Path, hash: &Hash) -> bool {
    let Ok(mut file) = File::open(path) else { return false };
    let mut hasher = Hasher::for_hash(hash);
    let mut buf = vec![0u8; 1 << 16];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(_) => return false,
        }
    }
    hasher.matches(hash)
}

fn present(job: &Job, verify: bool) -> bool {
    let Ok(meta) = std::fs::metadata(&job.path) else { return false };
    if !meta.is_file() {
        return false;
    }
    if let Some(size) = job.size {
        if meta.len() != size {
            return false;
        }
    }
    match (&job.hash, verify) {
        (Some(hash), true) => file_matches(&job.path, hash),
        _ => true,
    }
}

fn part_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".cxpart");
    path.with_file_name(name)
}

fn fetch(http: &Http, job: &Job, progress: &Progress) -> Result<(), String> {
    if let Some(dir) = job.path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let name = job.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    progress.begin(&name);
    let result = fetch_inner(http, job, progress);
    progress.end(&name);
    result
}

fn fetch_inner(http: &Http, job: &Job, progress: &Progress) -> Result<(), String> {
    let mut res = http.send(|a| a.get(&job.url).call()).map_err(|e| e.to_string())?;
    let status = res.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("server answered {status}"));
    }
    let part = part_path(&job.path);
    let file = File::create(&part).map_err(|e| format!("cannot write {}: {e}", part.display()))?;
    let mut out = BufWriter::with_capacity(1 << 16, file);
    let mut hasher = job.hash.as_ref().map(Hasher::for_hash);
    let mut reader = res.body_mut().as_reader();
    let mut buf = vec![0u8; 1 << 16];
    let mut written = 0u64;
    let result = (|| {
        loop {
            if progress.cancelled() {
                return Err("cancelled".to_string());
            }
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            if let Some(h) = hasher.as_mut() {
                h.update(&buf[..n]);
            }
            written += n as u64;
            progress.done_bytes.fetch_add(n as u64, Ordering::Relaxed);
        }
        out.flush().map_err(|e| e.to_string())?;
        drop(out);
        if let Some(size) = job.size {
            if written != size {
                return Err(format!("expected {size} bytes, got {written}"));
            }
        }
        if let (Some(h), Some(hash)) = (hasher.take(), job.hash.as_ref()) {
            if !h.matches(hash) {
                return Err("checksum does not match".to_string());
            }
        }
        #[cfg(unix)]
        if job.executable {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&part, &job.path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
        progress.done_bytes.fetch_sub(written.min(progress.done_bytes.load(Ordering::Relaxed)), Ordering::Relaxed);
    }
    result
}

fn fetch_with_retry(http: &Http, job: &Job, progress: &Progress) -> Result<(), String> {
    let mut last = String::new();
    for attempt in 0..4 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(400 * (1 << attempt)));
        }
        match fetch(http, job, progress) {
            Ok(()) => return Ok(()),
            Err(e) if e == "cancelled" => return Err(e),
            Err(e) => last = e,
        }
    }
    let name = job.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Err(format!("could not download {name}: {last}"))
}

pub fn run(http: &Http, jobs: Vec<Job>, threads: usize, progress: &Progress, verify: bool) -> Result<usize, String> {
    let mut seen = HashSet::new();
    let jobs: Vec<Job> = jobs.into_iter().filter(|j| seen.insert(j.path.clone())).collect();

    let checked = parallel(&jobs, threads.max(1), progress, |job| Ok(!present(job, verify)))?;
    let missing: Vec<&Job> = jobs.iter().zip(checked).filter(|(_, need)| *need).map(|(j, _)| j).collect();

    progress.total_files.fetch_add(missing.len(), Ordering::Relaxed);
    let unknown = missing.iter().filter(|j| j.size.is_none()).count() as u64;
    let bytes: u64 = missing.iter().map(|j| j.size.unwrap_or(0)).sum();
    progress.total_bytes.fetch_add(bytes + unknown * (256 << 10), Ordering::Relaxed);

    let count = missing.len();
    parallel(&missing, threads.max(1), progress, |job| {
        fetch_with_retry(http, job, progress)?;
        progress.done_files.fetch_add(1, Ordering::Relaxed);
        Ok(())
    })?;
    if unknown > 0 {
        let done = progress.done_bytes.load(Ordering::Relaxed);
        progress.total_bytes.store(done.max(1), Ordering::Relaxed);
    }
    Ok(count)
}

fn parallel<T: Sync, R: Send + Default + Clone>(
    items: &[T],
    threads: usize,
    progress: &Progress,
    work: impl Fn(&T) -> Result<R, String> + Sync,
) -> Result<Vec<R>, String> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let next = AtomicUsize::new(0);
    let failed: Mutex<Option<String>> = Mutex::new(None);
    let results: Mutex<Vec<R>> = Mutex::new(vec![R::default(); items.len()]);
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        for _ in 0..threads.min(items.len()) {
            scope.spawn(|| loop {
                if stop.load(Ordering::Relaxed) || progress.cancelled() {
                    return;
                }
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items.get(i) else { return };
                match work(item) {
                    Ok(r) => results.lock().unwrap()[i] = r,
                    Err(e) => {
                        stop.store(true, Ordering::Relaxed);
                        failed.lock().unwrap().get_or_insert(e);
                        return;
                    }
                }
            });
        }
    });
    if progress.cancelled() {
        return Err("cancelled".into());
    }
    if let Some(e) = failed.into_inner().unwrap() {
        return Err(e);
    }
    Ok(results.into_inner().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_hex() {
        let mut h = Sha1::new();
        h.update(b"abc");
        assert_eq!(hex(&h.finalize()), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn quick_check_uses_size() {
        let dir = std::env::temp_dir().join(format!("cxl-dl-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.bin");
        std::fs::write(&path, b"abc").unwrap();
        let ok = Job::new("x", path.clone()).sha1(Some("a9993e364706816aba3e25717850c26c9cd0d89d")).size(Some(3));
        assert!(present(&ok, false));
        assert!(present(&ok, true));
        let wrong = Job::new("x", path.clone()).sha1(Some("0000000000000000000000000000000000000000")).size(Some(3));
        assert!(present(&wrong, false));
        assert!(!present(&wrong, true));
        let size = Job::new("x", path).size(Some(4));
        assert!(!present(&size, false));
        let _ = std::fs::remove_dir_all(dir);
    }
}
