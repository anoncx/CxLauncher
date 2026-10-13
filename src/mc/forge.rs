use super::java::hide_console;
use super::loader::Loader;
use super::meta::{cached_json, Version};
use super::Endpoints;
use crate::net::Http;
use crate::paths::{write_atomic, Paths};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Deserialize)]
struct Promotions {
    #[serde(default)]
    promos: HashMap<String, String>,
}

#[derive(Deserialize)]
struct NeoForgeVersions {
    #[serde(default)]
    versions: Vec<String>,
}

pub fn neoforge_prefix(game: &str) -> String {
    let short = game.strip_prefix("1.").unwrap_or(game);
    if short.matches('.').count() == 0 {
        format!("{short}.0.")
    } else {
        format!("{short}.")
    }
}

pub fn pick_neoforge(versions: &[String], game: &str) -> Option<String> {
    let prefix = neoforge_prefix(game);
    let matching = || versions.iter().filter(|v| v.starts_with(&prefix));
    matching().rfind(|v| !v.contains('-')).or_else(|| matching().next_back()).cloned()
}

pub fn latest(http: &Http, ep: &Endpoints, paths: &Paths, loader: Loader, game: &str) -> Result<String, String> {
    let cache = paths.meta().join(loader.id());
    let found = match loader {
        Loader::Forge => {
            let promos: Promotions = cached_json(http, &ep.forge_promos, &cache.join("promotions.json"))?;
            promos.promos.get(&format!("{game}-recommended")).or_else(|| promos.promos.get(&format!("{game}-latest"))).cloned()
        }
        Loader::NeoForge => {
            let list: NeoForgeVersions = cached_json(http, &ep.neoforge_versions, &cache.join("versions.json"))?;
            pick_neoforge(&list.versions, game)
        }
        _ => None,
    };
    found.ok_or(format!("{} has no build for Minecraft {game} yet", loader.label()))
}

fn installer_url(ep: &Endpoints, loader: Loader, game: &str, version: &str) -> String {
    match loader {
        Loader::NeoForge => format!("{0}/net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar", ep.neoforge_maven.trim_end_matches('/')),
        _ => {
            let full = format!("{game}-{version}");
            format!("{0}/net/minecraftforge/forge/{full}/forge-{full}-installer.jar", ep.forge_maven.trim_end_matches('/'))
        }
    }
}

fn version_dirs(paths: &Paths) -> HashSet<String> {
    std::fs::read_dir(paths.root.join("versions"))
        .map(|d| d.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

fn read_version(paths: &Paths, id: &str) -> Option<Version> {
    let text = std::fs::read_to_string(paths.version_dir(id).join(format!("{id}.json"))).ok()?;
    serde_json::from_str(&text).ok()
}

fn tail(output: &std::process::Output) -> String {
    let text = format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    lines[lines.len().saturating_sub(4)..].join(" | ")
}

fn run_installer(java: &Path, installer: &Path, root: &Path, loader: Loader) -> Result<(), String> {
    let profiles = root.join("launcher_profiles.json");
    if !profiles.exists() {
        write_atomic(&profiles, br#"{"profiles":{},"version":3}"#).map_err(|e| e.to_string())?;
    }
    let mut cmd = Command::new(java);
    cmd.arg("-jar").arg(installer).arg("--installClient").arg(root).current_dir(root).stdin(Stdio::null());
    hide_console(&mut cmd);
    let output = cmd.output().map_err(|e| format!("could not start the {} installer: {e}", loader.label()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("The {} installer failed: {}", loader.label(), tail(&output)))
    }
}

pub fn install(http: &Http, ep: &Endpoints, paths: &Paths, loader: Loader, game: &str, version: &str, java: &Path) -> Result<Version, String> {
    let version = version.strip_prefix(&format!("{game}-")).unwrap_or(version);
    let record = paths.meta().join("installs").join(format!("{}-{game}-{version}.id", loader.id()));
    if let Some(found) = std::fs::read_to_string(&record).ok().and_then(|id| read_version(paths, id.trim())) {
        return Ok(found);
    }
    let installer = paths.cache().join("installers").join(format!("{}-{game}-{version}.jar", loader.id()));
    if !installer.exists() {
        let bytes = http.bytes(&installer_url(ep, loader, game, version), 128 << 20).map_err(|e| format!("Could not download {} {version} ({e})", loader.label()))?;
        write_atomic(&installer, &bytes).map_err(|e| e.to_string())?;
    }
    let before = version_dirs(paths);
    run_installer(java, &installer, &paths.root, loader)?;
    let after = version_dirs(paths);
    let id = after
        .difference(&before)
        .find(|id| id.contains(loader.id()) || id.to_lowercase().contains(loader.id()))
        .or_else(|| after.iter().find(|id| id.contains(version) && id.to_lowercase().contains(loader.id())))
        .cloned()
        .ok_or(format!("The {} installer finished but left no version behind", loader.label()))?;
    let installed = read_version(paths, &id).ok_or(format!("The {} installer wrote an unreadable version file", loader.label()))?;
    write_atomic(&record, id.as_bytes()).map_err(|e| e.to_string())?;
    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_minecraft_versions_to_neoforge_lines() {
        assert_eq!(neoforge_prefix("1.21.1"), "21.1.");
        assert_eq!(neoforge_prefix("1.21"), "21.0.");
        assert_eq!(neoforge_prefix("26.3"), "26.3.");
    }

    #[test]
    fn prefers_stable_neoforge_builds() {
        let list: Vec<String> = ["21.1.1", "21.1.2", "21.1.3-beta", "21.0.5", "21.10.1"].iter().map(|s| s.to_string()).collect();
        assert_eq!(pick_neoforge(&list, "1.21.1").as_deref(), Some("21.1.2"));
        assert_eq!(pick_neoforge(&list, "1.21").as_deref(), Some("21.0.5"));
        assert_eq!(pick_neoforge(&list, "1.19"), None);
        assert_eq!(pick_neoforge(&["21.1.9-beta".to_string()], "1.21.1").as_deref(), Some("21.1.9-beta"));
    }

    #[test]
    fn builds_installer_urls() {
        let ep = Endpoints::default();
        assert_eq!(
            installer_url(&ep, Loader::Forge, "1.20.1", "47.2.0"),
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.1-47.2.0/forge-1.20.1-47.2.0-installer.jar"
        );
        assert_eq!(
            installer_url(&ep, Loader::NeoForge, "1.21.1", "21.1.2"),
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.2/neoforge-21.1.2-installer.jar"
        );
    }
}
