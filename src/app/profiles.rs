use super::finder::Kind;
use super::play::Run;
use super::{background, later, App};
use crate::download::Progress;
use crate::mc::{modrinth, mrpack};
use crate::store::{new_id, Profile};
use crate::sys::{ago, duration, now};
use crate::{Command, ProfileEdit, ProfileItem};
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

impl App {
    fn describe(&self, p: &Profile) -> String {
        let game = self.game_of(p);
        let loader = if p.vanilla { "Vanilla" } else { "Fabric" };
        let extra = if p.uses_client() { "  ·  CxVisuals" } else { "" };
        format!("Minecraft {game}  ·  {loader}{extra}")
    }

    pub fn push_profiles(&mut self) {
        let Some(ui) = self.ui() else { return };
        let current = self.profile();
        let items: Vec<ProfileItem> = self
            .profiles
            .list
            .iter()
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
        ui.set_profile_id(current.id.as_str().into());
        ui.set_profile_name(current.name.as_str().into());
        ui.set_profile_glyph(current.glyph.clamp(0, 5));
        let client = if self.info.client_game.is_empty() { crate::client::GAME.to_string() } else { self.info.client_game.clone() };
        ui.set_client_game(client.as_str().into());
        let mut versions = vec![Command { id: "".into(), title: "Automatic".into(), detail: format!("CxVisuals {client}, otherwise the latest release").into() }];
        versions.extend(self.info.versions.iter().take(120).map(|v| Command { id: v.as_str().into(), title: v.as_str().into(), detail: Default::default() }));
        self.models.versions.set_vec(versions);
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
        ui.set_profile_edit(ProfileEdit {
            id: p.id.as_str().into(),
            name: p.name.as_str().into(),
            glyph: p.glyph,
            game: p.game.as_str().into(),
            vanilla: p.vanilla,
            client: p.client,
            server: p.server.as_str().into(),
            loader: p.loader.as_str().into(),
            pack: p.pack.as_str().into(),
            java: p.java.as_str().into(),
            servers: slint::ModelRc::from(&*p.servers.iter().map(|s| Command { id: s.address.as_str().into(), title: if s.name.is_empty() { s.address.as_str().into() } else { s.name.as_str().into() }, detail: s.address.as_str().into() }).collect::<Vec<_>>()),
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
            p.client = edit.client;
            p.server = edit.server.trim().to_string();
            p.loader = edit.loader.trim().to_string();
            p.java = edit.java.trim().to_string();
            p.server = edit.server.trim().to_string();
            if !p.server.is_empty() && !p.servers.iter().any(|s| s.address == p.server) {
                p.servers.push(crate::store::Server { name: p.server.clone(), address: p.server.clone() });
            }
        }
        self.profiles.save(&self.core.paths);
        if let Some(ui) = self.ui() {
            ui.set_profile_open(false);
        }
        if fresh {
            self.toast("Profile created");
            self.switch_profile(&id);
        } else {
            self.push_profiles();
            self.push_play();
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
            vanilla: pack.fabric.is_none(),
            client: false,
            loader: pack.fabric.unwrap_or_default(),
            pack: label,
            pack_id: project_id,
            pack_version,
            glyph: (self.profiles.list.len() % 6) as i32,
            ..Default::default()
        });
        self.profiles.save(&self.core.paths);
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
                let version = modrinth::pick(modrinth::pack_versions(&core.http, &core.ep, &project)?, None).ok_or("This modpack has no Fabric release")?;
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
            let result = modrinth::pack_versions(&core.http, &core.ep, &profile.pack_id).and_then(|list| {
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
