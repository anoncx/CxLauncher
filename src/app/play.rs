use super::{background, later, options, profiles, App};
use crate::auth::{self, Account};
use crate::client;
use crate::download::Progress;
use crate::game::{self, Game, Options, Session};
use crate::install::{self, Prepared, Request, Stage};
use crate::mc::fabric;
use crate::mc::loader::Loader;
use crate::sys::{duration, megabytes, now};
use slint::{ComponentHandle, SharedString};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub enum Run {
    Idle,
    Preparing { progress: Arc<Progress>, stage: Stage, repair: bool },
    Playing { game: Game, profile: String, started: Instant, stopping: bool, shown_secs: u64, ready: bool },
}

#[derive(Default)]
pub struct Info {
    pub name: String,
    pub version: String,
    pub client_game: String,
    pub latest: String,
    pub releases: Vec<String>,
    pub versions: Vec<profiles::VersionEntry>,
    pub loaders: HashMap<String, String>,
    pub notes: Vec<String>,
    pub error: String,
    pub online: i32,
}

fn clock(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, secs % 3600 / 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

impl App {
    pub fn active_account(&self) -> Option<Account> {
        self.accounts.get(&self.settings.account).or(self.accounts.list.first()).cloned()
    }

    pub fn play(&mut self, repair: bool) {
        if !matches!(self.run, Run::Idle) {
            return;
        }
        let account = self.active_account();
        if account.is_none() && !repair {
            self.signin_start();
            return;
        }
        let core = self.core.clone();
        let profile = self.profile();
        let local = Some(self.settings.local_client.clone()).filter(|s| !s.is_empty()).map(PathBuf::from);
        let java = Some(if profile.java.trim().is_empty() { self.settings.java.clone() } else { profile.java.clone() })
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        let threads = self.settings.threads as usize;
        let progress = Arc::new(Progress::default());
        self.info.error.clear();
        self.run = Run::Preparing { progress: progress.clone(), stage: Stage::Checking, repair };
        self.start_ticker();
        self.push_play();

        background(move || {
            let account = match account {
                Some(a) if !repair && !auth::fresh(&a) => match auth::renew(&core.http, &core.ep, &a) {
                    Ok(renewed) => {
                        let saved = renewed.clone();
                        later(move |app| app.store_account(saved, false));
                        Some(renewed)
                    }
                    Err(e) if crate::net::is_offline(&e) => Some(a),
                    Err(e) => {
                        later(move |app| app.prepared(Err(e), None));
                        return;
                    }
                },
                other => other,
            };
            let request = Request {
                http: &core.http,
                ep: &core.ep,
                paths: &core.paths,
                profile: &profile,
                local_client: local,
                java,
                threads,
                verify: repair,
            };
            let result = install::prepare(&request, &progress, &|stage| later(move |app| app.on_stage(stage)));
            later(move |app| app.prepared(result, account));
        });
    }

    fn on_stage(&mut self, next: Stage) {
        if let Run::Preparing { stage, .. } = &mut self.run {
            *stage = next;
        }
        self.push_play();
    }

    fn prepared(&mut self, result: Result<Prepared, String>, account: Option<Account>) {
        let repair = matches!(self.run, Run::Preparing { repair: true, .. });
        self.run = Run::Idle;
        let prepared = match result {
            Ok(p) => p,
            Err(e) => {
                if e != "cancelled" {
                    self.info.error = e;
                }
                self.push_play();
                return;
            }
        };
        if let Some(c) = &prepared.client {
            self.info.version = c.version.clone();
            self.info.name = c.name.clone();
        }
        if let Some(loader) = prepared.version.id.strip_prefix("fabric-loader-").and_then(|r| r.split('-').next()) {
            self.info.loaders.insert(prepared.game.clone(), loader.to_string());
        }
        if repair {
            self.toast("Game files checked and repaired");
            self.push_play();
            return;
        }
        let Some(account) = account else {
            self.push_play();
            return;
        };
        let profile = self.profile();
        let session = Session { name: account.name.clone(), uuid: account.uuid.clone(), token: account.token.clone(), xuid: String::new(), offline: account.offline };
        let options = Options {
            memory: self.settings.memory,
            collector: self.settings.collector,
            jvm_args: self.settings.jvm_args.clone(),
            width: self.settings.width,
            height: self.settings.height,
            fullscreen: self.settings.fullscreen,
            server: profile.server.clone(),
        };
        let args = game::arguments(&prepared, &session, &options);
        self.console.clear();
        let shown = crate::mc::args::redact(&args).join(" ");
        self.console.push(game::Level::Debug, format!("{} {shown}", prepared.java.display()));
        self.console.push(
            game::Level::Debug,
            format!(
                "CxLauncher {} · Minecraft {} · Java {} · {}",
                env!("CARGO_PKG_VERSION"),
                prepared.game,
                prepared.java_major,
                prepared.client.as_ref().map(|c| format!("{} {}", c.name, c.version)).unwrap_or_else(|| self.loader_label(&profile, &prepared.game))
            ),
        );
        let _ = options::skip_onboarding(&prepared.game_dir);
        let _discrete_gpu = game::DiscreteGpuGuard::apply(self.settings.discrete_gpu);
        match Game::spawn(&prepared.java, &args, &prepared.game_dir) {
            Ok(game) => {
                if let Some(p) = self.profiles.get_mut(&profile.id) {
                    p.last_played = now();
                }
                self.profiles.save(&self.core.paths);
                self.run = Run::Playing { game, profile: profile.id, started: Instant::now(), stopping: false, shown_secs: u64::MAX, ready: false };
                self.start_ticker();
            }
            Err(e) => self.info.error = e,
        }
        self.push_profiles();
        self.push_play();
        self.push_log();
    }

    pub fn cancel(&mut self) {
        match &self.run {
            Run::Preparing { progress, .. } => progress.cancel.store(true, Ordering::Relaxed),
            Run::Playing { .. } => self.stop(),
            Run::Idle => {}
        }
    }

    pub fn stop(&mut self) {
        if let Run::Playing { game, stopping, .. } = &mut self.run {
            *stopping = true;
            game.kill();
        }
    }

    fn start_ticker(&mut self) {
        if self.ticker.running() {
            return;
        }
        self.ticker.start(slint::TimerMode::Repeated, Duration::from_millis(100), || later(|app| app.tick()));
    }

    fn tick(&mut self) {
        match &mut self.run {
            Run::Idle => self.ticker.stop(),
            Run::Preparing { .. } => self.push_progress(),
            Run::Playing { game, started, shown_secs, ready, .. } => {
                let lines = game.drain();
                let ended = game.exited();
                let secs = started.elapsed().as_secs();
                let tick_clock = secs != *shown_secs;
                *shown_secs = secs;
                let opened = !*ready && (secs >= 25 || lines.iter().any(|(_, l)| game::window_ready(l)));
                if opened {
                    *ready = true;
                }
                if opened {
                    self.game_opened();
                }
                if !lines.is_empty() {
                    for (level, text) in lines {
                        self.console.push(level, text);
                    }
                    self.log_changed();
                }
                if let Some(code) = ended {
                    self.ended(code);
                } else if tick_clock {
                    if let Some(ui) = self.ui() {
                        ui.set_time(clock(secs).into());
                    }
                }
            }
        }
    }

    fn game_opened(&mut self) {
        if self.settings.after_launch == 1 {
            if let Some(ui) = self.ui() {
                ui.window().set_minimized(true);
            }
        }
        self.push_play();
    }

    fn ended(&mut self, code: i32) {
        let Run::Playing { profile, started, stopping, .. } = std::mem::replace(&mut self.run, Run::Idle) else { return };
        if let Some(p) = self.profiles.get_mut(&profile) {
            p.played += started.elapsed().as_secs();
        }
        self.profiles.save(&self.core.paths);
        self.console.push(game::Level::Debug, format!("Minecraft closed with code {code}"));
        self.log_changed();
        if let Some(ui) = self.ui() {
            ui.window().set_minimized(false);
        }
        if code != 0 && !stopping {
            self.info.error = format!("Minecraft closed with an error (code {code}). The console has the details.");
            self.go(3);
        }
        self.push_profiles();
        self.push_play();
    }

    fn push_progress(&self) {
        let Run::Preparing { progress, stage, .. } = &self.run else { return };
        let Some(ui) = self.ui() else { return };
        let detail = match stage {
            Stage::Checking => "Checking for updates".to_string(),
            Stage::Starting => "Starting Minecraft".to_string(),
            Stage::Downloading => {
                let done = progress.done_bytes.load(Ordering::Relaxed);
                let total = progress.total_bytes.load(Ordering::Relaxed);
                let current = progress.current_label();
                let base = if total == 0 { "Checking files".to_string() } else { format!("{} of {}", megabytes(done), megabytes(total)) };
                if current.is_empty() { base } else { format!("{base}  ·  {current}") }
            }
        };
        ui.set_progress(progress.fraction());
        ui.set_detail(detail.into());
    }

    pub fn push_play(&mut self) {
        let Some(ui) = self.ui() else { return };
        let profile = self.profile();
        let state = match &self.run {
            Run::Idle => 0,
            Run::Preparing { stage: Stage::Checking, .. } => 1,
            Run::Preparing { stage: Stage::Downloading, .. } => 2,
            Run::Preparing { stage: Stage::Starting, .. } => 3,
            Run::Playing { ready: false, .. } => 3,
            Run::Playing { .. } => 4,
        };
        ui.set_state(state);
        match &self.run {
            Run::Preparing { .. } => self.push_progress(),
            Run::Playing { ready, .. } => ui.set_detail(if *ready { SharedString::default() } else { "Starting Minecraft".into() }),
            Run::Idle => {
                ui.set_progress(0.0);
                ui.set_detail(SharedString::default());
                ui.set_time(SharedString::default());
            }
        }
        ui.set_error(self.info.error.clone().into());
        ui.set_client_on(profile.uses_client());
        ui.set_client_name(if self.info.name.is_empty() { client::NAME.into() } else { self.info.name.clone().into() });
        ui.set_client_version(self.info.version.clone().into());
        let game = self.game_of(&profile);
        ui.set_loader_label(self.loader_label(&profile, &game).into());
        ui.set_loader_name(if profile.vanilla { "Vanilla" } else { profile.loader().label() }.into());
        ui.set_game_version(game.into());
        ui.set_mod_count(crate::install::jars(&crate::install::mods_dir(&self.core.paths, &profile.id)).len() as i32);
        ui.set_online(self.info.online);
        ui.set_played(if profile.played == 0 { "0m".into() } else { duration(profile.played).into() });
        ui.set_last_played(crate::sys::ago(profile.last_played, now()).into());
        let notes = if profile.uses_client() { self.info.notes.iter().map(SharedString::from).collect() } else { Vec::new() };
        self.models.notes.set_vec(notes);
        let status = match &self.run {
            Run::Playing { game, .. } => format!("Running  ·  pid {}", game.pid()),
            _ if self.console.is_empty() => "Not running".to_string(),
            _ => "Not running  ·  output of the last session".to_string(),
        };
        ui.set_log_status(status.into());
    }

    pub fn load_client_info(&mut self) {
        let core = self.core.clone();
        let local = self.settings.local_client.clone();
        let wanted = Some(self.profile().game.trim().to_string()).filter(|g| !g.is_empty()).unwrap_or_else(|| client::GAME.to_string());
        background(move || {
            let found = if local.is_empty() {
                client::manifest(&core.http, &core.ep, &core.paths, &wanted).map(|m| (m.name, m.version, m.minecraft, m.loader, m.notes))
            } else {
                client::from_jar(PathBuf::from(&local)).map(|c| (c.name, c.version, c.minecraft, c.loader, vec!["Local build".to_string()]))
            };
            let (name, version, game, minimum, notes) = found.unwrap_or_default();
            let game = if game.is_empty() { client::GAME.to_string() } else { game };
            let loader = fabric::latest_loader(&core.http, &core.ep, &core.paths, Loader::Fabric, &game, minimum.as_deref()).unwrap_or_default();
            let manifest = crate::mc::meta::manifest(&core.http, &core.ep, &core.paths).unwrap_or_default();
            let releases: Vec<String> = manifest.releases().map(|v| v.id.clone()).collect();
            let versions: Vec<profiles::VersionEntry> = manifest.versions.iter().map(|v| profiles::VersionEntry { id: v.id.clone(), kind: v.kind.clone() }).collect();
            let latest = manifest.latest.release.clone();
            later(move |app| {
                app.info.name = name;
                app.info.version = version;
                app.info.client_game = game.clone();
                if !loader.is_empty() {
                    app.info.loaders.insert(game, loader);
                }
                app.info.latest = latest;
                app.info.releases = releases;
                app.info.versions = versions;
                app.info.notes = notes;
                app.push_play();
                app.push_profiles();
            });
        });
    }

    pub fn game_of(&self, profile: &crate::store::Profile) -> String {
        let pick = |v: &str| Some(v.trim().to_string()).filter(|v| !v.is_empty());
        pick(&profile.game)
            .or_else(|| if profile.uses_client() { pick(&self.info.client_game) } else { pick(&self.info.latest) })
            .unwrap_or_else(|| client::GAME.into())
    }

    pub fn loader_label(&self, profile: &crate::store::Profile, game: &str) -> String {
        if profile.vanilla {
            return "Vanilla".into();
        }
        let loader = profile.loader();
        let known = (loader == Loader::Fabric).then(|| self.info.loaders.get(game).cloned()).flatten();
        match Some(profile.loader.trim().to_string()).filter(|l| !l.is_empty()).or(known) {
            Some(v) => format!("{} {v}", loader.label()),
            None => loader.label().into(),
        }
    }

    pub fn poll_online(&mut self) {
        let core = self.core.clone();
        background(move || {
            let online = core
                .http
                .json::<serde_json::Value>(&format!("{}/v2/status", core.ep.cloud.trim_end_matches('/')))
                .ok()
                .and_then(|v| v.get("online").and_then(|o| o.as_i64()))
                .map(|o| o.clamp(0, 99_999) as i32)
                .unwrap_or(-1);
            later(move |app| {
                if app.info.online != online {
                    app.info.online = online;
                    if let Some(ui) = app.ui() {
                        ui.set_online(online);
                    }
                }
            });
        });
    }

    pub fn shutdown(&mut self) {
        self.ticker.stop();
        self.save();
    }
}
