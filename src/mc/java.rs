use super::meta::{cached_json, pinned_json};
use super::Endpoints;
use crate::download::Job;
use crate::net::Http;
use crate::paths::Paths;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};

const MARKER: &str = ".cxlauncher-version";

#[derive(Debug, Deserialize)]
struct Pointer {
    sha1: Option<String>,
    url: String,
}

#[derive(Debug, Deserialize)]
struct Named {
    name: String,
}

#[derive(Debug, Deserialize)]
struct Entry {
    manifest: Pointer,
    version: Named,
}

#[derive(Debug, Deserialize)]
struct Raw {
    sha1: Option<String>,
    size: Option<u64>,
    url: String,
}

#[derive(Debug, Deserialize)]
struct FileDownloads {
    raw: Raw,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Item {
    File {
        #[serde(default)]
        executable: bool,
        downloads: FileDownloads,
    },
    Directory,
    Link {
        target: String,
    },
}

#[derive(Debug, Deserialize)]
struct Listing {
    files: BTreeMap<String, Item>,
}

#[derive(Debug)]
pub struct Runtime {
    pub dir: PathBuf,
    pub java: PathBuf,
    pub version: String,
    pub jobs: Vec<Job>,
    links: Vec<(PathBuf, String)>,
}

pub fn platform() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("windows", "aarch64") => "windows-arm64",
        ("windows", "x86") => "windows-x86",
        ("linux", "x86_64") => "linux",
        ("linux", "x86") => "linux-i386",
        ("macos", "aarch64") => "mac-os-arm64",
        ("macos", _) => "mac-os",
        _ => return None,
    })
}

pub fn executable(home: &Path) -> PathBuf {
    let mac = home.join("jre.bundle/Contents/Home/bin/java");
    if cfg!(target_os = "macos") && mac.exists() {
        return mac;
    }
    if cfg!(windows) {
        home.join("bin").join("javaw.exe")
    } else {
        home.join("bin").join("java")
    }
}

fn safe(rel: &str) -> bool {
    !rel.is_empty() && Path::new(rel).components().all(|c| matches!(c, Component::Normal(_)))
}

pub fn plan(http: &Http, ep: &Endpoints, paths: &Paths, component: &str) -> Result<Runtime, String> {
    let platform = platform().ok_or("Mojang has no Java for this system. Pick an installed Java in Settings.")?;
    let index: HashMap<String, HashMap<String, Vec<Entry>>> = cached_json(http, &ep.java_runtimes, &paths.meta().join("java-runtimes.json"))?;
    let entry = index
        .get(platform)
        .and_then(|c| c.get(component))
        .and_then(|v| v.first())
        .ok_or(format!("Mojang has no {component} for {platform}. Pick an installed Java in Settings."))?;
    let sha1 = entry.manifest.sha1.as_deref().unwrap_or("latest");
    let listing: Listing = pinned_json(http, &entry.manifest.url, entry.manifest.sha1.as_deref(), &paths.meta().join("runtime").join(format!("{component}-{sha1}.json")))?;

    let dir = paths.runtime(component);
    let mut jobs = Vec::new();
    let mut links = Vec::new();
    for (rel, item) in listing.files {
        if !safe(&rel) {
            continue;
        }
        match item {
            Item::File { executable, downloads } => {
                let mut job = Job::new(downloads.raw.url, dir.join(&rel)).sha1(downloads.raw.sha1.as_deref()).size(downloads.raw.size);
                job.executable = executable;
                jobs.push(job);
            }
            Item::Directory => {}
            Item::Link { target } => links.push((dir.join(&rel), target)),
        }
    }
    Ok(Runtime { java: executable(&dir), dir, version: entry.version.name.clone(), jobs, links })
}

pub fn finish(rt: &Runtime) -> Result<(), String> {
    for (link, target) in &rt.links {
        if std::fs::symlink_metadata(link).is_ok() {
            continue;
        }
        let Some(parent) = link.parent() else { continue };
        let _ = std::fs::create_dir_all(parent);
        #[cfg(unix)]
        let _ = std::os::unix::fs::symlink(target, link);
        #[cfg(not(unix))]
        let _ = std::fs::copy(parent.join(target), link);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&rt.java) {
            if meta.permissions().mode() & 0o111 == 0 {
                let _ = std::fs::set_permissions(&rt.java, std::fs::Permissions::from_mode(0o755));
            }
        }
    }
    if !rt.java.exists() {
        return Err(format!("the Java download is incomplete ({} is missing)", rt.java.display()));
    }
    let _ = std::fs::write(rt.dir.join(MARKER), &rt.version);
    Ok(())
}

pub fn parse_major(version: &str) -> Option<u32> {
    let version = version.trim().trim_matches('"');
    let mut parts = version.split(|c: char| !c.is_ascii_digit());
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        return parts.next()?.parse().ok();
    }
    Some(first)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub java: PathBuf,
    pub version: String,
    pub major: u32,
}

fn read_release(home: &Path) -> Option<String> {
    let text = std::fs::read_to_string(home.join("release")).ok()?;
    text.lines().find_map(|l| l.strip_prefix("JAVA_VERSION=")).map(|v| v.trim().trim_matches('"').to_string())
}

pub fn probe(exe: &Path) -> Option<Installed> {
    if !exe.is_file() {
        return None;
    }
    let home = exe.parent()?.parent()?;
    let version = read_release(home).or_else(|| {
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("-version");
        hide_console(&mut cmd);
        let out = cmd.output().ok()?;
        let text = String::from_utf8_lossy(&out.stderr).into_owned();
        let quoted = text.split('"').nth(1)?;
        Some(quoted.to_string())
    })?;
    Some(Installed { java: exe.to_path_buf(), major: parse_major(&version)?, version })
}

pub fn hide_console(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

pub fn detect(paths: &Paths) -> Vec<Installed> {
    let mut homes: Vec<PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        homes.push(PathBuf::from(home));
    }
    let roots: &[&str] = if cfg!(windows) {
        &[
            "C:\\Program Files\\Java",
            "C:\\Program Files\\Eclipse Adoptium",
            "C:\\Program Files\\Microsoft",
            "C:\\Program Files\\Zulu",
            "C:\\Program Files\\BellSoft",
            "C:\\Program Files\\Amazon Corretto",
        ]
    } else if cfg!(target_os = "macos") {
        &["/Library/Java/JavaVirtualMachines"]
    } else {
        &["/usr/lib/jvm", "/usr/java", "/opt/java", "/opt/jdk"]
    };
    for root in roots {
        if let Ok(dir) = std::fs::read_dir(root) {
            for entry in dir.flatten() {
                let p = entry.path();
                homes.push(if cfg!(target_os = "macos") { p.join("Contents/Home") } else { p });
            }
        }
    }
    if let Ok(dir) = std::fs::read_dir(paths.root.join("runtime")) {
        homes.extend(dir.flatten().map(|e| e.path()));
    }
    let mut found: Vec<Installed> = Vec::new();
    for home in homes {
        let exe = executable(&home);
        let exe = if exe.exists() { exe } else { home.join("bin").join(if cfg!(windows) { "java.exe" } else { "java" }) };
        let real = std::fs::canonicalize(&exe).unwrap_or(exe.clone());
        if found.iter().any(|f| std::fs::canonicalize(&f.java).unwrap_or(f.java.clone()) == real) {
            continue;
        }
        if let Some(java) = probe(&exe) {
            found.push(java);
        }
    }
    found.sort_by_key(|j| std::cmp::Reverse(j.major));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        assert_eq!(parse_major("25.0.1"), Some(25));
        assert_eq!(parse_major("\"21\""), Some(21));
        assert_eq!(parse_major("1.8.0_392"), Some(8));
        assert_eq!(parse_major("26-ea"), Some(26));
        assert_eq!(parse_major("x"), None);
    }

    #[test]
    fn rejects_escaping_paths() {
        assert!(safe("bin/java"));
        assert!(!safe("../evil"));
        assert!(!safe("/etc/passwd"));
        assert!(!safe(""));
    }

    #[test]
    fn reads_runtime_listing() {
        let listing: Listing = serde_json::from_str(
            r#"{"files": {
                "bin": {"type": "directory"},
                "bin/java": {"type": "file", "executable": true, "downloads": {"raw": {"sha1": "ab", "size": 3, "url": "https://x/java"}}},
                "lib/link": {"type": "link", "target": "../bin/java"}
            }}"#,
        )
        .unwrap();
        assert_eq!(listing.files.len(), 3);
        assert!(matches!(listing.files["bin/java"], Item::File { executable: true, .. }));
    }
}
