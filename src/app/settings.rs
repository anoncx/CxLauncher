use super::{background, later, App};
use crate::mc::java;
use crate::sys::Machine;
use crate::store::Settings;
use crate::{AppWindow, Command, SettingsView, Theme};
use slint::{Color, ComponentHandle, ModelRc, VecModel};

const BACKGROUNDS: [(&str, &str); 7] = [
    ("Graphite", "#17181c"),
    ("Midnight", "#0a1020"),
    ("Forest", "#0b1712"),
    ("Plum", "#170e18"),
    ("Linen", "#efe7d8"),
    ("Mist", "#e6edf5"),
    ("Rose", "#f5e6e9"),
];

fn command(id: &str, title: &str, detail: &str) -> Command {
    Command { id: id.into(), title: title.into(), detail: detail.into() }
}

fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let digits = text.trim().trim_start_matches('#');
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&digits[i * 2..i * 2 + 2], 16).ok();
    Some([channel(0)?, channel(1)?, channel(2)?])
}

fn is_dark([r, g, b]: [u8; 3]) -> bool {
    let linear = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b) < 0.18
}

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

pub fn install_backgrounds(ui: &AppWindow) {
    let rgb = |hex: &str| parse_hex(hex).map(|[r, g, b]| Color::from_rgb_u8(r, g, b)).unwrap_or_default();
    let theme = ui.global::<Theme>();
    theme.set_backgrounds(ModelRc::new(VecModel::from_iter(BACKGROUNDS.iter().map(|(_, h)| rgb(h)))));
    theme.set_background_names(ModelRc::new(VecModel::from_iter(BACKGROUNDS.iter().map(|(n, _)| (*n).into()))));
}

pub fn apply_theme(ui: &AppWindow, s: &Settings) {
    let theme = ui.global::<Theme>();
    let paper = parse_hex(&s.background);
    theme.set_mode(s.theme);
    theme.set_accent_index(s.accent);
    theme.set_effects(s.effects);
    theme.set_calm(s.reduce_motion);
    theme.set_roundness_index(s.roundness);
    theme.set_has_paper(paper.is_some());
    if let Some(rgb @ [r, g, b]) = paper {
        theme.set_custom_paper(Color::from_rgb_u8(r, g, b));
        theme.set_custom_dark(is_dark(rgb));
    }
}

impl App {
    fn java_label(&self) -> String {
        if self.settings.java.is_empty() {
            return "Managed (recommended)".into();
        }
        self.javas
            .iter()
            .find(|j| j.java.to_string_lossy() == self.settings.java)
            .map(|j| format!("Java {}", j.version))
            .unwrap_or_else(|| "Custom Java".into())
    }

    pub fn push_settings(&mut self) {
        let Some(ui) = self.ui() else { return };
        let machine = Machine::get();
        let s = &self.settings;
        ui.set_settings(SettingsView {
            memory: s.memory as i32,
            memory_max: machine.max_memory().max(2048) as i32,
            memory_recommended: machine.recommended_memory() as i32,
            java: s.java.as_str().into(),
            java_label: self.java_label().into(),
            jvm_args: s.jvm_args.as_str().into(),
            collector: s.collector,
            width: s.width.to_string().into(),
            height: s.height.to_string().into(),
            fullscreen: s.fullscreen,
            discrete_gpu: s.discrete_gpu,
            after_launch: s.after_launch,
            threads: s.threads as i32,
            local: !s.local_client.is_empty(),
            local_jar: s.local_client.as_str().into(),
            data_dir: self.core.paths.root.to_string_lossy().as_ref().into(),
            version: concat!("v", env!("CARGO_PKG_VERSION")).into(),
            update: self.update.as_ref().map(|u| u.version.as_str()).unwrap_or_default().into(),
            update_busy: self.update_busy,
            client_hint: !s.hide_client_hint,
            beta_updates: s.beta_updates,
            show_online: !s.hide_online,
            background: s.background.as_str().into(),
            background_index: BACKGROUNDS.iter().position(|(_, h)| *h == s.background).map_or(-1, |i| i as i32),
            machine: format!("{:.0} GB RAM, {} cores", machine.memory_mb as f32 / 1024.0, machine.cores).into(),
        });
        let mut javas = vec![command("", "Managed by CxLauncher", "Downloads the Java Minecraft asks for")];
        javas.extend(self.javas.iter().map(|j| command(&j.java.to_string_lossy(), &format!("Java {}", j.version), &j.java.to_string_lossy())));
        javas.push(command("browse", "Choose a Java…", "Point to a java or javaw executable"));
        self.models.javas.set_vec(javas);
    }

    pub fn detect_javas(&mut self) {
        let paths = self.core.paths.clone();
        background(move || {
            let found = java::detect(&paths);
            later(move |app| {
                if app.javas != found {
                    app.javas = found;
                    app.push_settings();
                }
            });
        });
    }

    pub fn setting(&mut self, key: &str, value: &str) {
        let on = value == "1";
        let number = value.trim().parse::<i64>().ok();
        let ui = self.ui();
        let s = &mut self.settings;
        match key {
            "theme" => s.theme = number.unwrap_or(0).clamp(0, 2) as i32,
            "accent" => s.accent = number.unwrap_or(0).clamp(0, 6) as i32,
            "effects" => s.effects = on,
            "calm" => s.reduce_motion = on,
            "client-hint" => s.hide_client_hint = !on,
            "beta-updates" => s.beta_updates = on,
            "show-online" => {
                s.hide_online = !on;
                if !on {
                    self.info.online = -1;
                }
            }
            "roundness" => s.roundness = number.unwrap_or(2).clamp(0, 3) as i32,
            "background-preset" => s.background = number.and_then(|i| BACKGROUNDS.get(i as usize)).map(|(_, h)| h.to_string()).unwrap_or_default(),
            "background" if value.trim().is_empty() => s.background.clear(),
            "background" => match parse_hex(value) {
                Some(rgb) => s.background = hex(rgb),
                None => return,
            },
            "memory" => s.memory = number.unwrap_or(s.memory as i64).clamp(1024, Machine::get().max_memory().max(1024) as i64) as u32,
            "width" => s.width = number.unwrap_or(0).clamp(0, 16384) as u32,
            "height" => s.height = number.unwrap_or(0).clamp(0, 16384) as u32,
            "fullscreen" => s.fullscreen = on,
            "discrete-gpu" => s.discrete_gpu = on,
            "collector" => s.collector = number.unwrap_or(0).clamp(0, 1) as i32,
            "jvm-args" => s.jvm_args = value.to_string(),
            "after-launch" => s.after_launch = number.unwrap_or(0).clamp(0, 1) as i32,
            "threads" => s.threads = number.unwrap_or(16).clamp(2, 64) as u32,
            "java" if value == "browse" => return self.pick_java(),
            "java" => s.java = value.to_string(),
            "local" if on => return self.pick_jar(),
            "local" => {
                s.local_client.clear();
                self.load_client_info();
            }
            _ => return,
        }
        if let Some(ui) = &ui {
            apply_theme(ui, &self.settings);
        }
        self.save();
        if !matches!(key, "width" | "height" | "jvm-args") {
            self.push_settings();
        }
    }

    fn pick_java(&mut self) {
        let exe = if cfg!(windows) { vec!["exe"] } else { Vec::new() };
        let mut dialog = rfd::FileDialog::new().set_title("Choose a Java executable");
        if !exe.is_empty() {
            dialog = dialog.add_filter("Java", &exe);
        }
        if let Some(path) = dialog.pick_file() {
            match java::probe(&path) {
                Some(found) => {
                    self.settings.java = path.to_string_lossy().into_owned();
                    if !self.javas.contains(&found) {
                        self.javas.push(found);
                    }
                    self.save();
                }
                None => self.toast("That is not a Java executable"),
            }
        }
        self.push_settings();
    }

    pub fn pick_jar(&mut self) {
        let picked = rfd::FileDialog::new().set_title("Choose a CxVisuals build").add_filter("Mod jar", &["jar"]).pick_file();
        if let Some(path) = picked {
            match crate::client::from_jar(path.clone()) {
                Ok(c) => {
                    self.settings.local_client = path.to_string_lossy().into_owned();
                    self.save();
                    self.toast(format!("Using local {} {}", c.name, c.version));
                    self.load_client_info();
                }
                Err(e) => self.toast(e),
            }
        }
        self.push_settings();
    }
}
