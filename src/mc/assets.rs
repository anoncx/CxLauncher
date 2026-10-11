use super::meta::{pinned_json, AssetIndexRef};
use crate::download::Job;
use crate::net::Http;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct AssetIndex {
    #[serde(default)]
    pub objects: HashMap<String, AssetObject>,
    #[serde(default, rename = "virtual")]
    pub legacy: bool,
    #[serde(default)]
    pub map_to_resources: bool,
}

#[derive(Debug, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

#[derive(Debug, Default)]
pub struct Assets {
    pub jobs: Vec<Job>,
    pub copies: Vec<(PathBuf, PathBuf)>,
    pub root: PathBuf,
}

fn safe(name: &str) -> bool {
    Path::new(name).components().all(|c| matches!(c, Component::Normal(_)))
}

pub fn plan(http: &Http, index: &AssetIndexRef, assets: &Path, resources: &str, game_dir: &Path) -> Result<Assets, String> {
    let file = assets.join("indexes").join(format!("{}.json", index.id));
    let parsed: AssetIndex = pinned_json(http, &index.url, index.sha1.as_deref(), &file)?;
    let base = resources.trim_end_matches('/');
    let objects = assets.join("objects");
    let legacy_root = assets.join("virtual").join(&index.id);
    let mut out = Assets { root: if parsed.legacy { legacy_root.clone() } else { assets.to_path_buf() }, ..Default::default() };
    for (name, o) in parsed.objects {
        if o.hash.len() <= 2 {
            continue;
        }
        let prefix = &o.hash[..2];
        let stored = objects.join(prefix).join(&o.hash);
        out.jobs.push(Job::new(format!("{base}/{prefix}/{}", o.hash), stored.clone()).sha1(Some(&o.hash)).size(Some(o.size)));
        if safe(&name) {
            if parsed.legacy {
                out.copies.push((stored.clone(), legacy_root.join(&name)));
            }
            if parsed.map_to_resources {
                out.copies.push((stored, game_dir.join("resources").join(&name)));
            }
        }
    }
    Ok(out)
}

pub fn place(copies: &[(PathBuf, PathBuf)]) -> Result<(), String> {
    for (from, to) in copies {
        if to.exists() {
            continue;
        }
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::copy(from, to).map_err(|e| format!("cannot place {}: {e}", to.display()))?;
    }
    Ok(())
}
