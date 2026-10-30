use std::path::PathBuf;

fn file_name(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).collect();
    let clean = clean.trim();
    if clean.is_empty() { "CxLauncher".into() } else { clean.to_string() }
}

#[cfg(windows)]
pub fn create(id: &str, name: &str) -> Result<PathBuf, String> {
    use crate::mc::java::hide_console;
    use std::process::Command;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = dirs::desktop_dir().ok_or("there is no desktop folder")?;
    let path = dir.join(format!("{} (Minecraft).lnk", file_name(name)));
    let script = "$s = (New-Object -ComObject WScript.Shell).CreateShortcut($env:CX_LINK); $s.TargetPath = $env:CX_EXE; $s.Arguments = $env:CX_ARGS; $s.WorkingDirectory = Split-Path $env:CX_EXE; $s.Save()";
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]).env("CX_LINK", &path).env("CX_EXE", &exe).env("CX_ARGS", format!("--launch \"{id}\""));
    hide_console(&mut cmd);
    let status = cmd.status().map_err(|e| e.to_string())?;
    if status.success() && path.exists() {
        Ok(path)
    } else {
        Err("the shortcut could not be created".into())
    }
}

#[cfg(not(windows))]
pub fn create(id: &str, name: &str) -> Result<PathBuf, String> {
    let exe = std::env::var_os("APPIMAGE").map(PathBuf::from).or_else(|| std::env::current_exe().ok()).ok_or("the launcher path is unknown")?;
    let dir = dirs::desktop_dir().or_else(dirs::data_dir).ok_or("there is no desktop folder")?;
    let path = dir.join(format!("{} (Minecraft).desktop", file_name(name)));
    let entry = format!("[Desktop Entry]\nType=Application\nName={name} (Minecraft)\nExec=\"{}\" --launch \"{id}\"\nTerminal=false\nCategories=Game;\n", exe.display());
    std::fs::write(&path, entry).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_safe_file_names() {
        assert_eq!(file_name("My/Pack: 1.21"), "My_Pack_ 1_21");
        assert_eq!(file_name("   "), "CxLauncher");
    }
}
