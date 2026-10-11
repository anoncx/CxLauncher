use super::meta::{Arg, ArgValue, Version};
use super::rules::{allowed, Env};
use std::collections::HashMap;

pub type Vars = HashMap<&'static str, String>;

pub fn substitute(text: &str, vars: &Vars) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match vars.get(key) {
                    Some(value) => out.push_str(value),
                    None => out.push_str(&rest[start..start + 3 + end]),
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn expand(list: &[Arg], env: &Env, vars: &Vars) -> Vec<String> {
    let mut out = Vec::new();
    for arg in list {
        match arg {
            Arg::Plain(s) => out.push(substitute(s, vars)),
            Arg::Ruled { rules, value } => {
                if !allowed(Some(rules), env) {
                    continue;
                }
                match value {
                    ArgValue::One(s) => out.push(substitute(s, vars)),
                    ArgValue::Many(v) => out.extend(v.iter().map(|s| substitute(s, vars))),
                }
            }
        }
    }
    out
}

pub fn jvm(version: &Version, env: &Env, vars: &Vars) -> Vec<String> {
    match &version.arguments {
        Some(a) if !a.jvm.is_empty() => expand(&a.jvm, env, vars),
        _ => ["-Djava.library.path=${natives_directory}", "-cp", "${classpath}"].iter().map(|s| substitute(s, vars)).collect(),
    }
}

pub fn game(version: &Version, env: &Env, vars: &Vars) -> Vec<String> {
    match (&version.arguments, &version.minecraft_arguments) {
        (Some(a), _) if !a.game.is_empty() => expand(&a.game, env, vars),
        (_, Some(legacy)) => legacy.split_whitespace().map(|s| substitute(s, vars)).collect(),
        _ => Vec::new(),
    }
}

pub fn redact(args: &[String]) -> Vec<String> {
    let mut out = args.to_vec();
    for i in 1..out.len() {
        if out[i - 1] == "--accessToken" {
            out[i] = "••••••••".into();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mc::meta::{Arguments, Rule};

    #[test]
    fn substitutes_placeholders() {
        let vars: Vars = [("a", "1".to_string()), ("bb", "two".to_string())].into_iter().collect();
        assert_eq!(substitute("x${a}y${bb}z", &vars), "x1ytwoz");
        assert_eq!(substitute("${missing}-${a}", &vars), "${missing}-1");
        assert_eq!(substitute("open ${a", &vars), "open ${a");
        assert_eq!(substitute("-Dk=${a}", &vars), "-Dk=1");
    }

    #[test]
    fn expands_ruled_arguments() {
        let version = Version {
            arguments: Some(Arguments {
                game: vec![
                    Arg::Plain("--width".into()),
                    Arg::Plain("${resolution_width}".into()),
                    Arg::Ruled {
                        rules: vec![Rule { action: "allow".into(), os: None, features: Some([("is_demo_user".into(), true)].into_iter().collect()) }],
                        value: ArgValue::One("--demo".into()),
                    },
                    Arg::Ruled {
                        rules: vec![Rule { action: "allow".into(), os: None, features: Some([("is_quick_play_multiplayer".into(), true)].into_iter().collect()) }],
                        value: ArgValue::Many(vec!["--quickPlayMultiplayer".into(), "${quickPlayMultiplayer}".into()]),
                    },
                ],
                jvm: vec![],
            }),
            ..Default::default()
        };
        let vars: Vars = [("resolution_width", "1280".to_string()), ("quickPlayMultiplayer", "play.example.net".to_string())].into_iter().collect();
        let env = Env::current().feature("is_quick_play_multiplayer", true);
        assert_eq!(game(&version, &env, &vars), ["--width", "1280", "--quickPlayMultiplayer", "play.example.net"]);
        assert_eq!(jvm(&version, &env, &vars)[1], "-cp");
    }

    #[test]
    fn redacts_token() {
        let args: Vec<String> = ["--username", "Steve", "--accessToken", "secret", "--version", "1"].iter().map(|s| s.to_string()).collect();
        assert_eq!(redact(&args)[3], "••••••••");
        assert_eq!(redact(&args)[1], "Steve");
    }
}
