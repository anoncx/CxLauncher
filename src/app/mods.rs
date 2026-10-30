use super::finder::{Finder, Kind};
use super::images::{thumb, Pixels};
use super::options;
use super::{background, first_letter, later, App, Core};
use crate::download::{self, Progress};
use crate::install::{files_ext, jars};
use crate::mc::{modmeta, modrinth};
use crate::paths::{load_json, save_json};
use crate::ModItem;
use serde::{Deserialize, Serialize};
use slint::Image;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

const FABRIC_API: &str = "P7dR8mSH";
const BOOST: [(&str, &str); 5] =
    [("sodium", "sodium"), ("lithium", "lithium"), ("ferrite-core", "ferritecore"), ("entityculling", "entityculling"), ("immediatelyfast", "immediatelyfast")];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Content {
    #[default]
    Mod,
    ResourcePack,
    Shader,
}

impl Content {
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Content::ResourcePack,
            2 => Content::Shader,
            _ => Content::Mod,
        }
    }

    pub fn project_type(self) -> &'static str {
        match self {
            Content::Mod => "mod",
            Content::ResourcePack => "resourcepack",
            Content::Shader => "shader",
        }
    }

    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Content::Mod => "Mods",
            Content::ResourcePack => "Resource packs",
            Content::Shader => "Shaders",
        }
    }

    fn dir_name(self) -> &'static str {
        match self {
            Content::Mod => "mods",
            Content::ResourcePack => "resourcepacks",
            Content::Shader => "shaderpacks",
        }
    }

    fn ext(self) -> &'static str {
        match self {
            Content::Mod => "jar",
            _ => "zip",
        }
    }

    pub(super) fn dir(self, core: &Core, profile: &str) -> PathBuf {
        core.paths.profile(profile).join(self.dir_name())
    }

    pub(super) fn index_file(self, core: &Core, profile: &str) -> PathBuf {
        core.paths.profile(profile).join(format!("cxlauncher-{}.json", self.dir_name()))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub file: String,
    pub slug: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Index {
    pub projects: HashMap<String, Entry>,
}

#[derive(Default)]
pub struct Mods {
    pub find: Finder,
    pub content: Content,
    icons: HashMap<String, Image>,
    ids: HashSet<String>,
    pub(crate) index: Index,
    boosting: bool,
    pub(super) confirm: Option<String>,
}

struct Found {
    file: String,
    stamp: String,
    info: Option<modmeta::ModInfo>,
    icon: Option<Pixels>,
    enabled: bool,
}

fn stamp(path: &Path) -> String {
    let meta = std::fs::metadata(path).ok();
    let modified = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    format!("{}:{}:{modified}", path.display(), meta.map(|m| m.len()).unwrap_or(0))
}

fn pack_png(path: &Path) -> Option<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    let mut out = Vec::new();
    zip.by_name("pack.png").ok()?.take(4 << 20).read_to_end(&mut out).ok()?;
    Some(out)
}

fn scan(dir: &Path, ext: &str, known: &HashSet<String>) -> Vec<Found> {
    let mut files: Vec<PathBuf> = files_ext(dir, ext);
    if ext == "jar" {
        if let Ok(d) = std::fs::read_dir(dir) {
            files.extend(d.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".jar.disabled")));
        }
    }
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    files
        .into_iter()
        .map(|path| {
            let key = stamp(&path);
            let fresh = !known.contains(&key);
            let mut info = if ext == "jar" { modmeta::read(&path, fresh) } else { None };
            let icon = info
                .as_mut()
                .and_then(|i| i.icon.take())
                .and_then(|b| thumb(&b, 64))
                .or_else(|| fresh.then(|| pack_png(&path).and_then(|b| thumb(&b, 64))).flatten());
            Found {
                file: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                enabled: path.extension().is_some_and(|e| e == ext),
                stamp: key,
                info,
                icon,
            }
        })
        .collect()
}

pub(super) fn bare_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':'])
}

fn resource_packs(lines: &[String]) -> Vec<String> {
    options::get(lines, "resourcePacks").and_then(|v| serde_json::from_str::<Vec<String>>(v).ok()).unwrap_or_default()
}

#[derive(Clone, Copy)]
struct Target<'a> {
    game: &'a str,
    loader: &'a str,
}

fn resolve(core: &Core, target: Target, project: &str, skip: &HashSet<String>, depth: u8, out: &mut Vec<(modrinth::ModVersion, String)>) -> Result<bool, String> {
    let Some(version) = modrinth::pick(modrinth::versions(&core.http, &core.ep, &core.paths, project, target.game, target.loader)?, None) else {
        return Ok(false);
    };
    if skip.contains(&version.project_id) || out.iter().any(|(v, _)| v.project_id == version.project_id) {
        return Ok(true);
    }
    let deps: Vec<String> = version.dependencies.iter().filter(|d| d.dependency_type == "required").filter_map(|d| d.project_id.clone()).collect();
    out.push((version, project.to_string()));
    if depth < 3 {
        for dep in deps {
            if !skip.contains(&dep) {
                resolve(core, target, &dep, skip, depth + 1, out)?;
            }
        }
    }
    Ok(true)
}

type Installed = (Vec<(String, Entry)>, Vec<String>);

fn install_all(core: &Core, content: Content, dir: &Path, target: Target, projects: &[String], skip: &HashSet<String>, threads: usize) -> Result<Installed, String> {
    let mut picked = Vec::new();
    let mut missing = Vec::new();
    for project in projects {
        match content {
            Content::Mod => {
                if !resolve(core, target, project, skip, 0, &mut picked)? {
                    missing.push(project.clone());
                }
            }
            _ => match modrinth::pick(modrinth::versions_any(&core.http, &core.ep, &core.paths, project, target.game)?, None) {
                Some(version) => picked.push((version, project.clone())),
                None => missing.push(project.clone()),
            },
        }
    }
    let mut jobs = Vec::new();
    let mut entries = Vec::new();
    for (version, slug) in &picked {
        if let Some(job) = version.job(dir) {
            let file = job.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            entries.push((version.project_id.clone(), Entry { file, slug: slug.clone() }));
            jobs.push(job);
        }
    }
    download::run(&core.http, jobs, threads, &Progress::default(), false)?;
    Ok((entries, missing))
}

impl App {
    pub fn open_mods_folder(&mut self) {
        crate::sys::reveal(&self.mods.content.dir(&self.core, &self.profile().id));
    }

    fn skip_projects(&self) -> HashSet<String> {
        let mut skip: HashSet<String> = self.mods.index.projects.keys().cloned().collect();
        if self.profile().uses_client() {
            skip.insert(FABRIC_API.into());
        }
        skip
    }

    pub fn set_content(&mut self, index: i32) {
        let content = Content::from_index(index);
        if self.mods.content == content {
            return;
        }
        self.mods.content = content;
        self.mods.find.hits.clear();
        self.models.installed.set_vec(Vec::new());
        self.models.results.set_vec(Vec::new());
        self.refresh_mods();
        if self.ui().map(|u| u.get_mods_tab()).unwrap_or(0) == 1 {
            let query = self.mods.find.query.clone();
            self.find(Kind::Mod, query, true);
        }
    }

    pub fn refresh_mods(&mut self) {
        let content = self.mods.content;
        let profile = self.profile();
        let core = self.core.clone();
        let dir = content.dir(&core, &profile.id);
        let opts = options::path(&core, &profile.id);
        let known: HashSet<String> = self.mods.icons.keys().cloned().collect();
        background(move || {
            let mut found = scan(&dir, content.ext(), &known);
            match content {
                Content::Mod => {}
                Content::ResourcePack => {
                    let packs = resource_packs(&options::read(&opts));
                    for f in &mut found {
                        f.enabled = packs.iter().any(|p| p == &format!("file/{}", f.file) || p == &f.file);
                    }
                }
                Content::Shader => {
                    let active = options::get(&options::read(&opts), "shaderPack").map(str::to_string);
                    for f in &mut found {
                        f.enabled = active.as_deref() == Some(f.file.as_str());
                    }
                }
            }
            let mut index: Index = load_json(&content.index_file(&core, &profile.id));
            index.projects.retain(|_, e| dir.join(&e.file).exists() || dir.join(format!("{}.disabled", e.file)).exists());
            let managed = if content == Content::Mod && profile.uses_client() {
                [("cxvisuals", "CxVisuals"), ("fabric-api", "Fabric API")]
                    .iter()
                    .map(|(id, name)| {
                        let jar = jars(&core.paths.managed(id)).into_iter().next();
                        let info = jar.as_ref().and_then(|j| modmeta::read(j, true));
                        let icon = info.as_ref().and_then(|i| i.icon.as_deref()).and_then(|b| thumb(b, 64));
                        (id.to_string(), name.to_string(), info.map(|i| i.version).unwrap_or_default(), icon)
                    })
                    .collect()
            } else {
                Vec::new()
            };
            later(move |app| app.show_mods(found, managed, index));
        });
    }

    fn show_mods(&mut self, found: Vec<Found>, managed: Vec<(String, String, String, Option<Pixels>)>, index: Index) {
        self.mods.ids = found.iter().filter_map(|f| f.info.as_ref().map(|i| i.id.clone())).collect();
        self.mods.index = index;
        let mut items = Vec::new();
        for (id, name, version, icon) in managed {
            if self.mods.ids.contains(&id) {
                continue;
            }
            let has_icon = icon.is_some();
            items.push(ModItem {
                file: id.as_str().into(),
                name: name.as_str().into(),
                version: version.as_str().into(),
                detail: if version.is_empty() { "Installs with the next launch".into() } else { "Kept up to date by CxLauncher".into() },
                icon: icon.map(Image::from_rgba8).unwrap_or_default(),
                has_icon,
                letter: first_letter(&name),
                enabled: true,
                managed: true,
            });
        }
        let mut icons = HashMap::new();
        for f in found {
            let icon = match f.icon {
                Some(px) => Some(Image::from_rgba8(px)),
                None => self.mods.icons.get(&f.stamp).cloned(),
            };
            if let Some(i) = &icon {
                icons.insert(f.stamp.clone(), i.clone());
            }
            let name = f.info.as_ref().map(|i| i.name.clone()).unwrap_or_else(|| f.file.trim_end_matches(".disabled").trim_end_matches(".jar").trim_end_matches(".zip").to_string());
            items.push(ModItem {
                file: f.file.as_str().into(),
                letter: first_letter(&name),
                name: name.into(),
                version: f.info.as_ref().map(|i| i.version.clone()).unwrap_or_default().into(),
                detail: f.info.as_ref().map(|i| i.description.clone()).filter(|d| !d.is_empty()).unwrap_or_else(|| f.file.clone()).into(),
                has_icon: icon.is_some(),
                icon: icon.unwrap_or_default(),
                enabled: f.enabled,
                managed: false,
            });
        }
        self.mods.icons = icons;
        self.models.installed.set_vec(items);
        self.push_boost();
        self.mark_results(Kind::Mod);
    }

    fn push_boost(&self) {
        let Some(ui) = self.ui() else { return };
        if self.mods.content != Content::Mod {
            ui.set_boost_state(2);
            return;
        }
        let missing: Vec<&str> = BOOST.iter().filter(|(slug, id)| !self.mods.ids.contains(*id) && !self.mods.index.projects.values().any(|e| e.slug == *slug)).map(|(s, _)| *s).collect();
        ui.set_boost_state(if self.mods.boosting { 1 } else if missing.is_empty() { 2 } else { 0 });
        ui.set_boost_detail(
            if missing.len() == BOOST.len() {
                "Sodium, Lithium, FerriteCore, EntityCulling and ImmediatelyFast. Often doubles FPS on older PCs.".to_string()
            } else {
                format!("{} of {} performance mods installed. Add the rest in one click.", BOOST.len() - missing.len(), BOOST.len())
            }
            .into(),
        );
    }

    pub fn installed_hit(&self, hit: &modrinth::Hit) -> bool {
        self.mods.index.projects.contains_key(&hit.project_id)
            || match self.mods.content {
                Content::Mod => self.mods.ids.contains(&hit.slug) || self.mods.ids.contains(&hit.slug.replace('-', "")),
                _ => false,
            }
    }

    pub fn toggle_mod(&mut self, file: &str, on: bool) {
        if file.contains(['/', '\\']) {
            self.toast("Could not change that file");
            return;
        }
        let content = self.mods.content;
        let profile = self.profile().id;
        match content {
            Content::Mod => {
                let dir = content.dir(&self.core, &profile);
                let from = dir.join(file);
                let to = if on { dir.join(file.trim_end_matches(".disabled")) } else { dir.join(format!("{file}.disabled")) };
                if std::fs::rename(&from, &to).is_err() {
                    self.toast("Could not change that mod");
                }
            }
            Content::ResourcePack => {
                let path = options::path(&self.core, &profile);
                let mut lines = options::read(&path);
                let mut packs = resource_packs(&lines);
                let entry = format!("file/{file}");
                if on && !packs.contains(&entry) {
                    packs.push(entry.clone());
                }
                if !on {
                    packs.retain(|p| p != &entry && p != file);
                }
                let value = serde_json::to_string(&packs).unwrap_or_default();
                options::set(&mut lines, "resourcePacks", &value);
                if options::write(&path, &lines).is_err() {
                    self.toast("Could not change that resource pack");
                }
            }
            Content::Shader => {
                let path = options::path(&self.core, &profile);
                let mut lines = options::read(&path);
                options::set(&mut lines, "shaderPack", if on { file } else { "OFF" });
                if options::write(&path, &lines).is_err() {
                    self.toast("Could not change that shader");
                }
            }
        }
        self.refresh_mods();
    }

    pub(super) fn remove_file(&mut self, file: &str) {
        let content = self.mods.content;
        let profile = self.profile().id;
        let path = content.dir(&self.core, &profile).join(file);
        if !bare_name(file) || std::fs::remove_file(&path).is_err() {
            self.toast("Could not remove that file");
            return;
        }
        self.toast(format!("Removed {}", file.trim_end_matches(".disabled")));
        let opts = options::path(&self.core, &profile);
        match content {
            Content::ResourcePack => {
                let mut lines = options::read(&opts);
                let mut packs = resource_packs(&lines);
                packs.retain(|p| p != &format!("file/{file}") && p != file);
                let value = serde_json::to_string(&packs).unwrap_or_default();
                options::set(&mut lines, "resourcePacks", &value);
                let _ = options::write(&opts, &lines);
            }
            Content::Shader => {
                let mut lines = options::read(&opts);
                if options::get(&lines, "shaderPack") == Some(file) {
                    options::set(&mut lines, "shaderPack", "OFF");
                    let _ = options::write(&opts, &lines);
                }
            }
            Content::Mod => {}
        }
        self.refresh_mods();
    }

    fn install_projects(&mut self, content: Content, projects: Vec<String>, on_done: impl FnOnce(&mut App, Result<(usize, Vec<String>), String>) + Send + 'static) {
        let core = self.core.clone();
        let profile = self.profile();
        let dir = content.dir(&core, &profile.id);
        let game = self.game_of(&profile);
        let loader = profile.loader().id();
        let skip = self.skip_projects();
        let threads = self.settings.threads as usize;
        background(move || {
            let result = install_all(&core, content, &dir, Target { game: &game, loader }, &projects, &skip, threads).map(|(entries, missing)| {
                let path = content.index_file(&core, &profile.id);
                let mut index: Index = load_json(&path);
                let count = entries.len();
                index.projects.extend(entries);
                save_json(&path, &index);
                (count, missing)
            });
            later(move |app| {
                on_done(app, result);
                app.refresh_mods();
            });
        });
    }

    pub fn install(&mut self, id: &str) {
        if !self.begin_install(Kind::Mod, id) {
            return;
        }
        let content = self.mods.content;
        let title = self.hit_title(Kind::Mod, id);
        let id = id.to_string();
        self.install_projects(content, vec![id.clone()], move |app, result| {
            app.end_install(Kind::Mod, &id);
            match result {
                Ok((_, missing)) if !missing.is_empty() => app.toast(format!("{title} is not out for this Minecraft version yet")),
                Ok((n, _)) if n > 1 => app.toast(format!("Installed {title} and {} needed mods", n - 1)),
                Ok(_) => app.toast(format!("Installed {title}")),
                Err(e) => app.toast(e),
            }
        });
    }

    pub fn boost(&mut self) {
        if self.mods.boosting {
            return;
        }
        self.mods.boosting = true;
        self.push_boost();
        let wanted: Vec<String> = BOOST.iter().filter(|(slug, id)| !self.mods.ids.contains(*id) && !self.mods.index.projects.values().any(|e| e.slug == *slug)).map(|(s, _)| s.to_string()).collect();
        self.install_projects(Content::Mod, wanted, |app, result| {
            app.mods.boosting = false;
            match result {
                Ok((n, missing)) if missing.is_empty() => app.toast(format!("Installed {n} performance mods")),
                Ok((n, missing)) => app.toast(format!("Installed {n} performance mods · {} not out for this version yet", missing.len())),
                Err(e) => app.toast(e),
            }
            app.push_boost();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_stay_inside_the_content_directory() {
        assert!(bare_name("sodium-0.5.8.jar"));
        assert!(bare_name("sodium.jar.disabled"));
        assert!(!bare_name("../evil.jar"));
        assert!(!bare_name("../../.bashrc"));
        assert!(!bare_name("sub/evil.jar"));
        assert!(!bare_name("sub\\evil.jar"));
        assert!(!bare_name("C:evil.jar"));
        assert!(!bare_name("/etc/passwd"));
        assert!(!bare_name(".."));
        assert!(!bare_name("."));
        assert!(!bare_name(""));
    }
}
