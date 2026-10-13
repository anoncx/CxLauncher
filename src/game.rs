use crate::install::Prepared;
use crate::mc::args::{self, Vars};
use crate::mc::java::hide_console;
use crate::mc::rules::Env;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

pub const SEPARATOR: &str = if cfg!(windows) { ";" } else { ":" };
const PENDING_LIMIT: usize = 4000;

#[derive(Clone, Debug, Default)]
pub struct Session {
    pub name: String,
    pub uuid: String,
    pub token: String,
    pub xuid: String,
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub memory: u32,
    pub collector: i32,
    pub jvm_args: String,
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    pub server: String,
}

/// Prism Launcher `UseDiscreteGpu` env block: DRI_PRIME for open drivers,
/// PRIME render offload for proprietary Nvidia. Harmless when absent.
/// The game inherits these from our own env at spawn, so the caller sets
/// them just around `Game::spawn` via [`DiscreteGpuGuard`].
pub fn discrete_gpu_envs() -> [(&'static str, &'static str); 4] {
    [
        ("DRI_PRIME", "1"),
        ("__NV_PRIME_RENDER_OFFLOAD", "1"),
        ("__VK_LAYER_NV_optimus", "NVIDIA_only"),
        ("__GLX_VENDOR_LIBRARY_NAME", "nvidia"),
    ]
}

/// Sets the discrete-GPU envs on this process (inherited by the game
/// at spawn) and restores the previous values on drop.
pub struct DiscreteGpuGuard {
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl DiscreteGpuGuard {
    pub fn apply(enabled: bool) -> Self {
        let mut saved = Vec::new();
        if enabled {
            for (k, v) in discrete_gpu_envs() {
                saved.push((k, std::env::var_os(k)));
                std::env::set_var(k, v);
            }
        }
        Self { saved }
    }
}

impl Drop for DiscreteGpuGuard {
    fn drop(&mut self) {
        for (k, old) in self.saved.drain(..) {
            match old {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}

pub fn split_args(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut started = false;
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            c => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

fn collector_flags(o: &Options, major: u32) -> Vec<String> {
    let mut flags: Vec<&str> = match o.collector {
        1 if major >= 23 => vec!["-XX:+UseZGC"],
        1 => vec!["-XX:+UseZGC", "-XX:+ZGenerational"],
        _ => vec![
            "-XX:+UnlockExperimentalVMOptions",
            "-XX:+UseG1GC",
            "-XX:G1NewSizePercent=20",
            "-XX:G1ReservePercent=20",
            "-XX:MaxGCPauseMillis=50",
            "-XX:G1HeapRegionSize=32M",
        ],
    };
    if o.collector != 1 && major >= 25 {
        flags.push("-XX:+UseCompactObjectHeaders");
    }
    flags.into_iter().map(str::to_string).collect()
}

fn join(paths: &[PathBuf]) -> String {
    paths.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>().join(SEPARATOR)
}

pub fn arguments(p: &Prepared, s: &Session, o: &Options) -> Vec<String> {
    let custom_size = !o.fullscreen && o.width >= 320 && o.height >= 240;
    let env = Env::current().feature("has_custom_resolution", custom_size).feature("is_quick_play_multiplayer", !o.server.trim().is_empty());
    let text = |p: &PathBuf| p.to_string_lossy().into_owned();
    let vars: Vars = [
        ("auth_player_name", s.name.clone()),
        ("version_name", p.version.id.clone()),
        ("game_directory", text(&p.game_dir)),
        ("assets_root", text(&p.assets)),
        ("game_assets", text(&p.game_assets)),
        ("assets_index_name", p.asset_index.clone()),
        ("auth_uuid", s.uuid.replace('-', "")),
        ("auth_access_token", s.token.clone()),
        ("auth_session", format!("token:{}", s.token)),
        ("clientid", String::new()),
        ("auth_xuid", if s.xuid.is_empty() { "0".into() } else { s.xuid.clone() }),
        ("user_type", "msa".into()),
        ("user_properties", "{}".into()),
        ("version_type", "CxLauncher".into()),
        ("natives_directory", text(&p.natives)),
        ("launcher_name", "CxLauncher".into()),
        ("launcher_version", env!("CARGO_PKG_VERSION").into()),
        ("classpath", join(&p.classpath)),
        ("classpath_separator", SEPARATOR.into()),
        ("library_directory", text(&p.libraries)),
        ("resolution_width", o.width.to_string()),
        ("resolution_height", o.height.to_string()),
        ("quickPlayMultiplayer", o.server.trim().to_string()),
    ]
    .into_iter()
    .collect();

    let mut out = vec![format!("-Xmx{}M", o.memory.max(1024)), format!("-Xms{}M", o.memory.clamp(512, 1024))];
    out.extend(collector_flags(o, p.java_major));
    out.extend(split_args(&o.jvm_args));
    out.push(format!("-Dcxlauncher={}", env!("CARGO_PKG_VERSION")));
    if !p.mods.is_empty() {
        out.push(format!("-Dfabric.addMods={}", join(&p.mods)));
    }
    out.extend(args::jvm(&p.version, &env, &vars));
    out.push(p.version.main_class.clone().unwrap_or_else(|| "net.minecraft.client.main.Main".into()));
    out.extend(args::game(&p.version, &env, &vars));
    if o.fullscreen {
        out.push("--fullscreen".into());
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
    Debug,
}

impl Level {
    pub fn index(self) -> i32 {
        match self {
            Level::Info => 0,
            Level::Warn => 1,
            Level::Error => 2,
            Level::Debug => 3,
        }
    }
}

fn tagged(text: &str) -> Option<Level> {
    let head = &text[..text.len().min(120)];
    if head.contains("/ERROR]") || head.contains("/FATAL]") || head.contains("[ERROR]") {
        Some(Level::Error)
    } else if head.contains("/WARN]") || head.contains("[WARN]") {
        Some(Level::Warn)
    } else if head.contains("/DEBUG]") || head.contains("/TRACE]") {
        Some(Level::Debug)
    } else if head.contains("/INFO]") || head.contains("[INFO]") {
        Some(Level::Info)
    } else {
        None
    }
}

#[derive(Default)]
pub struct Log {
    pub pending: VecDeque<(Level, String)>,
    pub dropped: usize,
}

pub struct Game {
    child: Child,
    pub log: Arc<Mutex<Log>>,
}

fn pump(stream: impl Read + Send + 'static, log: Arc<Mutex<Log>>, fallback: Level) {
    std::thread::spawn(move || {
        let mut reader = BufReader::with_capacity(1 << 15, stream);
        let mut buf = Vec::with_capacity(256);
        let mut last = fallback;
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let line = String::from_utf8_lossy(&buf).trim_end().to_string();
            if line.is_empty() {
                continue;
            }
            let level = tagged(&line).unwrap_or(if line.starts_with('\t') || line.starts_with("Caused by") { last } else { fallback });
            last = level;
            let mut log = log.lock().unwrap();
            if log.pending.len() >= PENDING_LIMIT {
                log.pending.pop_front();
                log.dropped += 1;
            }
            log.pending.push_back((level, line));
        }
    });
}

impl Game {
    pub fn spawn(java: &std::path::Path, args: &[String], cwd: &std::path::Path) -> Result<Game, String> {
        let mut cmd = Command::new(java);
        cmd.args(args).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        hide_console(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| format!("could not start Java: {e}"))?;
        let log = Arc::new(Mutex::new(Log::default()));
        if let Some(out) = child.stdout.take() {
            pump(out, log.clone(), Level::Info);
        }
        if let Some(err) = child.stderr.take() {
            pump(err, log.clone(), Level::Warn);
        }
        Ok(Game { child, log })
    }

    pub fn drain(&self) -> Vec<(Level, String)> {
        let mut log = self.log.lock().unwrap();
        log.pending.drain(..).collect()
    }

    pub fn exited(&mut self) -> Option<i32> {
        self.child.try_wait().ok().flatten().map(|s| s.code().unwrap_or(-1))
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }
}

pub fn window_ready(line: &str) -> bool {
    ["Backend library:", "OpenGL", "Created: ", "Sound engine started", "Reloading ResourceManager"].iter().any(|k| line.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_user_arguments() {
        assert_eq!(split_args(r#"-Dx=1  -Dname="a b" "#), ["-Dx=1", "-Dname=a b"]);
        assert!(split_args("   ").is_empty());
    }

    #[test]
    fn reads_levels() {
        assert_eq!(tagged("[12:00:01] [Render thread/WARN]: x"), Some(Level::Warn));
        assert_eq!(tagged("[12:00:01] [main/ERROR]: boom"), Some(Level::Error));
        assert_eq!(tagged("\tat net.minecraft.X"), None);
    }

    #[test]
    fn picks_collector_flags() {
        let g1 = collector_flags(&Options::default(), 25);
        assert!(g1.contains(&"-XX:+UseG1GC".to_string()));
        assert!(g1.contains(&"-XX:+UseCompactObjectHeaders".to_string()));
        let z = collector_flags(&Options { collector: 1, ..Default::default() }, 25);
        assert_eq!(z, ["-XX:+UseZGC"]);
        assert!(!collector_flags(&Options::default(), 21).contains(&"-XX:+UseCompactObjectHeaders".to_string()));
    }
}
