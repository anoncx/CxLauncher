use super::meta::{cached_json, Version};
use super::Endpoints;
use crate::net::Http;
use crate::paths::{write_atomic, Paths};
use serde::Deserialize;
use std::cmp::Ordering;

#[derive(Debug, Deserialize)]
struct Loader {
    version: String,
    #[serde(default)]
    stable: bool,
}

#[derive(Debug, Deserialize)]
struct LoaderEntry {
    loader: Loader,
}

pub fn compare(a: &str, b: &str) -> Ordering {
    let split = |s: &str| s.split(['.', '-', '+']).map(str::to_string).collect::<Vec<_>>();
    let (pa, pb) = (split(a), split(b));
    for i in 0..pa.len().max(pb.len()) {
        let (x, y) = (pa.get(i).map(String::as_str).unwrap_or("0"), pb.get(i).map(String::as_str).unwrap_or("0"));
        let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            _ => x.cmp(y),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}

pub fn latest_loader(http: &Http, ep: &Endpoints, paths: &Paths, game: &str, minimum: Option<&str>) -> Result<String, String> {
    let url = format!("{}/versions/loader/{game}", ep.fabric_meta.trim_end_matches('/'));
    let list: Vec<LoaderEntry> = cached_json(http, &url, &paths.meta().join("fabric").join(format!("loaders-{game}.json")))?;
    let fits = |l: &&LoaderEntry| minimum.is_none_or(|m| compare(&l.loader.version, m) != Ordering::Less);
    list.iter()
        .filter(fits)
        .find(|l| l.loader.stable)
        .or_else(|| list.iter().find(fits))
        .map(|l| l.loader.version.clone())
        .ok_or(format!("Fabric has no loader for Minecraft {game} yet"))
}

pub fn profile(http: &Http, ep: &Endpoints, paths: &Paths, game: &str, loader: &str) -> Result<Version, String> {
    let cache = paths.meta().join("fabric").join(format!("{game}-{loader}.json"));
    if let Ok(text) = std::fs::read_to_string(&cache) {
        if let Ok(v) = serde_json::from_str(&text) {
            return Ok(v);
        }
    }
    let url = format!("{}/versions/loader/{game}/{loader}/profile/json", ep.fabric_meta.trim_end_matches('/'));
    let text = http.text(&url)?;
    let v: Version = serde_json::from_str(&text).map_err(|e| format!("unexpected Fabric profile ({e})"))?;
    let _ = write_atomic(&cache, text.as_bytes());
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert_eq!(compare("0.19.10", "0.19.3"), Ordering::Greater);
        assert_eq!(compare("0.19.3", "0.19.3"), Ordering::Equal);
        assert_eq!(compare("0.19", "0.19.0"), Ordering::Equal);
        assert_eq!(compare("0.18.9", "0.19.0"), Ordering::Less);
        assert_eq!(compare("0.154.2+26.2", "0.154.10+26.2"), Ordering::Less);
    }
}
