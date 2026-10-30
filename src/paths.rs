use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn default_root() -> PathBuf {
        if let Some(dir) = std::env::var_os("CXLAUNCHER_HOME").filter(|d| !d.is_empty()) {
            return PathBuf::from(dir);
        }
        dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("CxLauncher")
    }

    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn accounts(&self) -> PathBuf {
        self.root.join("accounts.json")
    }

    pub fn profiles_file(&self) -> PathBuf {
        self.root.join("profiles.json")
    }

    pub fn meta(&self) -> PathBuf {
        self.root.join("meta")
    }

    pub fn version_dir(&self, id: &str) -> PathBuf {
        self.root.join("versions").join(id)
    }

    pub fn version_jar(&self, id: &str) -> PathBuf {
        self.version_dir(id).join(format!("{id}.jar"))
    }

    pub fn libraries(&self) -> PathBuf {
        self.root.join("libraries")
    }

    pub fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }

    pub fn natives(&self, version: &str) -> PathBuf {
        self.root.join("natives").join(version)
    }

    pub fn runtime(&self, component: &str) -> PathBuf {
        self.root.join("runtime").join(component)
    }

    pub fn managed(&self, name: &str) -> PathBuf {
        self.root.join("managed").join(name)
    }

    pub fn profile(&self, id: &str) -> PathBuf {
        self.root.join("profiles").join(id)
    }

    pub fn icon(&self, id: &str) -> PathBuf {
        self.root.join("icons").join(format!("{id}.png"))
    }

    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("{}.cxtmp", fastrand::u32(..)));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_atomic(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn load_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save_json<T: serde::Serialize>(path: &Path, value: &T) {
    if let Ok(s) = serde_json::to_vec_pretty(value) {
        let _ = write_atomic(path, &s);
    }
}
