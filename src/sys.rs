use std::sync::OnceLock;

#[derive(Clone, Copy, Debug)]
pub struct Machine {
    pub memory_mb: u32,
    pub cores: u32,
}

impl Machine {
    pub fn get() -> Machine {
        static MACHINE: OnceLock<Machine> = OnceLock::new();
        *MACHINE.get_or_init(|| Machine {
            memory_mb: total_memory_mb().unwrap_or(8192),
            cores: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(4),
        })
    }

    pub fn low_end(&self) -> bool {
        self.memory_mb < 6 * 1024 || self.cores <= 2
    }

    pub fn recommended_memory(&self) -> u32 {
        match self.memory_mb {
            0..=4608 => 2048,
            4609..=9216 => 3072,
            9217..=17408 => 4096,
            _ => 6144,
        }
    }

    pub fn max_memory(&self) -> u32 {
        (self.memory_mb.saturating_sub(1024) / 512 * 512).max(1024)
    }

    pub fn download_threads(&self) -> usize {
        if self.low_end() {
            8
        } else {
            16
        }
    }
}

#[cfg(target_os = "linux")]
fn total_memory_mb() -> Option<u32> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb: u64 = info.lines().find_map(|l| l.strip_prefix("MemTotal:"))?.trim().trim_end_matches("kB").trim().parse().ok()?;
    Some((kb / 1024) as u32)
}

#[cfg(windows)]
fn total_memory_mb() -> Option<u32> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..unsafe { std::mem::zeroed() } };
    (unsafe { GlobalMemoryStatusEx(&mut status) } != 0).then_some((status.ullTotalPhys / (1024 * 1024)) as u32)
}

#[cfg(target_os = "macos")]
fn total_memory_mb() -> Option<u32> {
    let out = std::process::Command::new("sysctl").args(["-n", "hw.memsize"]).output().ok()?;
    let bytes: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some((bytes / (1024 * 1024)) as u32)
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn total_memory_mb() -> Option<u32> {
    None
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn reveal(dir: &std::path::Path) {
    let _ = std::fs::create_dir_all(dir);
    open(&dir.to_string_lossy());
}

pub fn open(target: &str) {
    let target = target.to_string();
    std::thread::spawn(move || {
        let _ = open::that_detached(target);
    });
}

pub fn open_url(target: &str) {
    if is_web_url(target) {
        open(target);
    }
}

fn is_web_url(target: &str) -> bool {
    let Some((scheme, rest)) = target.split_once("://") else { return false };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    !rest.is_empty() && !target.chars().any(|c| c.is_whitespace() || c.is_control())
}

pub fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else if mb >= 10.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

pub fn ago(then: u64, now: u64) -> String {
    if then == 0 {
        return "Never".into();
    }
    let s = now.saturating_sub(then);
    match s {
        0..=59 => "Just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h ago", s / 3600),
        86_400..=172_799 => "Yesterday".into(),
        _ => format!("{} days ago", s / 86_400),
    }
}

pub fn duration(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        _ => format!("{}h {}m", secs / 3600, secs % 3600 / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_web_links_only() {
        assert!(is_web_url("https://modrinth.com/mod/sodium"));
        assert!(is_web_url("http://localhost:8080/callback"));
        assert!(is_web_url("HTTPS://Modrinth.com"));
        assert!(!is_web_url("file:///etc/passwd"));
        assert!(!is_web_url("javascript:alert(1)"));
        assert!(!is_web_url("ms-msdt:/id"));
        assert!(!is_web_url("smb://host/share"));
        assert!(!is_web_url("\\\\host\\share\\run.exe"));
        assert!(!is_web_url("C:\\Windows\\system32\\calc.exe"));
        assert!(!is_web_url("/usr/bin/sh"));
        assert!(!is_web_url("https://"));
        assert!(!is_web_url(" https://x"));
        assert!(!is_web_url("https://x\nfile:///y"));
        assert!(!is_web_url(""));
    }

    #[test]
    fn recommends_memory() {
        let m = |memory_mb| Machine { memory_mb, cores: 8 };
        assert_eq!(m(4096).recommended_memory(), 2048);
        assert_eq!(m(8192).recommended_memory(), 3072);
        assert_eq!(m(16384).recommended_memory(), 4096);
        assert_eq!(m(65536).recommended_memory(), 6144);
        assert!(m(4096).low_end());
        assert_eq!(m(8000).max_memory(), 6656);
    }

    #[test]
    fn formats_values() {
        assert_eq!(megabytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(megabytes(300 * 1024 * 1024), "300 MB");
        assert_eq!(megabytes(3 * 1024 * 1024 * 1024), "3.0 GB");
        assert_eq!(ago(100, 100 + 7200), "2 h ago");
        assert_eq!(duration(3725), "1h 2m");
    }
}
