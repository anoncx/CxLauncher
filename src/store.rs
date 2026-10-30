use crate::mc::loader::Loader;
use crate::paths::{load_json, save_json, Paths};
use crate::sys::{now, Machine};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: i32,
    pub accent: i32,
    pub effects: bool,
    pub background: String,
    pub roundness: i32,
    pub reduce_motion: bool,
    pub hide_client_hint: bool,
    pub beta_updates: bool,
    pub memory: u32,
    pub java: String,
    pub jvm_args: String,
    pub collector: i32,
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    pub discrete_gpu: bool,
    pub after_launch: i32,
    pub import_prompted: bool,
    pub threads: u32,
    pub local_client: String,
    pub profile: String,
    pub account: String,
}

impl Default for Settings {
    fn default() -> Self {
        let machine = Machine::get();
        Self {
            theme: 0,
            accent: 0,
            effects: !machine.low_end(),
            background: String::new(),
            roundness: 2,
            reduce_motion: false,
            hide_client_hint: false,
            beta_updates: false,
            memory: machine.recommended_memory(),
            java: String::new(),
            jvm_args: String::new(),
            collector: 0,
            width: 1280,
            height: 720,
            fullscreen: false,
            discrete_gpu: false,
            after_launch: 1,
            import_prompted: false,
            threads: machine.download_threads() as u32,
            local_client: String::new(),
            profile: String::new(),
            account: String::new(),
        }
    }
}

impl Settings {
    pub fn load(paths: &Paths) -> Self {
        let mut s: Settings = load_json(&paths.settings());
        let machine = Machine::get();
        s.memory = s.memory.clamp(1024, machine.max_memory().max(1024));
        s.threads = s.threads.clamp(2, 64);
        s.theme = s.theme.clamp(0, 2);
        s.accent = s.accent.clamp(0, 6);
        s.roundness = s.roundness.clamp(0, 3);
        s
    }

    pub fn save(&self, paths: &Paths) {
        save_json(&paths.settings(), self);
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub game: String,
    pub vanilla: bool,
    pub kind: String,
    pub client: bool,
    pub loader: String,
    pub pack: String,
    pub pack_id: String,
    pub pack_version: String,
    pub server: String,
    pub servers: Vec<Server>,
    pub java: String,
    pub glyph: i32,
    pub created: u64,
    pub last_played: u64,
    pub played: u64,
    pub include_snapshots: bool,
    pub source: String,
    pub memory: u32,
    pub jvm_args: String,
    pub env: String,
    pub wrapper: String,
    pub pre_launch: String,
    pub post_exit: String,
    pub notes: String,
    pub group: String,
}

impl Profile {
    pub fn loader(&self) -> Loader {
        Loader::parse(&self.kind)
    }

    pub fn uses_client(&self) -> bool {
        self.client && !self.vanilla && self.loader().runs_client()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Server {
    pub name: String,
    pub address: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Profiles {
    pub list: Vec<Profile>,
}

pub fn new_id() -> String {
    (0..10).map(|_| fastrand::alphanumeric().to_ascii_lowercase()).collect()
}

impl Profiles {
    pub fn load(paths: &Paths) -> Self {
        let mut p: Profiles = load_json(&paths.profiles_file());
        p.list.retain(|p| !p.id.is_empty() && p.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        if p.list.is_empty() {
            p.list.push(Profile { id: "minecraft".into(), name: "Minecraft".into(), vanilla: true, created: now(), ..Default::default() });
        }
        p
    }

    pub fn save(&self, paths: &Paths) {
        save_json(&paths.profiles_file(), self);
    }

    pub fn get(&self, id: &str) -> Option<&Profile> {
        self.list.iter().find(|p| p.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Profile> {
        self.list.iter_mut().find(|p| p.id == id)
    }

    pub fn add(&mut self, mut profile: Profile) -> &Profile {
        let name = profile.name.trim().to_string();
        if profile.id.is_empty() {
            profile.id = new_id();
        }
        profile.name = if name.is_empty() { format!("Profile {}", self.list.len() + 1) } else { name };
        profile.created = now();
        self.list.push(profile);
        self.list.last().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_default_profile() {
        let dir = std::env::temp_dir().join(format!("cxl-store-{}", fastrand::u64(..)));
        let paths = Paths::new(dir.clone());
        let mut p = Profiles::load(&paths);
        assert_eq!(p.list.len(), 1);
        assert!(!p.list[0].client);
        let id = p.add(Profile { name: "  Survival ".into(), ..Default::default() }).id.clone();
        p.save(&paths);
        let again = Profiles::load(&paths);
        assert_eq!(again.get(&id).unwrap().name, "Survival");
        let _ = std::fs::remove_dir_all(dir);
    }
}
