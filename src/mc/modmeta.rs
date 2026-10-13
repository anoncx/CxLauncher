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

type Zip = zip::ZipArchive<std::fs::File>;

pub fn read(jar: &Path, with_icon: bool) -> Option<ModInfo> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(jar).ok()?).ok()?;
    read_fabric(&mut zip, with_icon)
        .or_else(|| read_quilt(&mut zip, with_icon))
        .or_else(|| read_forge(&mut zip, "META-INF/neoforge.mods.toml", with_icon))
        .or_else(|| read_forge(&mut zip, "META-INF/mods.toml", with_icon))
        .or_else(|| read_legacy(&mut zip, with_icon))
}

fn named(id: String, name: String, version: String, description: String, minecraft: String, icon: Option<Vec<u8>>) -> Option<ModInfo> {
    (!id.is_empty()).then(|| ModInfo { name: Some(name).filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone()), id, version, description, minecraft, icon })
}

fn json_entry(zip: &mut Zip, name: &str) -> Option<Value> {
    parse(&String::from_utf8_lossy(&entry(zip, name, 1 << 20)?))
}

fn read_fabric(zip: &mut Zip, with_icon: bool) -> Option<ModInfo> {
    let v = json_entry(zip, "fabric.mod.json")?;
    let minecraft = match v.get("depends").and_then(|d| d.get("minecraft")) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a.first().and_then(Value::as_str).unwrap_or_default().to_string(),
        _ => String::new(),
    };
    let icon = if with_icon { icon_path(&v).and_then(|p| entry(zip, &p, 4 << 20)) } else { None };
    named(text(&v, "id"), text(&v, "name"), text(&v, "version"), text(&v, "description"), minecraft, icon)
}

fn read_quilt(zip: &mut Zip, with_icon: bool) -> Option<ModInfo> {
    let root = json_entry(zip, "quilt.mod.json")?;
    let v = root.get("quilt_loader")?;
    let meta = v.get("metadata").unwrap_or(&Value::Null);
    let minecraft = v
        .get("depends")
        .and_then(Value::as_array)
        .and_then(|deps| deps.iter().find(|d| d.get("id").and_then(Value::as_str) == Some("minecraft")))
        .and_then(|d| d.get("versions"))
        .map(|r| match r {
            Value::String(s) => s.clone(),
            Value::Array(a) => a.first().and_then(Value::as_str).unwrap_or_default().to_string(),
            _ => String::new(),
        })
        .unwrap_or_default();
    let icon = if with_icon { icon_path(meta).and_then(|p| entry(zip, &p, 4 << 20)) } else { None };
    named(text(v, "id"), text(meta, "name"), text(v, "version"), text(meta, "description"), minecraft, icon)
}

fn toml_text(table: &toml::Table, key: &str) -> String {
    table.get(key).and_then(toml::Value::as_str).unwrap_or_default().trim().to_string()
}

fn read_forge(zip: &mut Zip, file: &str, with_icon: bool) -> Option<ModInfo> {
    let raw = entry(zip, file, 1 << 20)?;
    let doc: toml::Table = toml::from_str(&String::from_utf8_lossy(&raw)).ok()?;
    let main = doc.get("mods")?.as_array()?.first()?.as_table()?;
    let id = toml_text(main, "modId");
    let minecraft = doc
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .and_then(|d| d.get(&id))
        .and_then(toml::Value::as_array)
        .and_then(|deps| deps.iter().filter_map(toml::Value::as_table).find(|d| toml_text(d, "modId") == "minecraft"))
        .map(|d| toml_text(d, "versionRange"))
        .unwrap_or_default();
    let version = Some(toml_text(main, "version")).filter(|v| !v.starts_with("${")).unwrap_or_default();
    let icon = if with_icon { Some(toml_text(&doc, "logoFile")).filter(|p| !p.is_empty()).or_else(|| Some(toml_text(main, "logoFile")).filter(|p| !p.is_empty())).and_then(|p| entry(zip, &p, 4 << 20)) } else { None };
    named(id, toml_text(main, "displayName"), version, toml_text(main, "description"), minecraft, icon)
}

fn read_legacy(zip: &mut Zip, with_icon: bool) -> Option<ModInfo> {
    let root = json_entry(zip, "mcmod.info")?;
    let list = root.get("modList").unwrap_or(&root);
    let v = list.as_array()?.first()?;
    let icon = if with_icon { Some(text(v, "logoFile")).filter(|p| !p.is_empty()).and_then(|p| entry(zip, &p, 4 << 20)) } else { None };
    named(text(v, "modid"), text(v, "name"), text(v, "version"), text(v, "description"), text(v, "mcversion"), icon)
}

pub fn game_version(range: &str) -> Option<String> {
    let v = range.trim_start_matches(['~', '^', '>', '=', '<', ' ', '[', '(']).split([' ', ',']).next()?.trim_end_matches(".x");
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
    fn reads_forge_quilt_and_legacy_metadata() {
        let dir = std::env::temp_dir().join(format!("cxl-mod-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        let forge = dir.join("forge.jar");
        let toml = b"modLoader=\"javafml\"
[[mods]]
modId=\"examplemod\"
version=\"1.2.3\"
displayName=\"Example\"
description=\"Hi\"
[[dependencies.examplemod]]
modId=\"minecraft\"
versionRange=\"[1.20.1,1.21)\"
";
        write_jar(&forge, &[("META-INF/mods.toml", toml)]);
        let info = read(&forge, false).unwrap();
        assert_eq!((info.id.as_str(), info.version.as_str(), info.name.as_str()), ("examplemod", "1.2.3", "Example"));
        assert_eq!(game_version(&info.minecraft).as_deref(), Some("1.20.1"));
        let quilt = dir.join("quilt.jar");
        write_jar(&quilt, &[("quilt.mod.json", br#"{"quilt_loader": {"id": "qmod", "version": "2.0", "metadata": {"name": "Q"}, "depends": [{"id": "minecraft", "versions": ">=1.20"}]}}"#)]);
        let info = read(&quilt, false).unwrap();
        assert_eq!((info.id.as_str(), info.name.as_str(), game_version(&info.minecraft).as_deref()), ("qmod", "Q", Some("1.20")));
        let legacy = dir.join("legacy.jar");
        write_jar(&legacy, &[("mcmod.info", br#"[{"modid": "old", "name": "Old", "version": "1.0", "mcversion": "1.12.2"}]"#)]);
        assert_eq!(read(&legacy, false).unwrap().id, "old");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reads_version_ranges() {
        assert_eq!(game_version(">=26.2 <26.3").as_deref(), Some("26.2"));
        assert_eq!(game_version("26.2.x").as_deref(), Some("26.2"));
        assert_eq!(game_version("*"), None);
    }
}
