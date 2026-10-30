use super::finder::Kind;
use super::play::Run;
use super::{background, later, App};
use crate::download::Progress;
use crate::mc::loader::Loader;
use crate::mc::{modrinth, mrpack};
use crate::store::{new_id, Profile};
use crate::sys::{ago, duration, now, Machine};
use crate::{Command, ProfileEdit, ProfileItem, VersionItem};
use std::path::{Path, PathBuf};

pub(crate) fn copy_dir(from: &Path, to: &Path) {
    let Ok(entries) = std::fs::read_dir(from) else { return };
    for entry in entries.flatten() {
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            let _ = std::fs::create_dir_all(&dst);
            copy_dir(&src, &dst);
        } else if src.is_file() {
            if let Some(dir) = dst.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::copy(&src, &dst);
        }
    }
}

fn stats(p: &Profile) -> String {
    match p.played {
        0 => "Never played".into(),
        s => format!("{} played  ·  {}", duration(s), ago(p.last_played, now()).to_lowercase()),
    }
}

pub struct VersionEntry {
    pub id: String,
    pub kind: String,
}

impl VersionEntry {
    fn tag(&self) -> &'static str {
        match self.kind.as_str() {
            "snapshot" => "SNAPSHOT",
            "old_beta" => "BETA",
            "old_alpha" => "ALPHA",
            _ => "",
        }
    }
}

#[derive(Default)]
pub struct VersionFilter {
    snapshots: bool,
    query: String,
}

impl VersionFilter {
    fn parse(arg: &str) -> Self {
        Self { snapshots: arg.starts_with('1'), query: arg.get(1..).unwrap_or_default().trim().to_lowercase() }
    }

    fn admits(&self, version: &VersionEntry) -> bool {
        (self.snapshots || version.kind == "release") && version.id.to_lowercase().contains(&self.query)
    }
}

impl App {
    fn describe(&self, p: &Profile) -> String {
        let game = self.game_of(p);
        let loader = if p.vanilla { "Vanilla" } else { p.loader().label() };
        let extra = if p.uses_client() { "  ·  CxVisuals" } else { "" };
        format!("Minecraft {game}  ·  {loader}{extra}")
    }

    pub fn push_profiles(&mut self) {
        let Some(ui) = self.ui() else { return };
        let current = self.profile();
        let mut groups: Vec<String> = self.profiles.list.iter().map(|p| p.group.clone()).filter(|g| !g.is_empty()).collect();
        groups.sort();
        groups.dedup();
        if !groups.contains(&self.group_filter) {
            self.group_filter.clear();
        }
        let shown = if self.group_filter.is_empty() { Vec::new() } else { vec![self.group_filter.clone()] };
        let items: Vec<ProfileItem> = self
            .profiles
            .list
            .iter()
            .filter(|p| shown.is_empty() || shown.contains(&p.group))
            .map(|p| ProfileItem {
                id: p.id.as_str().into(),
                name: p.name.as_str().into(),
                glyph: p.glyph.clamp(0, 5),
                detail: self.describe(p).into(),
                stats: stats(p).into(),
                pack: p.pack.as_str().into(),
                active: p.id == current.id,
            })
            .collect();
        self.models.profiles.set_vec(items);
        let mut options = vec![Command { id: "".into(), title: "All groups".into(), detail: "".into() }];
        options.extend(groups.iter().map(|g| Command { id: g.as_str().into(), title: g.as_str().into(), detail: "".into() }));
        ui.set_profile_groups(slint::ModelRc::from(&*options));
        ui.set_profile_group(self.group_filter.as_str().into());
        ui.set_profile_id(current.id.as_str().into());
        ui.set_profile_name(current.name.as_str().into());
        ui.set_profile_glyph(current.glyph.clamp(0, 5));
        let client = if self.info.client_game.is_empty() { crate::client::GAME.to_string() } else { self.info.client_game.clone() };
        ui.set_client_game(client.as_str().into());
        self.refresh_versions();
    }

    fn refresh_versions(&self) {
        let filter = &self.version_filter;
        let client = if self.info.client_game.is_empty() { crate::client::GAME } else { &self.info.client_game };
        let mut versions = Vec::new();
        if filter.query.is_empty() {
            versions.push(VersionItem { id: "".into(), title: "Automatic".into(), detail: format!("CxVisuals {client}, otherwise the latest release").into(), tag: "".into() });
        }
        versions.extend(self.info.versions.iter().filter(|v| filter.admits(v)).map(|v| VersionItem { id: v.id.as_str().into(), title: v.id.as_str().into(), detail: "".into(), tag: v.tag().into() }));
        self.models.versions.set_vec(versions);
    }

    pub fn add_client(&mut self) {
        let id = self.profile().id;
        let fits = |p: &Profile| p.game.trim().is_empty() || p.game == self.info.client_game;
        if !self.profiles.get(&id).is_some_and(fits) {
            self.toast(format!("CxVisuals is made for Minecraft {}. Pick that version first", self.info.client_game));
            self.edit_profile(Some(""));
            return;
        }
        if let Some(p) = self.profiles.get_mut(&id) {
            p.vanilla = false;
            p.client = true;
            if !p.loader().runs_client() {
                p.kind = Loader::Fabric.id().to_string();
            }
        }
        self.profiles.save(&self.core.paths);
        self.push_profiles();
        self.push_play();
        self.load_client_info();
        self.toast("CxVisuals will be added on the next start");
    }

    pub fn hide_client_hint(&mut self) {
        self.settings.hide_client_hint = true;
        self.save();
        self.push_play();
        self.push_settings();
    }

    pub fn filter_group(&mut self, group: &str) {
        self.group_filter = group.to_string();
        self.push_profiles();
    }

    pub fn filter_versions(&mut self, arg: &str) {
        self.version_filter = VersionFilter::parse(arg);
        self.refresh_versions();
    }

    fn needs_snapshots(&self, game: &str) -> bool {
        self.info.versions.iter().any(|v| v.id == game && v.kind != "release")
    }

    fn busy_with(&self, id: &str) -> bool {
        match &self.run {
            Run::Playing { profile, .. } => profile == id,
            Run::Preparing { .. } => self.profile().id == id,
            Run::Idle => false,
        }
    }

    pub fn switch_profile(&mut self, id: &str) {
        if self.profiles.get(id).is_none() || !matches!(self.run, Run::Idle) {
            return;
        }
        self.settings.profile = id.to_string();
        self.save();
        self.info.error.clear();
        self.models.results.set_vec(Vec::new());
        self.push_profiles();
        self.push_play();
        self.load_client_info();
        if self.page() == 2 {
            self.refresh_mods();
        }
    }

    pub fn play_profile(&mut self, id: &str) {
        self.switch_profile(id);
        if self.profile().id == id {
            self.go(0);
            self.play(false);
        }
    }

    pub fn edit_profile(&mut self, id: Option<&str>) {
        let Some(ui) = self.ui() else { return };
        let p = match id {
            None => Profile { glyph: (self.profiles.list.len() % 6) as i32, ..Default::default() },
            Some("") => self.profile(),
            Some(id) => self.profiles.get(id).cloned().unwrap_or_else(|| self.profile()),
        };
        let include_snapshots = p.include_snapshots || self.needs_snapshots(&p.game);
        self.version_filter = VersionFilter { snapshots: include_snapshots, query: String::new() };
        self.refresh_versions();
        ui.set_profile_edit(ProfileEdit {
            id: p.id.as_str().into(),
            name: p.name.as_str().into(),
            glyph: p.glyph,
            game: p.game.as_str().into(),
            vanilla: p.vanilla,
            kind: p.loader().id().into(),
            client: p.client,
            server: p.server.as_str().into(),
            loader: p.loader.as_str().into(),
            pack: p.pack.as_str().into(),
            java: p.java.as_str().into(),
            servers: slint::ModelRc::from(&*p.servers.iter().map(|s| Command { id: s.address.as_str().into(), title: if s.name.is_empty() { s.address.as_str().into() } else { s.name.as_str().into() }, detail: s.address.as_str().into() }).collect::<Vec<_>>()),
            include_snapshots,
            memory: if p.memory > 0 { p.memory.to_string() } else { String::new() }.into(),
            jvm_args: p.jvm_args.as_str().into(),
            env: p.env.as_str().into(),
            wrapper: p.wrapper.as_str().into(),
            pre_launch: p.pre_launch.as_str().into(),
            post_exit: p.post_exit.as_str().into(),
            notes: p.notes.as_str().into(),
            group: p.group.as_str().into(),
        });
        ui.set_profile_open(true);
        if !p.id.is_empty() {
            self.open_instance(&p.id);
        }
    }

    pub fn save_profile(&mut self, edit: ProfileEdit) {
        let fresh = edit.id.is_empty();
        let id = if fresh { self.profiles.add(Profile { name: edit.name.to_string(), ..Default::default() }).id.clone() } else { edit.id.to_string() };
        if let Some(p) = self.profiles.get_mut(&id) {
            let name = edit.name.trim();
            if !name.is_empty() {
                p.name = name.to_string();
            }
            p.glyph = edit.glyph.clamp(0, 5);
            p.game = edit.game.trim().to_string();
            p.vanilla = edit.vanilla;
            p.kind = Loader::parse(&edit.kind).id().to_string();
            p.client = edit.client;
            p.server = edit.server.trim().to_string();
            p.include_snapshots = edit.include_snapshots;
            p.loader = edit.loader.trim().to_string();
            p.java = edit.java.trim().to_string();
            p.memory = edit.memory.trim().parse::<u32>().map(|m| m.clamp(512, Machine::get().max_memory().max(1024))).unwrap_or(0);
            p.jvm_args = edit.jvm_args.trim().to_string();
            p.env = edit.env.trim().to_string();
            p.wrapper = edit.wrapper.trim().to_string();
            p.pre_launch = edit.pre_launch.trim().to_string();
            p.post_exit = edit.post_exit.trim().to_string();
            p.notes = edit.notes.trim().to_string();
            p.group = edit.group.trim().chars().take(24).collect();
            p.server = edit.server.trim().to_string();
            if !p.server.is_empty() && !p.servers.iter().any(|s| s.address == p.server) {
                p.servers.push(crate::store::Server { name: p.server.clone(), address: p.server.clone() });
            }
        }
        self.profiles.save(&self.core.paths);
        if let Some(ui) = self.ui() {
            ui.set_profile_open(false);
        }
        self.push_profiles();
        if fresh {
            self.toast("Profile created");
            self.switch_profile(&id);
        } else {
            self.push_play();
            self.load_client_info();
        }
    }

    pub fn use_fabric(&mut self) {
        let id = self.profile().id;
        if let Some(p) = self.profiles.get_mut(&id) {
            p.vanilla = false;
        }
        self.profiles.save(&self.core.paths);
        self.push_profiles();
        self.push_play();
        self.refresh_mods();
    }

    pub fn delete_profile(&mut self, id: &str) {
        if self.profiles.list.len() < 2 || self.busy_with(id) {
            self.toast("This profile is in use");
            return;
        }
        let Some(gone) = self.profiles.get(id).cloned() else { return };
        self.profiles.list.retain(|p| p.id != id);
        self.profiles.save(&self.core.paths);
        let dir = self.core.paths.profile(id);
        background(move || {
            let _ = std::fs::remove_dir_all(dir);
        });
        if let Some(ui) = self.ui() {
            ui.set_profile_open(false);
        }
        if self.settings.profile == id {
            let next = self.profiles.list[0].id.clone();
            self.switch_profile(&next);
        } else {
            self.push_profiles();
        }
        self.toast(format!("Deleted {}", gone.name));
    }

    pub fn open_profile_folder(&mut self, id: &str) {
        if self.profiles.get(id).is_some() {
            crate::sys::reveal(&self.core.paths.profile(id));
        }
    }

    pub fn duplicate_profile(&mut self, id: &str) {
        let Some(src) = self.profiles.get(id).cloned() else { return };
        let new_id = new_id();
        let mut copy = src.clone();
        copy.id = new_id.clone();
        copy.name = format!("{} (copy)", src.name);
        copy.created = now();
        copy.last_played = 0;
        copy.played = 0;
        self.profiles.add(copy);
        self.profiles.save(&self.core.paths);
        let core = self.core.clone();
        let src_id = src.id.clone();
        background(move || {
            copy_dir(&core.paths.profile(&src_id), &core.paths.profile(&new_id));
            later(move |app| {
                app.push_profiles();
                app.switch_profile(&new_id);
                app.toast(format!("Duplicated {}", src.name));
            });
        });
    }

    fn unpack<T: Send + 'static>(&mut self, title: String, source: impl FnOnce(&super::Core) -> Result<(PathBuf, T), String> + Send + 'static, done: impl FnOnce(&mut App, String, mrpack::Pack, T) + Send + 'static) {
        let core = self.core.clone();
        let id = new_id();
        let dir = core.paths.profile(&id);
        let threads = self.settings.threads as usize;
        self.toast(format!("Installing {title}…"));
        background(move || {
            let result = source(&core).and_then(|(file, meta)| mrpack::install(&core.http, &file, &dir, threads, &Progress::default()).map(|pack| (pack, meta)));
            if result.is_err() {
                let _ = std::fs::remove_dir_all(&dir);
            }
            later(move |app| {
                match result {
                    Ok((pack, meta)) => done(app, id, pack, meta),
                    Err(e) => app.toast(e),
                }
            });
        });
    }

    fn add_pack(&mut self, id: String, pack: mrpack::Pack, project_id: String, pack_version: String) {
        let label = format!("{} {}", pack.name, pack.version).trim().to_string();
        self.profiles.add(Profile {
            id: id.clone(),
            name: pack.name.clone(),
            game: pack.game,
            vanilla: pack.loader.is_none(),
            kind: pack.loader.as_ref().map(|(l, _)| l.id().to_string()).unwrap_or_default(),
            client: false,
            loader: pack.loader.map(|(_, v)| v).unwrap_or_default(),
            pack: label,
            pack_id: project_id,
            pack_version,
            glyph: (self.profiles.list.len() % 6) as i32,
            ..Default::default()
        });
        self.profiles.save(&self.core.paths);
        self.push_profiles();
        self.switch_profile(&id);
        self.toast(format!("{} is ready to play", pack.name));
    }

    pub fn install_pack(&mut self, project: &str) {
        if !self.begin_install(Kind::Pack, project) {
            return;
        }
        let title = self.hit_title(Kind::Pack, project);
        let project = project.to_string();
        let finished = project.clone();
        let project_id = project.clone();
        self.unpack(
            title,
            move |core| {
                let version = modrinth::pick(modrinth::pack_versions(&core.http, &core.ep, &project, None)?, None).ok_or("This modpack has no release")?;
                let pack_version = version.version_number.clone();
                let job = version.job(&core.paths.cache().join("packs")).ok_or("This modpack has no file to download")?;
                let file = job.path.clone();
                crate::download::run(&core.http, vec![job], 1, &Progress::default(), false)?;
                Ok((file, pack_version))
            },
            move |app, id, pack, version| {
                app.add_pack(id, pack, project_id, version);
                app.end_install(Kind::Pack, &finished);
            },
        );
    }

    pub fn update_pack(&mut self, id: &str) {
        let Some(profile) = self.profiles.get(id).cloned() else { return };
        if profile.pack_id.is_empty() {
            self.toast("This profile was not installed from Modrinth");
            return;
        }
        let core = self.core.clone();
        self.toast(format!("Checking updates for {}", profile.name));
        background(move || {
            let result = modrinth::pack_versions(&core.http, &core.ep, &profile.pack_id, Some(profile.loader().id())).and_then(|list| {
                let version = modrinth::pick(list, None).ok_or("No pack versions found")?;
                if version.version_number == profile.pack_version {
                    return Ok(None);
                }
                let job = version.job(&core.paths.cache().join("packs")).ok_or("Updated pack has no file")?;
                let file = job.path.clone();
                crate::download::run(&core.http, vec![job], 1, &Progress::default(), false)?;
                Ok(Some((file, version.version_number.clone())))
            });
            let dir = core.paths.profile(&profile.id);
            later(move |app| match result {
                Ok(None) => app.toast("Modpack is already up to date"),
                Ok(Some((file, version))) => {
                    let appdir = dir.clone();
                    background(move || {
                        let result = mrpack::install(&core.http, &file, &appdir, 8, &Progress::default()).map(|_| version);
                        later(move |app| match result {
                            Ok(version) => {
                                if let Some(p) = app.profiles.get_mut(&profile.id) {
                                    p.pack_version = version.clone();
                                    p.pack = format!("{} {}", p.name, version);
                                }
                                app.profiles.save(&app.core.paths);
                                app.push_profiles();
                                app.toast("Modpack updated");
                            }
                            Err(e) => app.toast(format!("Could not update modpack: {e}")),
                        });
                    });
                }
                Err(e) => app.toast(format!("Could not check updates: {e}")),
            });
        });
    }

    pub fn import_pack(&mut self) {
        let Some(file) = rfd::FileDialog::new().set_title("Import a modpack").add_filter("Modrinth modpack", &["mrpack", "zip"]).pick_file() else { return };
        match mrpack::read(&file) {
            Ok(pack) => self.unpack(pack.name.clone(), move |_| Ok((file, String::new())), |app, id, pack, _| app.add_pack(id, pack, String::new(), String::new())),
            Err(e) => self.toast(e),
        }
    }
}
