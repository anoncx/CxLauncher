use super::meta::cached_json;
use super::Endpoints;
use crate::download::{Hash, Job};
use crate::net::{encode, Http};
use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hashes {
    #[serde(default)]
    pub sha1: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModFile {
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub primary: bool,
    #[serde(default)]
    pub size: u64,
    pub hashes: Hashes,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dependency {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub version_id: Option<String>,
    pub dependency_type: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModVersion {
    pub id: String,
    pub project_id: String,
    pub version_number: String,
    #[serde(default)]
    pub version_type: String,
    #[serde(default)]
    pub files: Vec<ModFile>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
}

impl ModVersion {
    pub fn primary(&self) -> Option<&ModFile> {
        self.files.iter().find(|f| f.primary).or(self.files.first())
    }

    pub fn job(&self, dir: &Path) -> Option<Job> {
        let file = self.primary()?;
        if file.filename.contains(['/', '\\']) || file.filename.starts_with('.') {
            return None;
        }
        let mut job = Job::new(&file.url, dir.join(&file.filename)).size(Some(file.size));
        job.hash = file.hashes.sha1.clone().map(|h| Hash::Sha1(h.to_ascii_lowercase()));
        Some(job)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Hit {
    pub project_id: String,
    #[serde(default)]
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub categories: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub followers: u64,
    #[serde(default)]
    #[allow(dead_code)]
    pub project_type: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Search {
    #[serde(default)]
    pub hits: Vec<Hit>,
    #[serde(default)]
    pub total_hits: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GalleryImage {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub featured: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Donation {
    #[serde(default)]
    #[allow(dead_code)]
    pub id: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub url: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct License {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub name: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub url: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Project {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub body: String,
    pub icon_url: Option<String>,
    pub downloads: u64,
    pub followers: u64,
    pub categories: Vec<String>,
    pub additional_categories: Vec<String>,
    pub loaders: Vec<String>,
    pub game_versions: Vec<String>,
    pub project_type: String,
    pub client_side: String,
    pub server_side: String,
    pub license: License,
    pub source_url: Option<String>,
    pub issues_url: Option<String>,
    pub wiki_url: Option<String>,
    pub discord_url: Option<String>,
    pub donation_urls: Vec<Donation>,
    pub gallery: Vec<GalleryImage>,
    pub published: String,
    pub updated: String,
    pub versions: Vec<String>,
}

impl Project {
    pub fn gallery_entries(&self) -> Vec<(String, String)> {
        let entry = |g: &GalleryImage| (g.url.clone(), g.title.clone().unwrap_or_default());
        let mut entries: Vec<_> = self.gallery.iter().filter(|g| g.featured).map(entry).collect();
        entries.extend(self.gallery.iter().filter(|g| !g.featured).map(entry));
        entries
    }

    pub fn links(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (label, url) in [
            ("Modrinth", format!("https://modrinth.com/{}/{}", self.project_type, self.slug)),
            ("Source", self.source_url.clone().unwrap_or_default()),
            ("Issues", self.issues_url.clone().unwrap_or_default()),
            ("Wiki", self.wiki_url.clone().unwrap_or_default()),
            ("Discord", self.discord_url.clone().unwrap_or_default()),
        ] {
            if !url.is_empty() {
                out.push((label.to_string(), url));
            }
        }
        for d in &self.donation_urls {
            if !d.url.is_empty() {
                let label = if d.platform.is_empty() { "Donate".to_string() } else { d.platform.clone() };
                out.push((label, d.url.clone()));
            }
        }
        out
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    pub game: Option<String>,
    pub categories: Vec<String>,
    pub loader: Option<String>,
    pub sort: Sort,
    pub offset: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Relevance,
    Downloads,
    Follows,
    Newest,
    Updated,
}

impl Sort {
    pub fn index(self) -> &'static str {
        match self {
            Sort::Relevance => "relevance",
            Sort::Downloads => "downloads",
            Sort::Follows => "follows",
            Sort::Newest => "newest",
            Sort::Updated => "updated",
        }
    }
}

fn base(ep: &Endpoints) -> &str {
    ep.modrinth.trim_end_matches('/')
}

fn loader_facets(loader: &str) -> String {
    format!("[\"categories:{loader}\"]")
}

pub fn versions(http: &Http, ep: &Endpoints, paths: &Paths, project: &str, game: &str, loader: &str) -> Result<Vec<ModVersion>, String> {
    let url = format!(
        "{}/project/{}/version?loaders={}&game_versions={}",
        base(ep),
        encode(project),
        encode(&format!("[\"{loader}\"]")),
        encode(&format!("[\"{game}\"]"))
    );
    let safe: String = project.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    cached_json(http, &url, &paths.meta().join("modrinth").join(format!("{safe}-{game}-{loader}.json")))
}

pub fn versions_any(http: &Http, ep: &Endpoints, paths: &Paths, project: &str, game: &str) -> Result<Vec<ModVersion>, String> {
    let url = format!("{}/project/{}/version?game_versions={}", base(ep), encode(project), encode(&format!("[\"{game}\"]")));
    let safe: String = project.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    cached_json(http, &url, &paths.meta().join("modrinth").join(format!("{safe}-{game}-any.json")))
}

pub fn pick(list: Vec<ModVersion>, number: Option<&str>) -> Option<ModVersion> {
    if let Some(number) = number {
        if let Some(v) = list.iter().find(|v| v.version_number == number) {
            return Some(v.clone());
        }
    }
    let release = list.iter().find(|v| v.version_type == "release").cloned();
    release.or_else(|| list.into_iter().next())
}

#[allow(dead_code)]
pub fn search(http: &Http, ep: &Endpoints, query: &str, game: Option<&str>, kind: &str) -> Result<Search, String> {
    search_filtered(http, ep, query, kind, &Filter { game: game.map(str::to_string), ..Default::default() })
}

pub fn search_filtered(http: &Http, ep: &Endpoints, query: &str, kind: &str, filter: &Filter) -> Result<Search, String> {
    let mut facets = vec![format!("[\"project_type:{kind}\"]")];
    if let Some(loader) = filter.loader.as_deref().filter(|_| kind == "mod") {
        facets.push(loader_facets(loader));
    }
    if let Some(game) = filter.game.as_ref().filter(|g| !g.is_empty()) {
        facets.push(format!("[\"versions:{game}\"]"));
    }
    for cat in &filter.categories {
        facets.push(format!("[\"categories:{cat}\"]"));
    }
    let sort = if query.trim().is_empty() && filter.sort == Sort::Relevance { Sort::Downloads } else { filter.sort };
    let url = format!(
        "{}/search?query={}&facets={}&limit=24&offset={}&index={}",
        base(ep),
        encode(query),
        encode(&format!("[{}]", facets.join(","))),
        filter.offset,
        sort.index()
    );
    http.json(&url)
}

pub fn project(http: &Http, ep: &Endpoints, id: &str) -> Result<Project, String> {
    http.json(&format!("{}/project/{}", base(ep), encode(id)))
}

pub fn pack_versions(http: &Http, ep: &Endpoints, project: &str, loader: Option<&str>) -> Result<Vec<ModVersion>, String> {
    let filter = loader.map(|l| format!("?loaders={}", encode(&format!("[\"{l}\"]")))).unwrap_or_default();
    http.json(&format!("{}/project/{}/version{filter}", base(ep), encode(project)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_search_totals_for_paging() {
        let found: Search = serde_json::from_str(r#"{"hits": [{"project_id": "a", "title": "A"}], "offset": 24, "limit": 24, "total_hits": 4210}"#).unwrap();
        assert_eq!(found.hits.len(), 1);
        assert_eq!(found.total_hits, 4210);
        let bare: Search = serde_json::from_str(r#"{"hits": []}"#).unwrap();
        assert_eq!(bare.total_hits, 0);
    }

    #[test]
    fn reads_versions() {
        let list: Vec<ModVersion> = serde_json::from_str(
            r#"[{"id": "v1", "project_id": "P7dR8mSH", "version_number": "0.154.2+26.2", "version_type": "release",
                "files": [{"url": "https://cdn/x.jar", "filename": "fabric-api-0.154.2+26.2.jar", "primary": true, "size": 10, "hashes": {"sha1": "AB", "sha512": "cd"}}],
                "dependencies": [{"project_id": "x", "dependency_type": "required"}]}]"#,
        )
        .unwrap();
        let job = list[0].job(Path::new("/m")).unwrap();
        assert_eq!(job.path, Path::new("/m/fabric-api-0.154.2+26.2.jar"));
        assert_eq!(job.hash, Some(Hash::Sha1("ab".into())));
    }

    #[test]
    fn refuses_unsafe_file_names() {
        let v: ModVersion = serde_json::from_str(
            r#"{"id": "v", "project_id": "p", "version_number": "1", "files": [{"url": "u", "filename": "../evil.jar", "hashes": {}}]}"#,
        )
        .unwrap();
        assert!(v.job(Path::new("/m")).is_none());
    }

    #[test]
    fn project_links_skip_empty() {
        let p: Project = serde_json::from_str(
            r#"{"id":"x","slug":"sodium","title":"Sodium","project_type":"mod","source_url":"https://github.com/x","gallery":[{"url":"a","featured":true},{"url":"b"}]}"#,
        )
        .unwrap();
        let links = p.links();
        assert_eq!(links[0].0, "Modrinth");
        assert_eq!(links[1].0, "Source");
        assert_eq!(p.gallery_entries().into_iter().map(|(url, _)| url).collect::<Vec<_>>(), vec!["a".to_string(), "b".to_string()]);
    }
}
