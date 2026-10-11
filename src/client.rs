use crate::download::{Hash, Job};
use crate::mc::meta::cached_json;
use crate::mc::{modmeta, Endpoints};
use crate::net::Http;
use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const GAME: &str = "26.2";
pub const NAME: &str = "CxVisuals";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Requirement {
    pub project: String,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub minecraft: String,
    pub loader: Option<String>,
    pub file: String,
    pub url: Option<String>,
    pub size: Option<u64>,
    pub sha1: Option<String>,
    pub sha256: Option<String>,
    pub requires: Vec<Requirement>,
    pub notes: Vec<String>,
    pub published: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Client {
    pub name: String,
    pub version: String,
    pub minecraft: String,
    pub loader: Option<String>,
    pub jar: PathBuf,
    pub job: Option<Job>,
    pub requires: Vec<Requirement>,
    pub local: bool,
}

fn default_requires() -> Vec<Requirement> {
    vec![Requirement { project: "fabric-api".into(), version: None }]
}

fn valid_file(name: &str) -> bool {
    !name.is_empty() && name.ends_with(".jar") && !name.starts_with('.') && !name.contains(['/', '\\'])
}

pub fn manifest(http: &Http, ep: &Endpoints, paths: &Paths) -> Result<Manifest, String> {
    cached_json(http, &ep.client, &paths.meta().join("client.json"))
}

pub fn from_cloud(http: &Http, ep: &Endpoints, paths: &Paths) -> Result<Client, String> {
    let m = manifest(http, ep, paths).map_err(|e| format!("cannot reach the {NAME} cloud: {e}"))?;
    if !valid_file(&m.file) {
        return Err(format!("the {NAME} cloud sent a bad file name"));
    }
    let url = m.url.clone().filter(|u| !u.is_empty()).unwrap_or_else(|| format!("{}/{}", ep.client.trim_end_matches('/'), m.file));
    let jar = paths.managed("cxvisuals").join(&m.file);
    let mut job = Job::new(url, jar.clone()).size(m.size);
    job.hash = match (&m.sha1, &m.sha256) {
        (Some(s), _) if !s.is_empty() => Some(Hash::Sha1(s.to_ascii_lowercase())),
        (_, Some(s)) if !s.is_empty() => Some(Hash::Sha256(s.to_ascii_lowercase())),
        _ => None,
    };
    Ok(Client {
        name: Some(m.name).filter(|n| !n.is_empty()).unwrap_or_else(|| NAME.into()),
        version: m.version,
        minecraft: Some(m.minecraft).filter(|v| !v.is_empty()).unwrap_or_else(|| GAME.into()),
        loader: m.loader.filter(|l| !l.is_empty()),
        jar,
        job: Some(job),
        requires: if m.requires.is_empty() { default_requires() } else { m.requires },
        local: false,
    })
}

pub fn from_jar(jar: PathBuf) -> Result<Client, String> {
    let info = modmeta::read(&jar, false).ok_or(format!("{} is not a Fabric mod", jar.display()))?;
    Ok(Client {
        name: info.name,
        version: info.version,
        minecraft: modmeta::game_version(&info.minecraft).unwrap_or_else(|| GAME.into()),
        loader: None,
        jar,
        job: None,
        requires: default_requires(),
        local: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_file_names() {
        assert!(valid_file("cxvisuals-0.1.0+26.2.jar"));
        assert!(!valid_file("../cxvisuals.jar"));
        assert!(!valid_file("a/b.jar"));
        assert!(!valid_file("cxvisuals.exe"));
    }

    #[test]
    fn reads_published_manifest() {
        let m: Manifest = serde_json::from_str(
            r#"{"name": "CxVisuals", "version": "0.1.0+26.2", "minecraft": "26.2", "loader": "0.19.3", "file": "cxvisuals-0.1.0+26.2.jar",
                "size": 7, "sha1": "78f030564d7efdbd189214ec651878610c27aec2", "sha256": "4f9d", "requires": [{"project": "fabric-api", "version": "0.154.2+26.2"}],
                "notes": ["Added Signal effect - Shaders"], "published": "2026-10-08T08:21:54+00:00"}"#,
        )
        .unwrap();
        assert_eq!(m.requires[0].project, "fabric-api");
        assert_eq!(m.requires[0].version.as_deref(), Some("0.154.2+26.2"));
        assert_eq!(m.loader.as_deref(), Some("0.19.3"));
        assert_eq!(m.notes.len(), 1);
    }

    #[test]
    fn reads_manifest_defaults() {
        let m: Manifest = serde_json::from_str(r#"{"version": "0.1.0+26.2", "minecraft": "26.2", "file": "cxvisuals.jar", "sha1": "AB"}"#).unwrap();
        assert!(m.requires.is_empty());
        assert_eq!(m.sha1.as_deref(), Some("AB"));
    }
}
