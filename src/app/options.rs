use super::Core;
use crate::paths::write_atomic;
use std::path::{Path, PathBuf};

const ONBOARD: &str = "onboardAccessibility";

pub fn path(core: &Core, profile: &str) -> PathBuf {
    core.paths.profile(profile).join("options.txt")
}

pub fn read(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path).map(|s| s.lines().map(|l| l.to_string()).collect()).unwrap_or_default()
}

pub fn write(path: &Path, lines: &[String]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut text = lines.join("\n");
    text.push('\n');
    write_atomic(path, text.as_bytes()).map_err(|e| e.to_string())
}

pub fn get<'a>(lines: &'a [String], key: &str) -> Option<&'a str> {
    let prefix = format!("{key}:");
    lines.iter().find_map(|l| l.strip_prefix(&prefix))
}

pub fn set(lines: &mut Vec<String>, key: &str, value: &str) {
    let prefix = format!("{key}:");
    match lines.iter_mut().find(|l| l.starts_with(&prefix)) {
        Some(line) => *line = format!("{key}:{value}"),
        None => lines.push(format!("{key}:{value}")),
    }
}

pub fn skip_onboarding(game_dir: &Path) -> Result<(), String> {
    let path = game_dir.join("options.txt");
    let mut lines = read(&path);
    if get(&lines, ONBOARD).is_some() {
        return Ok(());
    }
    set(&mut lines, ONBOARD, "false");
    write(&path, &lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_the_accessibility_screen_once_and_respects_existing_choice() {
        let dir = std::env::temp_dir().join(format!("cxl-options-{}", fastrand::u64(..)));
        skip_onboarding(&dir).unwrap();
        let lines = read(&dir.join("options.txt"));
        assert_eq!(get(&lines, ONBOARD), Some("false"));
        write(&dir.join("options.txt"), &["onboardAccessibility:true".to_string(), "fov:0.5".to_string()]).unwrap();
        skip_onboarding(&dir).unwrap();
        let lines = read(&dir.join("options.txt"));
        assert_eq!(get(&lines, ONBOARD), Some("true"));
        assert_eq!(get(&lines, "fov"), Some("0.5"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
