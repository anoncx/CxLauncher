use super::{background, later, App};
use crate::mc::importer::{self, Found};
use crate::store::{new_id, Profile};
use crate::ImportItem;
use std::collections::HashSet;

#[derive(Default)]
pub struct Import {
    found: Vec<Found>,
    picked: Vec<bool>,
    busy: bool,
}

fn describe(f: &Found) -> String {
    let game = if f.game.is_empty() { "latest".to_string() } else { f.game.clone() };
    let loader = f.loader.as_ref().map(|(l, _)| l.label()).unwrap_or("Vanilla");
    let plural = |n: usize, one: &str| if n == 1 { format!("{n} {one}") } else { format!("{n} {one}s") };
    format!("{}  ·  Minecraft {game}  ·  {loader}  ·  {}  ·  {}", f.launcher, plural(f.mods, "mod"), plural(f.worlds, "world"))
}

impl App {
    pub fn import_check(&mut self) {
        if !self.settings.import_prompted {
            self.import_scan(false);
        }
    }

    pub fn import_open(&mut self) {
        self.import_scan(true);
    }

    fn import_scan(&mut self, manual: bool) {
        if self.import.busy {
            return;
        }
        let known: HashSet<String> = self.profiles.list.iter().map(|p| p.source.clone()).filter(|s| !s.is_empty()).collect();
        background(move || {
            let found: Vec<Found> = importer::scan().into_iter().filter(|f| !known.contains(&f.key)).collect();
            later(move |app| app.import_found(found, manual));
        });
    }

    fn import_found(&mut self, found: Vec<Found>, manual: bool) {
        if self.import.busy {
            return;
        }
        if found.is_empty() {
            if manual {
                self.toast("No instances from other launchers found");
            }
            return;
        }
        self.import.picked = vec![true; found.len()];
        self.import.found = found;
        self.push_imports();
        if let Some(ui) = self.ui() {
            ui.set_import_open(true);
        }
    }

    fn push_imports(&self) {
        let items: Vec<ImportItem> = self
            .import
            .found
            .iter()
            .zip(&self.import.picked)
            .map(|(f, &picked)| ImportItem { name: f.name.as_str().into(), detail: describe(f).into(), picked })
            .collect();
        self.models.imports.set_vec(items);
        if let Some(ui) = self.ui() {
            ui.set_import_picked(self.import.picked.iter().filter(|p| **p).count() as i32);
            ui.set_import_busy(self.import.busy);
        }
    }

    pub fn import_toggle(&mut self, index: &str) {
        if self.import.busy {
            return;
        }
        if let Some(p) = index.parse::<usize>().ok().and_then(|i| self.import.picked.get_mut(i)) {
            *p = !*p;
            self.push_imports();
        }
    }

    pub fn import_set_all(&mut self, on: bool) {
        if !self.import.busy {
            self.import.picked.iter_mut().for_each(|p| *p = on);
            self.push_imports();
        }
    }

    pub fn import_close(&mut self) {
        if self.import.busy {
            return;
        }
        self.settings.import_prompted = true;
        self.save();
        self.import = Default::default();
        if let Some(ui) = self.ui() {
            ui.set_import_open(false);
        }
    }

    pub fn import_run(&mut self) {
        if self.import.busy {
            return;
        }
        let jobs: Vec<(Found, String)> = self.import.found.iter().zip(&self.import.picked).filter(|(_, &p)| p).map(|(f, _)| (f.clone(), new_id())).collect();
        if jobs.is_empty() {
            return;
        }
        self.import.busy = true;
        self.push_imports();
        let core = self.core.clone();
        background(move || {
            let mut done = Vec::new();
            let mut failed = Vec::new();
            for (found, id) in jobs {
                let dest = core.paths.profile(&id);
                match importer::copy_instance(&found, &dest) {
                    Ok(()) => done.push((found, id)),
                    Err(e) => {
                        let _ = std::fs::remove_dir_all(&dest);
                        failed.push(e);
                    }
                }
            }
            later(move |app| app.import_done(done, failed));
        });
    }

    fn import_done(&mut self, done: Vec<(Found, String)>, failed: Vec<String>) {
        let total = done.len();
        for (found, id) in done {
            let (kind, loader) = found.loader.as_ref().map(|(l, v)| (l.id().to_string(), v.clone())).unwrap_or_default();
            self.profiles.add(Profile {
                id,
                name: found.name.clone(),
                game: found.game.clone(),
                vanilla: found.loader.is_none(),
                kind,
                loader,
                client: false,
                glyph: (self.profiles.list.len() % 6) as i32,
                source: found.key.clone(),
                ..Default::default()
            });
        }
        self.profiles.save(&self.core.paths);
        self.settings.import_prompted = true;
        self.save();
        self.import = Default::default();
        if let Some(ui) = self.ui() {
            ui.set_import_open(false);
        }
        self.push_profiles();
        match (total, failed.first()) {
            (0, Some(e)) => self.toast(e.clone()),
            (n, Some(_)) => self.toast(format!("Imported {n}, {} failed", failed.len())),
            (1, None) => self.toast("Imported 1 profile"),
            (n, None) => self.toast(format!("Imported {n} profiles")),
        }
    }
}
