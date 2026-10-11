use serde_json::Value;
use std::io::Read;
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct ModInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub minecraft: String,
    pub icon: Option<Vec<u8>>,
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().trim().to_string()
}

fn icon_path(v: &Value) -> Option<String> {
    match v.get("icon")? {
        Value::String(s) => Some(s.clone()),
        Value::Object(sizes) => sizes
            .iter()
            .filter_map(|(k, p)| Some((k.parse::<u32>().ok()?, p.as_str()?)))
            .min_by_key(|(size, _)| size.abs_diff(64))
            .map(|(_, p)| p.to_string()),
        _ => None,
    }
}

fn parse(raw: &str) -> Option<Value> {
    serde_json::from_str(raw).ok().or_else(|| {
        let flat: String = raw.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        serde_json::from_str(&flat).ok()
    })
}

fn entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str, limit: u64) -> Option<Vec<u8>> {
    let file = zip.by_name(name.trim_start_matches('/')).ok()?;
    let mut out = Vec::new();
    file.take(limit).read_to_end(&mut out).ok()?;
    Some(out)
}

pub fn read(jar: &Path, with_icon: bool) -> Option<ModInfo> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(jar).ok()?).ok()?;
    let raw = entry(&mut zip, "fabric.mod.json", 1 << 20)?;
    let v = parse(&String::from_utf8_lossy(&raw))?;
    let id = text(&v, "id");
    if id.is_empty() {
        return None;
    }
    let minecraft = match v.get("depends").and_then(|d| d.get("minecraft")) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a.first().and_then(Value::as_str).unwrap_or_default().to_string(),
        _ => String::new(),
    };
    let icon = if with_icon { icon_path(&v).and_then(|p| entry(&mut zip, &p, 4 << 20)) } else { None };
    Some(ModInfo {
        name: Some(text(&v, "name")).filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone()),
        id,
        version: text(&v, "version"),
        description: text(&v, "description"),
        minecraft,
        icon,
    })
}

pub fn game_version(range: &str) -> Option<String> {
    let v = range.trim_start_matches(['~', '^', '>', '=', '<', ' ']).split([' ', ',']).next()?.trim_end_matches(".x");
    (!v.is_empty() && v.starts_with(|c: char| c.is_ascii_digit())).then(|| v.to_string())
}

#[cfg(test)]
pub fn write_jar(path: &Path, files: &[(&str, &[u8])]) {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in files {
        zip.start_file(*name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_fabric_mod_json() {
        let dir = std::env::temp_dir().join(format!("cxl-mod-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        let jar = dir.join("cxvisuals.jar");
        let json = br#"{"schemaVersion": 1, "id": "cxvisuals", "version": "0.1.0+26.2", "name": "CxVisuals",
            "description": "Visual modules
for Minecraft", "icon": {"32": "a.png", "64": "b.png"}, "depends": {"minecraft": "~26.2"}}"#;
        write_jar(&jar, &[("fabric.mod.json", json), ("b.png", b"png")]);
        let info = read(&jar, true).unwrap();
        assert_eq!(info.id, "cxvisuals");
        assert_eq!(info.version, "0.1.0+26.2");
        assert_eq!(info.icon.as_deref(), Some(&b"png"[..]));
        assert_eq!(game_version(&info.minecraft).as_deref(), Some("26.2"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reads_version_ranges() {
        assert_eq!(game_version(">=26.2 <26.3").as_deref(), Some("26.2"));
        assert_eq!(game_version("26.2.x").as_deref(), Some("26.2"));
        assert_eq!(game_version("*"), None);
    }
}
