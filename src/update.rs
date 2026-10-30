use crate::mc::Endpoints;
use crate::download::hex;
use crate::net::{host, Http};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const LIMIT: u64 = 256 << 20;
const TRUSTED: [&str; 3] = ["github.com", "objects.githubusercontent.com", "visuals.cxiy.dev"];

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct File {
    file: String,
    kind: String,
    platform: String,
    url: String,
    size: u64,
    sha256: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct Manifest {
    version: String,
    notes: String,
    page: String,
    files: Vec<File>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Method {
    Replace(PathBuf),
    Installer,
    Page,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub page: String,
    asset: Option<File>,
    pub method: Method,
}

pub fn newer(current: &str, candidate: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> { v.trim_start_matches('v').split(['.', '-', '+']).map_while(|p| p.parse().ok()).collect() };
    let (a, b) = (parts(current), parts(candidate));
    !b.is_empty() && b > a
}

fn trusted(url: &str) -> bool {
    url.starts_with("https://") && TRUSTED.contains(&host(url))
}

fn platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

fn decide(platform: &str, exe: &Path, appimage: Option<&Path>) -> (Method, &'static str) {
    match platform {
        "windows" if exe.with_file_name("uninstall.exe").is_file() => (Method::Installer, "installer"),
        "windows" => (Method::Replace(exe.to_path_buf()), "portable"),
        _ => match appimage {
            Some(image) => (Method::Replace(image.to_path_buf()), "appimage"),
            None => (Method::Page, ""),
        },
    }
}

fn read(manifest: Manifest, platform: &str, exe: &Path, appimage: Option<&Path>) -> Release {
    let (method, kind) = decide(platform, exe, appimage);
    let asset = manifest.files.into_iter().find(|f| f.platform == platform && f.kind == kind && trusted(&f.url) && f.file.ends_with(if platform == "windows" { ".exe" } else { ".AppImage" }));
    let method = if asset.is_some() { method } else { Method::Page };
    Release { version: manifest.version.trim_start_matches('v').to_string(), page: manifest.page, asset, method }
}

pub fn check(http: &Http, ep: &Endpoints) -> Result<Option<Release>, String> {
    let manifest: Manifest = http.json(&ep.launcher)?;
    if !newer(env!("CARGO_PKG_VERSION"), &manifest.version) {
        return Ok(None);
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file());
    Ok(Some(read(manifest, platform(), &exe, appimage.as_deref())))
}

pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("old"));
    }
}

fn fetch(http: &Http, asset: &File) -> Result<Vec<u8>, String> {
    let data = http.bytes(&asset.url, LIMIT)?;
    if asset.size > 0 && data.len() as u64 != asset.size {
        return Err("the download is incomplete, try again".into());
    }
    if !asset.sha256.is_empty() && !hex(&Sha256::digest(&data)).eq_ignore_ascii_case(&asset.sha256) {
        return Err("the download does not match its checksum".into());
    }
    Ok(data)
}

fn write_executable(path: &Path, data: &[u8]) -> Result<(), String> {
    std::fs::write(path, data).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn swap(target: &Path, data: &[u8]) -> Result<(), String> {
    let staged = target.with_extension("new");
    write_executable(&staged, data)?;
    let result = if cfg!(windows) {
        let old = target.with_extension("old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(target, &old).and_then(|_| std::fs::rename(&staged, target)).map_err(|e| e.to_string())
    } else {
        std::fs::rename(&staged, target).map_err(|e| e.to_string())
    };
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    result
}

pub fn install(http: &Http, release: &Release, temp: &Path) -> Result<PathBuf, String> {
    let asset = release.asset.as_ref().ok_or("this build has no matching download")?;
    let data = fetch(http, asset)?;
    match &release.method {
        Method::Replace(target) => {
            swap(target, &data)?;
            Ok(target.clone())
        }
        Method::Installer => {
            let path = temp.join(&asset.file);
            write_executable(&path, &data)?;
            Ok(path)
        }
        Method::Page => Err("this install is updated from the release page".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(files: &[(&str, &str, &str, &str)]) -> Manifest {
        Manifest {
            version: "0.2.0".into(),
            page: "https://github.com/anoncx/CxLauncher/releases/tag/v0.2.0".into(),
            files: files
                .iter()
                .map(|(file, platform, kind, url)| File { file: (*file).into(), platform: (*platform).into(), kind: (*kind).into(), url: (*url).into(), ..Default::default() })
                .collect(),
            ..Default::default()
        }
    }

    const RELEASE: &str = "https://github.com/anoncx/CxLauncher/releases/download/v0.2.0/";

    fn files() -> Manifest {
        manifest(&[
            ("a-setup.exe", "windows", "installer", &format!("{RELEASE}a-setup.exe")),
            ("a-portable.exe", "windows", "portable", &format!("{RELEASE}a-portable.exe")),
            ("a.AppImage", "linux", "appimage", &format!("{RELEASE}a.AppImage")),
            ("a.deb", "linux", "deb", &format!("{RELEASE}a.deb")),
        ])
    }

    #[test]
    fn compares_versions_numerically() {
        assert!(newer("0.1.12", "0.1.13"));
        assert!(newer("0.1.9", "0.1.10"));
        assert!(newer("0.1.12", "v0.2.0"));
        assert!(!newer("0.1.12", "0.1.12"));
        assert!(!newer("0.2.0", "0.1.99"));
        assert!(!newer("0.1.0", ""));
    }

    #[test]
    fn windows_portable_replaces_itself_and_installed_runs_the_installer() {
        let dir = std::env::temp_dir().join(format!("cxl-update-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("cxlauncher.exe");
        let portable = read(files(), "windows", &exe, None);
        assert_eq!(portable.method, Method::Replace(exe.clone()));
        assert_eq!(portable.asset.as_ref().unwrap().file, "a-portable.exe");
        std::fs::write(dir.join("uninstall.exe"), b"").unwrap();
        let installed = read(files(), "windows", &exe, None);
        assert_eq!(installed.method, Method::Installer);
        assert_eq!(installed.asset.as_ref().unwrap().file, "a-setup.exe");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn linux_replaces_an_appimage_and_sends_other_installs_to_the_page() {
        let exe = Path::new("/usr/bin/cxlauncher");
        let image = Path::new("/home/me/CxLauncher.AppImage");
        let appimage = read(files(), "linux", exe, Some(image));
        assert_eq!(appimage.method, Method::Replace(image.to_path_buf()));
        assert_eq!(appimage.asset.as_ref().unwrap().file, "a.AppImage");
        let packaged = read(files(), "linux", exe, None);
        assert_eq!(packaged.method, Method::Page);
    }

    #[test]
    fn untrusted_or_missing_downloads_fall_back_to_the_page() {
        let exe = Path::new("/tmp/cxlauncher.exe");
        let hostile = manifest(&[("a-portable.exe", "windows", "portable", "https://evil.example/a-portable.exe")]);
        assert_eq!(read(hostile, "windows", exe, None).method, Method::Page);
        let plain = manifest(&[("a-portable.exe", "windows", "portable", "http://github.com/a-portable.exe")]);
        assert_eq!(read(plain, "windows", exe, None).method, Method::Page);
        assert_eq!(read(manifest(&[]), "linux", exe, Some(exe)).method, Method::Page);
    }

    #[test]
    fn swaps_the_executable_and_keeps_the_old_one() {
        let dir = std::env::temp_dir().join(format!("cxl-swap-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("cxlauncher.exe");
        std::fs::write(&target, b"old").unwrap();
        swap(&target, b"new").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!target.with_extension("new").exists());
        if cfg!(windows) {
            assert_eq!(std::fs::read(target.with_extension("old")).unwrap(), b"old");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
