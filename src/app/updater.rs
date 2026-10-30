use super::{background, later, App};
use crate::update::{self, Method, Release};

impl App {
    pub fn check_update(&mut self) {
        update::cleanup();
        let core = self.core.clone();
        background(move || {
            if let Ok(Some(release)) = update::check(&core.http, &core.ep) {
                later(move |app| app.update_found(release));
            }
        });
    }

    fn update_found(&mut self, release: Release) {
        self.toast(format!("CxLauncher {} is available, update it in Settings", release.version));
        self.update = Some(release);
        self.push_settings();
    }

    pub fn install_update(&mut self) {
        let Some(release) = self.update.clone() else { return };
        if self.update_busy {
            return;
        }
        if release.method == Method::Page {
            crate::sys::open_url(&release.page);
            return;
        }
        self.update_busy = true;
        self.push_settings();
        let core = self.core.clone();
        background(move || {
            let result = update::install(&core.http, &release, &std::env::temp_dir());
            later(move |app| app.update_installed(result));
        });
    }

    fn update_installed(&mut self, result: Result<std::path::PathBuf, String>) {
        self.update_busy = false;
        match result.and_then(|path| std::process::Command::new(&path).spawn().map(|_| ()).map_err(|e| format!("cannot start {}: {e}", path.display()))) {
            Ok(()) => {
                self.save();
                let _ = slint::quit_event_loop();
            }
            Err(e) => {
                self.toast(format!("Update failed: {e}"));
                self.push_settings();
            }
        }
    }
}
