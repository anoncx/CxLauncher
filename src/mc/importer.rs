use super::loader::Loader;
use serde_json::Value;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub launcher: &'static str,
    pub name: String,
    pub game: String,
    pub loader: Option<(Loader, String)>,
    pub dir: PathBuf,
    pub only: Option<&'static [&'static str]>,
    pub key: String,
    pub mods: usize,
    pub worlds: usize,
}

pub struct Roots {
    pub home: PathBuf,
    pub data: PathBuf,
}

impl Roots {
    pub fn system() -> Self {
        Self { home: dirs::home_dir().unwrap_or_default(), data: dirs::data_dir().unwrap_or_default() }
    }
}

const OFFICIAL_MODDED: &[&str] = &["saves", "mods", "config", "resourcepacks", "shaderpacks", "options.txt", "servers.dat"];
const OFFICIAL_VANILLA: &[&str] = &["saves", "resourcepacks", "shaderpacks", "options.txt", "servers.dat"];
const SKIP: &[&str] = &["logs", "crash-reports", "minecraftinstance.json", "profile.json", "instance.cfg"];

pub fn scan() -> Vec<Found> {
    scan_in(&Roots::system())
}

pub fn scan_in(roots: &Roots) -> Vec<Found> {
    let mut out = Vec::new();
    official(roots, &mut out);
    prism(roots, &mut out);
    curseforge(roots, &mut out);
    modrinth(roots, &mut out);
    out.sort_by(|a, b| (a.launcher, a.name.to_lowercase()).cmp(&(b.launcher, b.name.to_lowercase())));
    out
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn count(dir: &Path, wanted: impl Fn(&std::fs::DirEntry) -> bool) -> usize {
    std::fs::read_dir(dir).map(|d| d.flatten().filter(|e| wanted(e)).count()).unwrap_or(0)
}

fn mods_in(dir: &Path) -> usize {
    count(&dir.join("mods"), |e| e.path().extension().is_some_and(|x| x == "jar" || x == "disabled"))
}

fn worlds_in(dir: &Path) -> usize {
    count(&dir.join("saves"), |e| e.file_type().is_ok_and(|t| t.is_dir()))
}

fn found(launcher: &'static str, name: String, game: String, loader: Option<(Loader, String)>, dir: PathBuf, key: String) -> Found {
    Found { launcher, name, game, loader, mods: mods_in(&dir), worlds: worlds_in(&dir), dir, only: None, key }
}

fn neoforge_game(version: &str) -> Option<String> {
    let mut parts = version.split('.');
    let minor: u32 = parts.next()?.parse().ok()?;
    let patch: u32 = parts.next()?.parse().ok()?;
    if !(18..=25).contains(&minor) {
        return None;
    }
    Some(if patch == 0 { format!("1.{minor}") } else { format!("1.{minor}.{patch}") })
}

fn looks_vanilla(id: &str) -> bool {
    let lower = id.to_lowercase();
    id.starts_with(|c: char| c.is_ascii_digit())
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' '))
        && !["optifine", "liteloader", "forge", "fabric", "quilt", "rift"].iter().any(|m| lower.contains(m))
}

/// Splits a launcher version id such as `fabric-loader-0.15.7-1.20.1` into the game version and loader.
pub fn parse_version_id(id: &str) -> Option<(String, Option<(Loader, String)>)> {
    let id = id.trim();
    for (prefix, loader) in [("fabric-loader-", Loader::Fabric), ("quilt-loader-", Loader::Quilt)] {
        if let Some(rest) = id.strip_prefix(prefix) {
            let (version, game) = rest.rsplit_once('-')?;
            return Some((game.to_string(), Some((loader, version.to_string()))));
        }
    }
    if let Some(version) = id.strip_prefix("neoforge-") {
        return Some((neoforge_game(version)?, Some((Loader::NeoForge, version.to_string()))));
    }
    if let Some((game, rest)) = id.split_once("-forge") {
        let version = rest.trim_start_matches('-').rsplit('-').next().filter(|v| !v.is_empty())?;
        return Some((game.to_string(), Some((Loader::Forge, version.to_string()))));
    }
    looks_vanilla(id).then(|| (id.to_string(), None))
}

fn official(roots: &Roots, out: &mut Vec<Found>) {
    let bases = [
        roots.data.join(".minecraft"),
        roots.data.join("minecraft"),
        roots.home.join(".minecraft"),
        roots.home.join(".var/app/com.mojang.Minecraft/.minecraft"),
    ];
    for base in bases {
        let Some(json) = read_json(&base.join("launcher_profiles.json")) else { continue };
        let Some(profiles) = json.get("profiles").and_then(Value::as_object) else { continue };
        for (id, profile) in profiles {
            let version = text(profile, "lastVersionId").unwrap_or("latest-release");
            let (game, loader) = match version {
                "latest-release" | "latest-snapshot" => (String::new(), None),
                other => match parse_version_id(other) {
                    Some(parsed) => parsed,
                    None => continue,
                },
            };
            let dir = text(profile, "gameDir").map(PathBuf::from).filter(|d| d.is_dir()).unwrap_or_else(|| base.clone());
            let name = text(profile, "name").map(str::to_string).unwrap_or_else(|| if game.is_empty() { "Latest release".into() } else { version.to_string() });
            let key = format!("official:{}:{id}", dir.display());
            let only = if loader.is_some() { OFFICIAL_MODDED } else { OFFICIAL_VANILLA };
            out.push(Found { only: Some(only), ..found("Minecraft Launcher", name, game, loader, dir, key) });
        }
    }
}

fn prism(roots: &Roots, out: &mut Vec<Found>) {
    let bases = [
        ("Prism Launcher", roots.data.join("PrismLauncher")),
        ("Prism Launcher", roots.home.join(".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher")),
        ("PolyMC", roots.data.join("PolyMC")),
        ("MultiMC", roots.data.join("MultiMC")),
    ];
    for (label, base) in bases {
        let Ok(entries) = std::fs::read_dir(base.join("instances")) else { continue };
        for entry in entries.flatten() {
            let root = entry.path();
            let folder = entry.file_name().to_string_lossy().into_owned();
            if folder.starts_with(['_', '.']) || !root.join("instance.cfg").is_file() {
                continue;
            }
            let Some(pack) = read_json(&root.join("mmc-pack.json")) else { continue };
            let components = pack.get("components").and_then(Value::as_array).cloned().unwrap_or_default();
            let version_of = |uid: &str| components.iter().find(|c| text(c, "uid") == Some(uid)).and_then(|c| text(c, "version")).map(str::to_string);
            let Some(game) = version_of("net.minecraft") else { continue };
            let loader = [
                ("net.fabricmc.fabric-loader", Loader::Fabric),
                ("org.quiltmc.quilt-loader", Loader::Quilt),
                ("net.neoforged", Loader::NeoForge),
                ("net.minecraftforge", Loader::Forge),
            ]
            .into_iter()
            .find_map(|(uid, l)| version_of(uid).map(|v| (l, v)));
            let cfg = std::fs::read_to_string(root.join("instance.cfg")).unwrap_or_default();
            let name = cfg.lines().find_map(|l| l.strip_prefix("name=")).map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&folder).to_string();
            let hidden = root.join(".minecraft");
            let dir = if hidden.is_dir() && !root.join("minecraft").exists() { hidden } else { root.join("minecraft") };
            if !dir.is_dir() {
                continue;
            }
            out.push(found(label, name, game, loader, dir, format!("prism:{}", root.display())));
        }
    }
}

fn curseforge(roots: &Roots, out: &mut Vec<Found>) {
    let Ok(entries) = std::fs::read_dir(roots.home.join("curseforge/minecraft/Instances")) else { return };
    for entry in entries.flatten() {
        let root = entry.path();
        let Some(meta) = read_json(&root.join("minecraftinstance.json")) else { continue };
        let base = meta.get("baseModLoader").cloned().unwrap_or(Value::Null);
        let Some(game) = text(&base, "minecraftVersion").or_else(|| text(&meta, "gameVersion")).map(str::to_string) else { continue };
        let loader = text(&base, "name").and_then(|name| {
            let (kind, rest) = name.split_once('-')?;
            let loader = Loader::ALL.into_iter().find(|l| l.id() == kind.to_lowercase())?;
            let version = rest.strip_suffix(&format!("-{game}")).unwrap_or(rest);
            Some((loader, version.to_string()))
        });
        let folder = entry.file_name().to_string_lossy().into_owned();
        let name = text(&meta, "name").map(str::to_string).unwrap_or(folder);
        out.push(found("CurseForge", name, game, loader, root.clone(), format!("curseforge:{}", root.display())));
    }
}

fn modrinth(roots: &Roots, out: &mut Vec<Found>) {
    let bases = [
        roots.data.join("ModrinthApp"),
        roots.data.join("com.modrinth.theseus"),
        roots.home.join(".var/app/com.modrinth.ModrinthApp/data/ModrinthApp"),
        roots.home.join(".var/app/com.modrinth.ModrinthApp/data/com.modrinth.theseus"),
    ];
    for base in bases {
        let db = base.join("app.db");
        if !db.is_file() {
            continue;
        }
        let rows = read_modrinth(&db).unwrap_or_default();
        for (path, name, game, loader, version) in rows {
            let dir = base.join("profiles").join(&path);
            if !dir.is_dir() {
                continue;
            }
            let loader = match loader.as_str() {
                "vanilla" => None,
                other => match Loader::ALL.into_iter().find(|l| l.id() == other) {
                    Some(l) => Some((l, version.unwrap_or_default())),
                    None => continue,
                },
            };
            out.push(found("Modrinth App", name, game, loader, dir.clone(), format!("modrinth:{}", dir.display())));
        }
    }
}

type ModrinthRow = (String, String, String, String, Option<String>);

fn read_modrinth(db: &Path) -> rusqlite::Result<Vec<ModrinthRow>> {
    let conn = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = conn.prepare("SELECT path, name, game_version, mod_loader, mod_loader_version FROM profiles")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
    rows.collect()
}

fn copy_tree(from: &Path, to: &Path, skip_top: &[&str]) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_name().to_str().is_some_and(|n| skip_top.contains(&n)) {
            continue;
        }
        let kind = entry.file_type()?;
        let dst = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &dst, &[])?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &dst)?;
        }
    }
    Ok(())
}

/// Copies the instance's game files into `dest`. Symbolic links are skipped and the source is never modified.
pub fn copy_instance(found: &Found, dest: &Path) -> Result<(), String> {
    let run = || -> io::Result<()> {
        std::fs::create_dir_all(dest)?;
        match found.only {
            None => copy_tree(&found.dir, dest, SKIP),
            Some(names) => {
                for name in names {
                    let src = found.dir.join(name);
                    let Ok(meta) = std::fs::symlink_metadata(&src) else { continue };
                    if meta.is_dir() {
                        copy_tree(&src, &dest.join(name), &[])?;
                    } else if meta.is_file() {
                        std::fs::copy(&src, dest.join(name))?;
                    }
                }
                Ok(())
            }
        }
    };
    run().map_err(|e| format!("Could not copy {}: {e}", found.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cxl-import-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn parses_version_ids() {
        let fabric = parse_version_id("fabric-loader-0.15.7-1.20.1").unwrap();
        assert_eq!(fabric, ("1.20.1".into(), Some((Loader::Fabric, "0.15.7".into()))));
        assert_eq!(parse_version_id("quilt-loader-0.26.0-1.21").unwrap().1, Some((Loader::Quilt, "0.26.0".into())));
        assert_eq!(parse_version_id("1.20.1-forge-47.2.0").unwrap(), ("1.20.1".into(), Some((Loader::Forge, "47.2.0".into()))));
        assert_eq!(parse_version_id("1.12.2-forge1.12.2-14.23.5.2860").unwrap(), ("1.12.2".into(), Some((Loader::Forge, "14.23.5.2860".into()))));
        assert_eq!(parse_version_id("neoforge-21.1.172").unwrap(), ("1.21.1".into(), Some((Loader::NeoForge, "21.1.172".into()))));
        assert_eq!(parse_version_id("neoforge-21.0.5").unwrap().0, "1.21");
        assert_eq!(parse_version_id("1.21.4").unwrap(), ("1.21.4".into(), None));
        assert_eq!(parse_version_id("24w14a").unwrap(), ("24w14a".into(), None));
        assert!(parse_version_id("1.8.9-OptiFine_HD_U_M5").is_none());
        assert!(parse_version_id("custom").is_none());
    }

    #[test]
    fn finds_official_profiles() {
        let home = temp();
        let mc = home.join(".minecraft");
        write(
            &mc.join("launcher_profiles.json"),
            r#"{"profiles":{"a":{"name":"","type":"latest-release","lastVersionId":"latest-release"},"b":{"name":"Modded","lastVersionId":"fabric-loader-0.15.7-1.20.1"},"c":{"name":"Tweaked","lastVersionId":"1.8.9-OptiFine_HD_U_M5"}}}"#,
        );
        write(&mc.join("mods/a.jar"), "x");
        std::fs::create_dir_all(mc.join("saves/World")).unwrap();
        let list = scan_in(&Roots { home: home.clone(), data: home.join("data") });
        assert_eq!(list.len(), 2);
        let latest = list.iter().find(|f| f.name == "Latest release").unwrap();
        assert!(latest.game.is_empty() && latest.loader.is_none());
        assert_eq!(latest.only, Some(OFFICIAL_VANILLA));
        let modded = list.iter().find(|f| f.name == "Modded").unwrap();
        assert_eq!((modded.mods, modded.worlds), (1, 1));
        assert_eq!(modded.loader, Some((Loader::Fabric, "0.15.7".into())));
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn finds_prism_instances() {
        let data = temp();
        let inst = data.join("PrismLauncher/instances/Pack One");
        write(&inst.join("instance.cfg"), "InstanceType=OneSix\nname=Pack One\n");
        write(&inst.join("mmc-pack.json"), r#"{"components":[{"uid":"net.minecraft","version":"1.20.1"},{"uid":"net.minecraftforge","version":"47.2.0"}],"formatVersion":1}"#);
        write(&inst.join(".minecraft/mods/m.jar"), "x");
        write(&data.join("PrismLauncher/instances/_LAUNCHER_TEMP/instance.cfg"), "name=tmp");
        let list = scan_in(&Roots { home: data.join("home"), data: data.clone() });
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].launcher, "Prism Launcher");
        assert_eq!(list[0].game, "1.20.1");
        assert_eq!(list[0].loader, Some((Loader::Forge, "47.2.0".into())));
        assert_eq!(list[0].mods, 1);
        let _ = std::fs::remove_dir_all(data);
    }

    #[test]
    fn finds_curseforge_instances() {
        let home = temp();
        let inst = home.join("curseforge/minecraft/Instances/All the Mods");
        write(&inst.join("minecraftinstance.json"), r#"{"name":"All the Mods","gameVersion":"1.20.1","baseModLoader":{"name":"fabric-0.15.7-1.20.1","minecraftVersion":"1.20.1"}}"#);
        let list = scan_in(&Roots { home: home.clone(), data: home.join("data") });
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].loader, Some((Loader::Fabric, "0.15.7".into())));
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn finds_modrinth_profiles() {
        let data = temp();
        let base = data.join("ModrinthApp");
        std::fs::create_dir_all(base.join("profiles/Cool Pack")).unwrap();
        let conn = rusqlite::Connection::open(base.join("app.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE profiles (path TEXT PRIMARY KEY, name TEXT NOT NULL, game_version TEXT NOT NULL, mod_loader TEXT NOT NULL, mod_loader_version TEXT);
             INSERT INTO profiles VALUES ('Cool Pack', 'Cool Pack', '1.21.1', 'neoforge', '21.1.172');
             INSERT INTO profiles VALUES ('Gone', 'Gone', '1.21.1', 'fabric', NULL);",
        )
        .unwrap();
        drop(conn);
        let list = scan_in(&Roots { home: data.join("home"), data: data.clone() });
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].loader, Some((Loader::NeoForge, "21.1.172".into())));
        let _ = std::fs::remove_dir_all(data);
    }

    #[test]
    fn copies_without_touching_the_source() {
        let root = temp();
        let src = root.join("src");
        write(&src.join("mods/a.jar"), "x");
        write(&src.join("logs/latest.log"), "x");
        write(&src.join("options.txt"), "x");
        let f = Found { launcher: "Test", name: "t".into(), game: "1.20.1".into(), loader: None, dir: src.clone(), only: None, key: "k".into(), mods: 0, worlds: 0 };
        let dest = root.join("dest");
        copy_instance(&f, &dest).unwrap();
        assert!(dest.join("mods/a.jar").is_file() && dest.join("options.txt").is_file());
        assert!(!dest.join("logs").exists());
        assert!(src.join("logs/latest.log").is_file());
        let _ = std::fs::remove_dir_all(root);
    }
}
