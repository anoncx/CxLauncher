use super::images::thumb;
use super::{background, first_letter, later, row_index, App, Core};
use crate::download::hex;
use crate::mc::modrinth;
use crate::paths::write_atomic;
use crate::{FindItem, ModDetail, ModImage, ModLink};
use sha1::{Digest, Sha1};
use slint::{Image, Model, VecModel};
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Mod,
    Pack,
}

#[derive(Default)]
pub struct Finder {
    generation: u64,
    detail_gen: u64,
    timer: slint::Timer,
    pub query: String,
    pub hits: Vec<modrinth::Hit>,
    pub installing: HashSet<String>,
    pub detail_kind: Option<Kind>,
    pub category: String,
    pub sort: modrinth::Sort,
}

fn commas(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn side_label(v: &str) -> String {
    match v {
        "required" => "Required",
        "optional" => "Optional",
        "unsupported" => "Unsupported",
        _ => "—",
    }
    .to_string()
}

fn type_label(v: &str) -> String {
    match v {
        "mod" => "Mod",
        "modpack" => "Modpack",
        "resourcepack" => "Resource pack",
        "shader" => "Shader",
        "datapack" => "Data pack",
        "plugin" => "Plugin",
        _ => "Project",
    }
    .to_string()
}

fn date_label(v: &str) -> String {
    v.split('T').next().unwrap_or(v).to_string()
}

fn downloads(n: u64) -> String {
    match n {
        0..=999 => format!("{n} downloads"),
        1_000..=999_999 => format!("{:.1}K downloads", n as f64 / 1e3).replace(".0K", "K"),
        _ => format!("{:.1}M downloads", n as f64 / 1e6).replace(".0M", "M"),
    }
}

fn icon_cache(core: &Core, url: &str) -> PathBuf {
    let mut h = Sha1::new();
    h.update(url.as_bytes());
    core.paths.cache().join("icons").join(hex(&h.finalize()))
}

impl App {
    fn finder(&mut self, kind: Kind) -> &mut Finder {
        match kind {
            Kind::Mod => &mut self.mods.find,
            Kind::Pack => &mut self.packs,
        }
    }

    fn finder_ref(&self, kind: Kind) -> &Finder {
        match kind {
            Kind::Mod => &self.mods.find,
            Kind::Pack => &self.packs,
        }
    }

    fn found_model(&self, kind: Kind) -> &Rc<VecModel<FindItem>> {
        match kind {
            Kind::Mod => &self.models.results,
            Kind::Pack => &self.models.packs,
        }
    }

    fn searching(&self, kind: Kind, on: bool, note: Option<String>) {
        let Some(ui) = self.ui() else { return };
        match kind {
            Kind::Mod => {
                ui.set_searching(on);
                if let Some(n) = note {
                    ui.set_find_note(n.into());
                }
            }
            Kind::Pack => {
                ui.set_packs_searching(on);
                if let Some(n) = note {
                    ui.set_packs_note(n.into());
                }
            }
        }
    }

    pub fn find(&mut self, kind: Kind, query: String, now: bool) {
        let finder = self.finder(kind);
        finder.query = query;
        if now {
            finder.timer.stop();
            self.search(kind);
        } else {
            finder.timer.start(slint::TimerMode::SingleShot, Duration::from_millis(320), move || later(move |app| app.search(kind)));
        }
    }

    pub fn set_filter(&mut self, kind: Kind, category: &str, sort: &str) {
        let finder = self.finder(kind);
        finder.category = if category == "all" { String::new() } else { category.to_string() };
        finder.sort = match sort {
            "downloads" => modrinth::Sort::Downloads,
            "follows" => modrinth::Sort::Follows,
            "newest" => modrinth::Sort::Newest,
            "updated" => modrinth::Sort::Updated,
            _ => modrinth::Sort::Relevance,
        };
        self.search(kind);
    }

    fn search(&mut self, kind: Kind) {
        let profile = self.profile();
        let game = (kind == Kind::Mod).then(|| self.game_of(&profile));
        let content_kind = if kind == Kind::Mod { self.mods.content.project_type().to_string() } else { "modpack".to_string() };
        let finder = self.finder(kind);
        finder.generation += 1;
        let generation = finder.generation;
        let query = finder.query.clone();
        let filter = modrinth::Filter {
            game: game.clone(),
            categories: if finder.category.is_empty() { Vec::new() } else { vec![finder.category.clone()] },
            loader: Some("fabric".to_string()),
            sort: finder.sort,
        };
        let core = self.core.clone();
        self.searching(kind, true, None);
        background(move || {
            let result = modrinth::search_filtered(&core.http, &core.ep, &query, &content_kind, &filter);
            later(move |app| app.show_results(kind, generation, query, game, result));
        });
    }

    fn show_results(&mut self, kind: Kind, generation: u64, query: String, game: Option<String>, result: Result<modrinth::Search, String>) {
        if generation != self.finder(kind).generation {
            return;
        }
        let what = match &game {
            Some(g) => format!("Fabric mods for {g}"),
            None => "Fabric modpacks".to_string(),
        };
        let (hits, note) = match result {
            Ok(s) if s.hits.is_empty() => (Vec::new(), format!("No {what} match “{query}”.")),
            Ok(s) => (s.hits, String::new()),
            Err(e) => (Vec::new(), format!("Modrinth is not reachable right now ({e}).")),
        };
        self.searching(kind, false, Some(note));
        let items: Vec<FindItem> = hits
            .iter()
            .map(|h| FindItem {
                id: h.project_id.as_str().into(),
                title: h.title.as_str().into(),
                author: h.author.as_str().into(),
                description: h.description.as_str().into(),
                downloads: downloads(h.downloads).into(),
                icon: Image::default(),
                has_icon: false,
                letter: first_letter(&h.title),
                state: 0,
            })
            .collect();
        self.found_model(kind).set_vec(items);
        self.finder(kind).hits = hits.clone();
        self.mark_results(kind);
        let core = self.core.clone();
        background(move || {
            for hit in hits {
                let Some(url) = hit.icon_url.filter(|u| !u.is_empty()) else { continue };
                let cache = icon_cache(&core, &url);
                let bytes = std::fs::read(&cache).ok().or_else(|| {
                    let b = core.http.bytes(&url, 2 << 20).ok()?;
                    let _ = write_atomic(&cache, &b);
                    Some(b)
                });
                let Some(px) = bytes.and_then(|b| thumb(&b, 64)) else { continue };
                let id = hit.project_id.clone();
                later(move |app| {
                    if generation != app.finder(kind).generation {
                        return;
                    }
                    let model = app.found_model(kind);
                    if let Some(i) = row_index(model, |r| r.id == id.as_str()) {
                        let mut row = model.row_data(i).unwrap();
                        row.icon = Image::from_rgba8(px);
                        row.has_icon = true;
                        model.set_row_data(i, row);
                    }
                });
            }
        });
    }

    pub fn open_detail(&mut self, kind: Kind, id: &str) {
        self.mods.find.detail_kind = None;
        self.packs.detail_kind = None;
        let finder = self.finder(kind);
        finder.detail_gen += 1;
        finder.detail_kind = Some(kind);
        let generation = finder.detail_gen;
        let hit = finder.hits.iter().find(|h| h.project_id == id).cloned();
        let author = hit.as_ref().map(|h| h.author.clone()).unwrap_or_default();
        let state = if self.finder(kind).installing.contains(id) {
            1
        } else if kind == Kind::Mod && hit.as_ref().is_some_and(|h| self.installed_hit(h)) {
            2
        } else {
            0
        };
        if let Some(ui) = self.ui() {
            ui.set_mod_detail(ModDetail {
                id: id.into(),
                title: hit.as_ref().map(|h| h.title.as_str()).unwrap_or_default().into(),
                author: hit.as_ref().map(|h| h.author.as_str()).unwrap_or_default().into(),
                description: hit.as_ref().map(|h| h.description.as_str()).unwrap_or_default().into(),
                downloads: hit.as_ref().map(|h| commas(h.downloads)).unwrap_or_default().into(),
                icon: Image::default(),
                has_icon: false,
                letter: first_letter(hit.as_ref().map(|h| h.title.as_str()).unwrap_or("?")),
                state,
                loading: true,
                ..Default::default()
            });
            ui.set_detail_open(true);
        }
        let core = self.core.clone();
        let id = id.to_string();
        background(move || {
            let result = modrinth::project(&core.http, &core.ep, &id);
            let icon = result
                .as_ref()
                .ok()
                .and_then(|p| p.icon_url.clone())
                .filter(|u| !u.is_empty())
                .and_then(|url| {
                    let cache = icon_cache(&core, &url);
                    let bytes = std::fs::read(&cache).ok().or_else(|| {
                        let b = core.http.bytes(&url, 2 << 20).ok()?;
                        let _ = write_atomic(&cache, &b);
                        Some(b)
                    })?;
                    thumb(&bytes, 64)
                });
            later(move |app| app.show_detail(kind, generation, result, author, icon));
        });
    }

    fn show_detail(&mut self, kind: Kind, generation: u64, result: Result<modrinth::Project, String>, author: String, icon: Option<super::images::Pixels>) {
        if generation != self.finder(kind).detail_gen {
            return;
        }
        let Some(ui) = self.ui() else { return };
        match result {
            Ok(p) => {
                let state = if self.finder(kind).installing.contains(&p.id) {
                    1
                } else if kind == Kind::Mod && self.mods.index.projects.contains_key(&p.id) {
                    2
                } else {
                    0
                };
                let links: Vec<ModLink> = p.links().into_iter().map(|(label, url)| ModLink { label: label.into(), url: url.into() }).collect();
                self.models.links.set_vec(links);
                let has_icon = icon.is_some();
                let gallery = p.gallery_urls();
                let core = self.core.clone();
                let gallery_images: Vec<ModImage> = gallery.iter().map(|_| ModImage { image: Image::default(), has_image: false, title: String::new().into() }).collect();
                self.models.gallery.set_vec(gallery_images);
                let gallery_id = p.id.clone();
                background(move || {
                    for (i, url) in gallery.iter().take(6).enumerate() {
                        let cached = {
                            let cache = icon_cache(&core, url);
                            std::fs::read(&cache).ok().or_else(|| {
                                let bytes = core.http.bytes(url, 4 << 20).ok()?;
                                let _ = write_atomic(&cache, &bytes);
                                Some(bytes)
                            })
                        };
                        let Some(px) = cached.and_then(|b| thumb(&b, 720)) else { continue };
                        later(move |app| {
                            if app.finder(kind).detail_gen != generation { return; }
                            if let Some(row) = app.models.gallery.row_data(i) {
                                app.models.gallery.set_row_data(i, ModImage { image: Image::from_rgba8(px), has_image: true, title: row.title });
                            }
                        });
                    }
                    let _ = gallery_id;
                });
                ui.set_mod_detail(ModDetail {
                    id: p.id.clone().into(),
                    slug: p.slug.as_str().into(),
                    title: p.title.as_str().into(),
                    author: author.into(),
                    description: p.description.as_str().into(),
                    body: p.body.as_str().into(),
                    downloads: commas(p.downloads).into(),
                    followers: commas(p.followers).into(),
                    license: p.license.id.as_str().into(),
                    project_type: type_label(&p.project_type).into(),
                    categories: p.categories.iter().chain(p.additional_categories.iter()).cloned().collect::<Vec<_>>().join(", ").into(),
                    client_side: side_label(&p.client_side).into(),
                    server_side: side_label(&p.server_side).into(),
                    updated: date_label(&p.updated).into(),
                    icon: icon.map(Image::from_rgba8).unwrap_or_default(),
                    has_icon,
                    letter: first_letter(&p.title),
                    state,
                    loading: false,
                    gallery_count: p.gallery.len() as i32,
                    ..Default::default()
                });
            }
            Err(e) => {
                ui.set_mod_detail(ModDetail { loading: false, body: format!("Could not load this project ({e}).").into(), ..Default::default() });
            }
        }
    }

    pub fn detail_install(&mut self, id: &str) {
        let kind = self.mods.find.detail_kind.or(self.packs.detail_kind).unwrap_or(Kind::Mod);
        match kind {
            Kind::Mod => self.install(id),
            Kind::Pack => self.install_pack(id),
        }
    }

    pub fn close_detail(&mut self) {
        self.mods.find.detail_gen += 1;
        self.packs.detail_gen += 1;
        self.mods.find.detail_kind = None;
        self.packs.detail_kind = None;
        if let Some(ui) = self.ui() {
            ui.set_detail_open(false);
        }
    }

    pub fn mark_results(&self, kind: Kind) {
        let model = self.found_model(kind);
        let finder = self.finder_ref(kind);
        for (i, hit) in finder.hits.iter().enumerate() {
            let Some(mut row) = model.row_data(i) else { continue };
            let state = if finder.installing.contains(&hit.project_id) {
                1
            } else if kind == Kind::Mod && self.installed_hit(hit) {
                2
            } else {
                0
            };
            if row.state != state {
                row.state = state;
                model.set_row_data(i, row);
            }
        }
    }

    pub fn hit_title(&self, kind: Kind, id: &str) -> String {
        self.finder_ref(kind).hits.iter().find(|h| h.project_id == id).map(|h| h.title.clone()).unwrap_or_default()
    }

    pub fn begin_install(&mut self, kind: Kind, id: &str) -> bool {
        let fresh = self.finder(kind).installing.insert(id.to_string());
        self.mark_results(kind);
        fresh
    }

    pub fn end_install(&mut self, kind: Kind, id: &str) {
        self.finder(kind).installing.remove(id);
        self.mark_results(kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_downloads() {
        assert_eq!(downloads(950), "950 downloads");
        assert_eq!(downloads(12_000), "12K downloads");
        assert_eq!(downloads(3_450_000), "3.5M downloads");
    }
}
