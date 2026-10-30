use super::mods::{bare_name, Content, Entry, Index};
use super::{background, later, App, Core};
use crate::download::{self, hex, Progress};
use crate::mc::modrinth::{self, ModVersion};
use crate::paths::{load_json, save_json};
use serde_json::json;
use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};
use std::path::Path;

const BATCH: usize = 200;

#[derive(Default)]
struct Outcome {
    updated: usize,
    adopted: usize,
}

pub fn candidate(list: &[ModVersion], installed: &str, beta: bool) -> Option<ModVersion> {
    let best_at = list.iter().position(|v| beta || v.version_type == "release")?;
    let installed_at = list.iter().position(|v| v.files.iter().any(|f| f.filename == installed));
    match installed_at {
        Some(at) if at <= best_at => None,
        _ => Some(list[best_at].clone()),
    }
}

fn adopt(core: &Core, dir: &Path, index: &mut Index) -> usize {
    let known: HashSet<String> = index.projects.values().map(|e| e.file.clone()).collect();
    let mut hashes: HashMap<String, String> = HashMap::new();
    for jar in crate::install::jars(dir) {
        let Some(name) = jar.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
        if known.contains(&name) {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&jar) {
            hashes.insert(hex(&Sha1::digest(&bytes)), name);
        }
    }
    let keys: Vec<&String> = hashes.keys().collect();
    let mut found: HashMap<String, ModVersion> = HashMap::new();
    for chunk in keys.chunks(BATCH) {
        let body = json!({ "hashes": chunk, "algorithm": "sha1" });
        let url = format!("{}/version_files", modrinth::base(&core.ep));
        if let Ok(reply) = core.http.post_json(&url, &body, None, &[]) {
            if let Ok(map) = reply.json::<HashMap<String, ModVersion>>() {
                found.extend(map);
            }
        }
    }
    let mut adopted = 0;
    for (hash, version) in found {
        if let Some(file) = hashes.get(&hash) {
            if !index.projects.contains_key(&version.project_id) {
                index.projects.insert(version.project_id.clone(), Entry { file: file.clone(), slug: version.project_id.clone() });
                adopted += 1;
            }
        }
    }
    adopted
}

struct Plan<'a> {
    core: &'a Core,
    dir: &'a Path,
    index_path: &'a Path,
    game: &'a str,
    loader: &'a str,
    threads: usize,
    beta: bool,
}

fn update(plan: &Plan) -> Result<Outcome, String> {
    let mut index: Index = load_json(plan.index_path);
    let adopted = adopt(plan.core, plan.dir, &mut index);
    let mut jobs = Vec::new();
    let mut replaced = Vec::new();
    for (project, entry) in &index.projects {
        let Ok(list) = modrinth::versions(&plan.core.http, &plan.core.ep, &plan.core.paths, project, plan.game, plan.loader) else { continue };
        let Some(version) = candidate(&list, &entry.file, plan.beta) else { continue };
        let Some(file) = version.primary() else { continue };
        let Some(job) = version.job(plan.dir) else { continue };
        replaced.push((project.clone(), entry.file.clone(), file.filename.clone()));
        jobs.push(job);
    }
    download::run(&plan.core.http, jobs, plan.threads, &Progress::default(), false)?;
    for (project, old, new) in &replaced {
        let was_off = plan.dir.join(format!("{old}.disabled")).exists();
        if bare_name(old) && old != new {
            let _ = std::fs::remove_file(plan.dir.join(old));
            let _ = std::fs::remove_file(plan.dir.join(format!("{old}.disabled")));
        }
        if was_off {
            let _ = std::fs::rename(plan.dir.join(new), plan.dir.join(format!("{new}.disabled")));
        }
        if let Some(entry) = index.projects.get_mut(project) {
            entry.file = new.clone();
        }
    }
    save_json(plan.index_path, &index);
    Ok(Outcome { updated: replaced.len(), adopted })
}

fn dependents(core: &Core, index: &Index, game: &str, loader: &str, project: &str) -> Vec<String> {
    let mut names: Vec<String> = index
        .projects
        .iter()
        .filter(|(id, _)| id.as_str() != project)
        .filter(|(id, entry)| {
            let Ok(list) = modrinth::versions(&core.http, &core.ep, &core.paths, id, game, loader) else { return false };
            list.iter()
                .find(|v| v.files.iter().any(|f| f.filename == entry.file))
                .is_some_and(|v| v.dependencies.iter().any(|d| d.dependency_type == "required" && d.project_id.as_deref() == Some(project)))
        })
        .map(|(_, entry)| entry.file.trim_end_matches(".disabled").trim_end_matches(".jar").to_string())
        .collect();
    names.sort();
    names
}

impl App {
    pub fn update_mods(&mut self) {
        if self.mods.content != Content::Mod {
            self.toast("Updates currently apply to mods only");
            return;
        }
        let core = self.core.clone();
        let profile = self.profile();
        let game = self.game_of(&profile);
        let loader = profile.loader().id();
        let beta = self.settings.beta_updates;
        let threads = self.settings.threads as usize;
        self.toast("Checking for mod updates…");
        background(move || {
            let dir = Content::Mod.dir(&core, &profile.id);
            let index_path = Content::Mod.index_file(&core, &profile.id);
            let plan = Plan { core: &core, dir: &dir, index_path: &index_path, game: &game, loader, threads, beta };
            let result = update(&plan);
            later(move |app| {
                match result {
                    Ok(Outcome { updated: 0, adopted: 0 }) => app.toast("All mods are up to date"),
                    Ok(Outcome { updated: 0, adopted }) => app.toast(format!("All mods are up to date. {adopted} mods are now tracked for updates")),
                    Ok(Outcome { updated, .. }) => app.toast(format!("Updated {updated} mods")),
                    Err(e) => app.toast(format!("Could not update mods: {e}")),
                }
                app.refresh_mods();
            });
        });
    }

    pub fn remove_mod(&mut self, file: &str) {
        let confirmed = self.mods.confirm.take().is_some_and(|armed| armed == file);
        let project = self.mods.index.projects.iter().find(|(_, e)| e.file == file.trim_end_matches(".disabled")).map(|(id, _)| id.clone());
        let Some(project) = project.filter(|_| self.mods.content == Content::Mod && !confirmed) else {
            return self.remove_file(file);
        };
        let core = self.core.clone();
        let profile = self.profile();
        let game = self.game_of(&profile);
        let loader = profile.loader().id();
        let index = self.mods.index.clone();
        let file = file.to_string();
        background(move || {
            let needed_by = dependents(&core, &index, &game, loader, &project);
            later(move |app| {
                if needed_by.is_empty() {
                    app.remove_file(&file);
                } else {
                    app.toast(format!("{} needs this mod. Remove it again to confirm", needed_by.join(", ")));
                    app.mods.confirm = Some(file);
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(id: &str, kind: &str, file: &str) -> ModVersion {
        serde_json::from_value(json!({
            "id": id, "project_id": "p", "version_number": id, "version_type": kind,
            "files": [{"url": "https://cdn/x", "filename": file, "primary": true, "size": 1, "hashes": {}}]
        }))
        .unwrap()
    }

    #[test]
    fn updates_to_the_newest_release_only() {
        let list = vec![version("3", "beta", "m-3b.jar"), version("2", "release", "m-2.jar"), version("1", "release", "m-1.jar")];
        assert_eq!(candidate(&list, "m-1.jar", false).unwrap().id, "2");
        assert!(candidate(&list, "m-2.jar", false).is_none());
        assert_eq!(candidate(&list, "m-2.jar", true).unwrap().id, "3");
    }

    #[test]
    fn never_downgrades_and_handles_unknown_files() {
        let list = vec![version("2", "release", "m-2.jar"), version("1", "release", "m-1.jar")];
        assert!(candidate(&list, "m-2.jar", false).is_none());
        assert_eq!(candidate(&list, "custom-build.jar", false).unwrap().id, "2");
        assert!(candidate(&[version("1", "alpha", "m-1a.jar")], "x.jar", false).is_none());
    }
}
