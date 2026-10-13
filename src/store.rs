use crate::paths::{load_json, save_json, Paths};
use crate::sys::{now, Machine};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: i32,
    pub accent: i32,
    pub effects: bool,
    pub reduce_motion: bool,
    pub memory: u32,
    pub java: String,
    pub jvm_args: String,
    pub collector: i32,
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    pub after_launch: i32,
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
            reduce_motion: false,
            memory: machine.recommended_memory(),
            java: String::new(),
            jvm_args: String::new(),
            collector: 0,
            width: 1280,
            height: 720,
            fullscreen: false,
            after_launch: 1,
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
        s
    }

    pub fn save(&self, paths: &Paths) {
        save_json(&paths.settings(), self);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub game: String,
    pub vanilla: bool,
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
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            game: String::new(),
            vanilla: false,
            client: true,
            loader: String::new(),
            pack: String::new(),
            pack_id: String::new(),
            pack_version: String::new(),
            server: String::new(),
            servers: Vec::new(),
            java: String::new(),
            glyph: 0,
            created: 0,
            last_played: 0,
            played: 0,
            include_snapshots: false,
        }
    }
}

impl Profile {
    pub fn uses_client(&self) -> bool {
        self.client && !self.vanilla
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
            p.list.push(Profile { id: "cxvisuals".into(), name: crate::client::NAME.into(), created: now(), ..Default::default() });
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
        assert!(p.list[0].client);
        let id = p.add(Profile { name: "  Survival ".into(), ..Default::default() }).id.clone();
        p.save(&paths);
        let again = Profiles::load(&paths);
        assert_eq!(again.get(&id).unwrap().name, "Survival");
        let _ = std::fs::remove_dir_all(dir);
    }
}
