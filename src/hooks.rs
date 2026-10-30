use crate::game::split_args;
use crate::mc::java::hide_console;
use crate::store::Profile;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

const PRE_LAUNCH_LIMIT: Duration = Duration::from_secs(120);

pub fn parse_env(text: &str) -> Vec<(String, String)> {
    split_args(text)
        .into_iter()
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            let valid = !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            valid.then(|| (key.to_string(), value.to_string()))
        })
        .collect()
}

pub fn variables(profile: &Profile, dir: &Path, game: &str) -> Vec<(String, String)> {
    let dir = dir.to_string_lossy().into_owned();
    vec![
        ("INST_ID".into(), profile.id.clone()),
        ("INST_NAME".into(), profile.name.clone()),
        ("INST_DIR".into(), dir.clone()),
        ("INST_MC_DIR".into(), dir),
        ("INST_MC_VER".into(), game.to_string()),
    ]
}

fn build(text: &str, vars: &[(String, String)], cwd: &Path) -> Option<Command> {
    let mut parts = split_args(text).into_iter();
    let mut cmd = Command::new(parts.next()?);
    cmd.args(parts).envs(vars.iter().map(|(k, v)| (k, v))).current_dir(cwd);
    hide_console(&mut cmd);
    Some(cmd)
}

pub fn run_before(text: &str, vars: &[(String, String)], cwd: &Path) -> Result<(), String> {
    let Some(mut cmd) = build(text, vars, cwd) else { return Ok(()) };
    let mut child = cmd.spawn().map_err(|e| format!("The pre-launch command could not start: {e}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("The pre-launch command failed ({status})")),
            Ok(None) if started.elapsed() > PRE_LAUNCH_LIMIT => {
                let _ = child.kill();
                return Err("The pre-launch command took too long and was stopped".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

pub fn run_after(text: &str, vars: &[(String, String)], cwd: &Path) {
    if let Some(mut cmd) = build(text, vars, cwd) {
        let _ = cmd.spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_environment_pairs_and_drops_invalid_ones() {
        let env = parse_env(r#"MESA_GL_VERSION_OVERRIDE=4.5 BAD-KEY=1 =x NAME="a b" plain"#);
        assert_eq!(env, [("MESA_GL_VERSION_OVERRIDE".to_string(), "4.5".to_string()), ("NAME".to_string(), "a b".to_string())]);
    }

    #[test]
    fn empty_commands_do_nothing_and_failures_are_reported() {
        let dir = std::env::temp_dir();
        assert!(run_before("  ", &[], &dir).is_ok());
        assert!(run_before("cxlauncher-no-such-command", &[], &dir).is_err());
    }

    #[test]
    fn exposes_instance_variables() {
        let profile = Profile { id: "abc".into(), name: "Survival".into(), ..Default::default() };
        let vars = variables(&profile, Path::new("/tmp/x"), "1.21.1");
        assert!(vars.contains(&("INST_ID".to_string(), "abc".to_string())));
        assert!(vars.contains(&("INST_MC_VER".to_string(), "1.21.1".to_string())));
    }
}
