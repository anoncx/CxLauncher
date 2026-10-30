use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub launch: Option<String>,
    pub world: Option<String>,
    pub import: Option<PathBuf>,
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Args {
    let mut out = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--launch" => out.launch = args.next().filter(|v| !v.is_empty()),
            "--world" => out.world = args.next().filter(|v| !v.is_empty()),
            "--import" => out.import = args.next().filter(|v| !v.is_empty()).map(PathBuf::from),
            other => {
                if let Some(value) = other.strip_prefix("--launch=") {
                    out.launch = Some(value.to_string()).filter(|v| !v.is_empty());
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Args {
        parse(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn reads_launch_world_and_import() {
        let a = args(&["--launch", "Survival", "--world", "New World", "--import", "pack.mrpack"]);
        assert_eq!(a.launch.as_deref(), Some("Survival"));
        assert_eq!(a.world.as_deref(), Some("New World"));
        assert_eq!(a.import, Some(PathBuf::from("pack.mrpack")));
        assert_eq!(args(&["--launch=abc"]).launch.as_deref(), Some("abc"));
    }

    #[test]
    fn ignores_unknown_and_missing_values() {
        assert_eq!(args(&["--verbose", "--launch"]), Args::default());
        assert_eq!(args(&[]), Args::default());
    }
}
