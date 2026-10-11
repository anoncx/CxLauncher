use super::images::thumb;
use super::{background, later, App};
use crate::sys::{ago, megabytes, now};
use crate::{ShotItem, WorldItem};
use flate2::read::GzDecoder;
use slint::Image;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .map(|e| {
            let p = e.path();
            if p.is_dir() { dir_size(&p) } else { e.metadata().map(|m| m.len()).unwrap_or(0) }
        })
        .sum()
}

fn stamp(p: &Path) -> u64 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
}

fn world_name(level: &Path) -> Option<String> {
    let raw = std::fs::read(level).ok()?;
    let mut data = Vec::new();
    GzDecoder::new(&raw[..]).read_to_end(&mut data).ok()?;
    let marker = b"LevelName";
    let pos = data.windows(marker.len()).position(|w| w == marker)?;
    let at = pos + marker.len();
    let len = u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?) as usize;
    let name = String::from_utf8_lossy(data.get(at + 2..at + 2 + len)?).trim().to_string();
    (!name.is_empty()).then_some(name)
}

struct Shots {
    items: Vec<(String, PathBuf)>,
}

fn scan_saves(dir: &Path) -> Vec<WorldItem> {
    let mut worlds: Vec<WorldItem> = std::fs::read_dir(dir.join("saves"))
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| {
                    let path = e.path();
                    let folder = e.file_name().to_string_lossy().into_owned();
                    let name = world_name(&path.join("level.dat")).unwrap_or_else(|| folder.clone());
                    WorldItem {
                        folder: folder.into(),
                        name: name.into(),
                        detail: format!("{}  ·  {}", megabytes(dir_size(&path)), ago(stamp(&path), now()).to_lowercase()).into(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    worlds.sort_by_key(|w| std::cmp::Reverse(stamp(&dir.join("saves").join(w.folder.as_str()))));
    worlds
}

fn scan_shots(dir: &Path) -> Shots {
    let mut items: Vec<(String, PathBuf)> = std::fs::read_dir(dir.join("screenshots"))
        .map(|d| {
            d.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| ["png", "jpg", "jpeg"].contains(&e.to_string_lossy().to_lowercase().as_str())))
                .map(|p| (p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), p))
                .collect()
        })
        .unwrap_or_default();
    items.sort_by_key(|(_, p)| std::cmp::Reverse(stamp(p)));
    items.truncate(24);
    Shots { items }
}

fn zip_dir(zip: &mut zip::ZipWriter<std::fs::File>, options: zip::write::SimpleFileOptions, dir: &Path, prefix: &str) -> Result<(), String> {
    let root = std::fs::canonicalize(dir).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().replace('\\', "-");
        if name == "." || name == ".." {
            continue;
        }
        let src = entry.path();
        let resolved = std::fs::canonicalize(&src).map_err(|e| e.to_string())?;
        if !resolved.starts_with(&root) {
            continue;
        }
        let rel = format!("{prefix}/{name}");
        if file_type.is_dir() {
            zip_dir(zip, options, &src, &rel)?;
        } else if file_type.is_file() {
            zip.start_file(rel, options).map_err(|e| e.to_string())?;
            let mut file = std::fs::File::open(&src).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, zip).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn export_pack(dir: &Path, profile: &crate::store::Profile, game: &str, out: &Path) -> Result<(), String> {
    let file = std::fs::File::create(out).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut deps = serde_json::json!({ "minecraft": game });
    if !profile.vanilla {
        deps["fabric-loader"] = serde_json::json!(if profile.loader.trim().is_empty() { "*" } else { profile.loader.trim() });
    }
    let index = serde_json::json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": "1.0",
        "name": profile.name,
        "summary": format!("Exported from CxLauncher on {}", ago(profile.created, now())),
        "files": [],
        "dependencies": deps,
    });
    zip.start_file("modrinth.index.json", options).map_err(|e| e.to_string())?;
    zip.write_all(index.to_string().as_bytes()).map_err(|e| e.to_string())?;
    zip_dir(&mut zip, options, dir, "overrides")?;
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

impl App {
    pub fn open_instance(&mut self, id: &str) {
        if self.profiles.get(id).is_none() {
            return;
        }
        let dir = self.core.paths.profile(id);
        let id = id.to_string();
        background(move || {
            let worlds = scan_saves(&dir);
            let shots = scan_shots(&dir);
            later(move |app| {
                let editing = app.ui().map(|u| u.get_profile_edit().id.to_string() == id).unwrap_or(false);
                if editing {
                    app.models.worlds.set_vec(worlds);
                    app.models.shots.set_vec(shots.items.iter().enumerate().map(|(i, (file, _))| ShotItem {
                        file: file.as_str().into(),
                        thumb: shots.items.get(i).and_then(|(_, p)| std::fs::read(p).ok()).and_then(|b| thumb(&b, 160)).map(Image::from_rgba8).unwrap_or_default(),
                    }).collect::<Vec<_>>());
                    if let Some(ui) = app.ui() {
                        ui.set_instance_id(id.as_str().into());
                        ui.set_shot_count(shots.items.len() as i32);
                    }
                }
            });
        });
    }

    pub fn open_world(&mut self, folder: &str) {
        let dir = self.core.paths.profile(&self.instance_id()).join("saves").join(folder);
        if !folder.contains(['/', '\\']) {
            crate::sys::reveal(&dir);
        }
    }

    pub fn open_shots(&mut self, file: &str) {
        let dir = self.core.paths.profile(&self.instance_id()).join("screenshots");
        if file.is_empty() {
            crate::sys::reveal(&dir);
        } else if !file.contains(['/', '\\']) {
            crate::sys::open(&dir.join(file).to_string_lossy());
        }
    }

    fn instance_id(&self) -> String {
        self.ui().map(|u| u.get_instance_id().to_string()).unwrap_or_else(|| self.profile().id)
    }

    pub fn backup_profile(&mut self, id: &str) {
        let Some(p) = self.profiles.get(id).cloned() else { return };
        let core = self.core.clone();
        self.toast(format!("Backing up {}…", p.name));
        background(move || {
            let stamp = {
                let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                format!("{}", t)
            };
            let name: String = p.name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
            let dst = core.paths.root.join("backups").join(format!("{name}-{stamp}"));
            super::profiles::copy_dir(&core.paths.profile(&p.id), &dst);
            later(move |app| app.toast(format!("Backup saved to backups/{name}-{stamp}")));
        });
    }

    pub fn export_profile(&mut self, id: &str) {
        let Some(p) = self.profiles.get(id).cloned() else { return };
        let name: String = p.name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
        let Some(out) = rfd::FileDialog::new()
            .set_title("Export as a Modrinth modpack")
            .set_file_name(format!("{name}.mrpack"))
            .add_filter("Modrinth modpack", &["mrpack"])
            .save_file()
        else {
            return;
        };
        let game = self.game_of(&p);
        let dir = self.core.paths.profile(&p.id);
        self.toast(format!("Exporting {}…", p.name));
        background(move || {
            let result = export_pack(&dir, &p, &game, &out.with_extension("mrpack"));
            later(move |app| match result {
                Ok(()) => app.toast(format!("Exported {}", out.with_extension("mrpack").display())),
                Err(e) => app.toast(e),
            });
        });
    }
}
