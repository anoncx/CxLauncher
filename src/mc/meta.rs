use super::libraries::Coord;
use super::rules::{allowed, Env};
use super::Endpoints;
use crate::download::{file_matches, Hash};
use crate::net::Http;
use crate::paths::{write_atomic, Paths};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub latest: Latest,
    #[serde(default)]
    pub versions: Vec<ManifestEntry>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Latest {
    #[serde(default)]
    pub release: String,
}

impl Manifest {
    pub fn releases(&self) -> impl Iterator<Item = &ManifestEntry> {
        self.versions.iter().filter(|v| v.kind == "release")
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub id: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub sha1: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Rule {
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<OsRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct OsRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ArgValue {
    One(String),
    Many(Vec<String>),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Arg {
    Plain(String),
    Ruled { rules: Vec<Rule>, value: ArgValue },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Arg>,
    #[serde(default)]
    pub jvm: Vec<Arg>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Artifact {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub url: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LibDownloads {
    #[serde(default)]
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub classifiers: Option<HashMap<String, Artifact>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibDownloads>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    #[serde(default)]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default)]
    pub extract: Option<Extract>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Downloads {
    #[serde(default)]
    pub client: Option<Artifact>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AssetIndexRef {
    pub id: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    pub url: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    #[serde(default)]
    pub component: Option<String>,
    #[serde(default)]
    pub major_version: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    pub id: String,
    #[serde(default)]
    pub inherits_from: Option<String>,
    #[serde(default)]
    pub main_class: Option<String>,
    #[serde(default)]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(default)]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default)]
    pub assets: Option<String>,
    #[serde(default)]
    pub downloads: Option<Downloads>,
    #[serde(default)]
    pub java_version: Option<JavaVersion>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
}

impl Version {
    pub fn over(self, parent: Version) -> Version {
        let mut libraries = self.libraries;
        libraries.extend(parent.libraries);
        let arguments = match (parent.arguments, self.arguments) {
            (Some(mut p), Some(c)) => {
                p.game.extend(c.game);
                p.jvm.extend(c.jvm);
                Some(p)
            }
            (p, c) => c.or(p),
        };
        Version {
            id: self.id,
            inherits_from: None,
            main_class: self.main_class.or(parent.main_class),
            minecraft_arguments: self.minecraft_arguments.or(parent.minecraft_arguments),
            arguments,
            asset_index: self.asset_index.or(parent.asset_index),
            assets: self.assets.or(parent.assets),
            downloads: self.downloads.or(parent.downloads),
            java_version: self.java_version.or(parent.java_version),
            libraries,
            kind: self.kind.or(parent.kind),
        }
    }

    pub fn active_libraries(&self, env: &Env) -> Vec<&Library> {
        let mut seen = HashSet::new();
        self.libraries
            .iter()
            .filter(|l| allowed(l.rules.as_deref(), env))
            .filter(|l| seen.insert(Coord::parse(&l.name).map(|c| c.key()).unwrap_or_else(|| l.name.clone())))
            .collect()
    }

    pub fn java_major(&self) -> u32 {
        self.java_version.as_ref().map(|j| j.major_version).filter(|v| *v > 0).unwrap_or(21)
    }
}

pub fn cached_json<T: serde::de::DeserializeOwned>(http: &Http, url: &str, cache: &Path) -> Result<T, String> {
    match http.text(url) {
        Ok(text) => {
            let parsed: T = serde_json::from_str(&text).map_err(|e| format!("unexpected answer from {} ({e})", crate::net::host(url)))?;
            let _ = write_atomic(cache, text.as_bytes());
            Ok(parsed)
        }
        Err(online) => {
            let text = std::fs::read_to_string(cache).map_err(|_| online.clone())?;
            serde_json::from_str(&text).map_err(|_| online)
        }
    }
}

pub fn pinned_json<T: serde::de::DeserializeOwned>(http: &Http, url: &str, sha1: Option<&str>, cache: &Path) -> Result<T, String> {
    if let Some(sha1) = sha1.filter(|s| !s.is_empty()) {
        if file_matches(cache, &Hash::Sha1(sha1.to_ascii_lowercase())) {
            if let Ok(parsed) = std::fs::read_to_string(cache).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
                return Ok(parsed);
            }
        }
    }
    cached_json(http, url, cache)
}

pub fn manifest(http: &Http, ep: &Endpoints, paths: &Paths) -> Result<Manifest, String> {
    cached_json(http, &ep.version_manifest, &paths.meta().join("version_manifest_v2.json"))
}

pub fn version(http: &Http, paths: &Paths, manifest: Option<&Manifest>, id: &str) -> Result<Version, String> {
    let cache = paths.meta().join("versions").join(format!("{id}.json"));
    match manifest {
        Some(m) => {
            let entry = m.versions.iter().find(|v| v.id == id).ok_or(format!("Minecraft {id} is not out yet"))?;
            pinned_json(http, &entry.url, entry.sha1.as_deref(), &cache)
        }
        None => {
            let text = std::fs::read_to_string(&cache).map_err(|_| format!("Minecraft {id} needs an internet connection the first time"))?;
            serde_json::from_str(&text).map_err(|e| e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VANILLA: &str = r#"{
        "id": "26.2",
        "type": "release",
        "mainClass": "net.minecraft.client.main.Main",
        "javaVersion": {"component": "java-runtime-epsilon", "majorVersion": 25},
        "assetIndex": {"id": "30", "sha1": "aa", "size": 10, "url": "https://x/30.json"},
        "assets": "30",
        "downloads": {"client": {"sha1": "bb", "size": 20, "url": "https://x/client.jar"}},
        "arguments": {
            "game": ["--username", "${auth_player_name}", {"rules": [{"action": "allow", "features": {"is_demo_user": true}}], "value": "--demo"}],
            "jvm": [{"rules": [{"action": "allow", "os": {"name": "osx"}}], "value": ["-XstartOnFirstThread"]}, "-cp", "${classpath}"]
        },
        "libraries": [
            {"name": "org.ow2.asm:asm:9.3", "downloads": {"artifact": {"path": "org/ow2/asm/asm/9.3/asm-9.3.jar", "sha1": "cc", "size": 1, "url": "https://x/asm-9.3.jar"}}},
            {"name": "org.lwjgl:lwjgl:3.3.3:natives-linux", "rules": [{"action": "allow", "os": {"name": "linux"}}], "downloads": {"artifact": {"path": "l.jar", "url": "https://x/l.jar"}}},
            {"name": "org.lwjgl:lwjgl:3.3.3", "downloads": {"artifact": {"path": "m.jar", "url": "https://x/m.jar"}}}
        ]
    }"#;

    const FABRIC: &str = r#"{
        "id": "fabric-loader-0.19.3-26.2",
        "inheritsFrom": "26.2",
        "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
        "arguments": {"game": [], "jvm": ["-DFabricMcEmu= net.minecraft.client.main.Main "]},
        "libraries": [
            {"name": "org.ow2.asm:asm:9.8", "url": "https://maven.fabricmc.net/", "sha1": "dd", "size": 2},
            {"name": "net.fabricmc:fabric-loader:0.19.3", "url": "https://maven.fabricmc.net/"}
        ]
    }"#;

    #[test]
    fn merges_loader_over_vanilla() {
        let vanilla: Version = serde_json::from_str(VANILLA).unwrap();
        let fabric: Version = serde_json::from_str(FABRIC).unwrap();
        let merged = fabric.over(vanilla);
        assert_eq!(merged.id, "fabric-loader-0.19.3-26.2");
        assert_eq!(merged.main_class.as_deref(), Some("net.fabricmc.loader.impl.launch.knot.KnotClient"));
        assert_eq!(merged.java_major(), 25);
        assert_eq!(merged.asset_index.as_ref().unwrap().id, "30");
        let args = merged.arguments.as_ref().unwrap();
        assert_eq!(args.jvm.len(), 4);
        assert_eq!(args.jvm[3], Arg::Plain("-DFabricMcEmu= net.minecraft.client.main.Main ".into()));

        let env = Env { os: "linux", arch: "x86_64", os_version: String::new(), features: HashMap::new() };
        let names: Vec<&str> = merged.active_libraries(&env).iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["org.ow2.asm:asm:9.8", "net.fabricmc:fabric-loader:0.19.3", "org.lwjgl:lwjgl:3.3.3:natives-linux", "org.lwjgl:lwjgl:3.3.3"]);

        let win = Env { os: "windows", ..env };
        assert_eq!(merged.active_libraries(&win).len(), 3);
    }
}
