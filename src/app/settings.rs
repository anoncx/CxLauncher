use super::{background, later, App};
use crate::mc::java;
use crate::sys::Machine;
use crate::{Command, SettingsView, Theme};
use slint::ComponentHandle;

fn command(id: &str, title: &str, detail: &str) -> Command {
    Command { id: id.into(), title: title.into(), detail: detail.into() }
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
            after_launch: s.after_launch,
            threads: s.threads as i32,
            local: !s.local_client.is_empty(),
            local_jar: s.local_client.as_str().into(),
            data_dir: self.core.paths.root.to_string_lossy().as_ref().into(),
            version: concat!("v", env!("CARGO_PKG_VERSION")).into(),
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
        let theme = ui.as_ref().map(|ui| ui.global::<Theme>());
        let s = &mut self.settings;
        match key {
            "theme" => {
                s.theme = number.unwrap_or(0).clamp(0, 2) as i32;
                if let Some(t) = &theme {
                    t.set_mode(s.theme);
                }
            }
            "accent" => {
                s.accent = number.unwrap_or(0).clamp(0, 6) as i32;
                if let Some(t) = &theme {
                    t.set_accent_index(s.accent);
                }
            }
            "effects" => {
                s.effects = on;
                if let Some(t) = &theme {
                    t.set_effects(on);
                }
            }
            "calm" => {
                s.reduce_motion = on;
                if let Some(t) = &theme {
                    t.set_calm(on);
                }
            }
            "memory" => s.memory = number.unwrap_or(s.memory as i64).clamp(1024, Machine::get().max_memory().max(1024) as i64) as u32,
            "width" => s.width = number.unwrap_or(0).clamp(0, 16384) as u32,
            "height" => s.height = number.unwrap_or(0).clamp(0, 16384) as u32,
            "fullscreen" => s.fullscreen = on,
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
