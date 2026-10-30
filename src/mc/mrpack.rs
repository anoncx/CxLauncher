use crate::download::{self, Hash, Job, Progress};
use super::loader::Loader;
use crate::net::{host, Http};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Component, Path};

#[derive(Debug, Deserialize)]
struct Hashes {
    #[serde(default)]
    sha1: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Sides {
    #[serde(default)]
    client: String,
}

#[derive(Debug, Deserialize)]
struct PackFile {
    path: String,
    hashes: Hashes,
    #[serde(default)]
    downloads: Vec<String>,
    #[serde(default, rename = "fileSize")]
    size: Option<u64>,
    #[serde(default)]
    env: Option<Sides>,
}

#[derive(Debug, Deserialize)]
struct Index {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "versionId")]
    version: String,
    #[serde(default)]
    files: Vec<PackFile>,
    #[serde(default)]
    dependencies: HashMap<String, String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pack {
    pub name: String,
    pub version: String,
    pub game: String,
    pub loader: Option<(Loader, String)>,
}

const TRUSTED: [&str; 4] = ["cdn.modrinth.com", "github.com", "raw.githubusercontent.com", "gitlab.com"];

fn trusted(url: &str) -> bool {
    let local = cfg!(debug_assertions) && url.starts_with("http://127.0.0.1");
    local || (url.starts_with("https://") && TRUSTED.contains(&host(url)))
}

fn safe(rel: &str) -> bool {
    !rel.is_empty() && Path::new(rel).components().all(|c| matches!(c, Component::Normal(_)))
}

fn index(zip: &mut zip::ZipArchive<std::fs::File>) -> Result<Index, String> {
    let mut raw = String::new();
    zip.by_name("modrinth.index.json")
        .map_err(|_| "this is not a Modrinth modpack".to_string())?
        .take(16 << 20)
        .read_to_string(&mut raw)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| format!("the modpack index is damaged ({e})"))
}

fn describe(index: &Index) -> Result<Pack, String> {
    let game = index.dependencies.get("minecraft").cloned().ok_or("the modpack names no Minecraft version")?;
    let loader = Loader::ALL.into_iter().find_map(|l| index.dependencies.get(l.pack_key()).map(|v| (l, v.clone())));
    Ok(Pack { name: index.name.clone(), version: index.version.clone(), game, loader })
}

pub fn read(file: &Path) -> Result<Pack, String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(file).map_err(|e| e.to_string())?).map_err(|_| "this is not a modpack file".to_string())?;
    describe(&index(&mut zip)?)
}

fn overrides(zip: &mut zip::ZipArchive<std::fs::File>, game_dir: &Path) -> Result<(), String> {
    for prefix in ["overrides/", "client-overrides/"] {
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            let Some(rel) = entry.name().strip_prefix(prefix).map(str::to_string) else { continue };
            if entry.is_dir() || !safe(&rel) {
                continue;
            }
            let out = game_dir.join(&rel);
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let mut file = std::fs::File::create(&out).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            std::io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn install(http: &Http, file: &Path, game_dir: &Path, threads: usize, progress: &Progress) -> Result<Pack, String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(file).map_err(|e| e.to_string())?).map_err(|_| "this is not a modpack file".to_string())?;
    let index = index(&mut zip)?;
    let pack = describe(&index)?;
    let wanted: Vec<&PackFile> = index.files.iter().filter(|f| f.env.as_ref().is_none_or(|e| e.client != "unsupported")).filter(|f| safe(&f.path)).collect();
    if let Some(url) = wanted.iter().filter_map(|f| f.downloads.first()).find(|u| !trusted(u)) {
        return Err(format!("This modpack downloads from {}, which Modrinth does not allow, so nothing was installed", host(url)));
    }
    let jobs: Vec<Job> = wanted
        .into_iter()
        .filter_map(|f| {
            let url = f.downloads.first()?;
            let mut job = Job::new(url, game_dir.join(&f.path)).size(f.size);
            job.hash = f.hashes.sha1.clone().map(|h| Hash::Sha1(h.to_ascii_lowercase()));
            Some(job)
        })
        .collect();
    std::fs::create_dir_all(game_dir).map_err(|e| e.to_string())?;
    download::run(http, jobs, threads, progress, false)?;
    overrides(&mut zip, game_dir)?;
    Ok(pack)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mc::modmeta::write_jar;

    #[test]
    fn reads_and_unpacks_overrides_safely() {
        let dir = std::env::temp_dir().join(format!("cxl-pack-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("pack.mrpack");
        let index = br#"{"formatVersion": 1, "game": "minecraft", "versionId": "6.1", "name": "Fast Pack",
            "files": [{"path": "../escape.jar", "hashes": {"sha1": "aa"}, "downloads": ["http://127.0.0.1:1/x"], "fileSize": 1},
                      {"path": "mods/server.jar", "hashes": {"sha1": "aa"}, "downloads": ["http://127.0.0.1:1/x"], "env": {"client": "unsupported", "server": "required"}}],
            "dependencies": {"minecraft": "1.21.4", "fabric-loader": "0.16.10"}}"#;
        write_jar(&file, &[("modrinth.index.json", index), ("overrides/config/a.txt", b"a"), ("client-overrides/options.txt", b"o"), ("overrides/../../evil.txt", b"x")]);
        let pack = read(&file).unwrap();
        assert_eq!(pack, Pack { name: "Fast Pack".into(), version: "6.1".into(), game: "1.21.4".into(), loader: Some((Loader::Fabric, "0.16.10".into())) });
        let game = dir.join("game");
        install(&Http::direct(), &file, &game, 2, &Progress::default()).unwrap();
        assert_eq!(std::fs::read(game.join("config/a.txt")).unwrap(), b"a");
        assert_eq!(std::fs::read(game.join("options.txt")).unwrap(), b"o");
        assert!(!dir.join("evil.txt").exists() && !dir.join("escape.jar").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reads_every_loader() {
        let index = |key: &str| Index { name: "F".into(), version: "1".into(), files: Vec::new(), dependencies: [("minecraft".to_string(), "1.20.1".to_string()), (key.to_string(), "47".to_string())].into_iter().collect() };
        assert_eq!(describe(&index("forge")).unwrap().loader, Some((Loader::Forge, "47".into())));
        assert_eq!(describe(&index("neoforge")).unwrap().loader, Some((Loader::NeoForge, "47".into())));
        assert_eq!(describe(&index("quilt-loader")).unwrap().loader, Some((Loader::Quilt, "47".into())));
    }
}
