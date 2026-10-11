use super::meta::Rule;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Env {
    pub os: &'static str,
    pub arch: &'static str,
    pub os_version: String,
    pub features: HashMap<String, bool>,
}

impl Env {
    pub fn current() -> Self {
        let os = match std::env::consts::OS {
            "windows" => "windows",
            "macos" => "osx",
            _ => "linux",
        };
        let arch = match std::env::consts::ARCH {
            "x86" => "x86",
            "aarch64" => "arm64",
            "arm" => "arm32",
            _ => "x86_64",
        };
        Self { os, arch, os_version: String::new(), features: HashMap::new() }
    }

    pub fn feature(mut self, name: &str, on: bool) -> Self {
        self.features.insert(name.to_string(), on);
        self
    }

    pub fn bits(&self) -> &'static str {
        if matches!(self.arch, "x86" | "arm32") {
            "32"
        } else {
            "64"
        }
    }
}

fn os_matches(rule: &Rule, env: &Env) -> bool {
    let Some(os) = &rule.os else { return true };
    if let Some(name) = &os.name {
        let name = name.as_str();
        let ok = name == env.os
            || (name == "osx-arm64" && env.os == "osx" && env.arch == "arm64")
            || (name == "linux-arm64" && env.os == "linux" && env.arch == "arm64");
        if !ok {
            return false;
        }
    }
    if let Some(arch) = &os.arch {
        let ok = match arch.as_str() {
            "x86" => env.arch == "x86",
            "x86_64" | "amd64" => env.arch == "x86_64",
            "arm64" | "aarch64" => env.arch == "arm64",
            other => other == env.arch,
        };
        if !ok {
            return false;
        }
    }
    if let Some(version) = &os.version {
        if !version_matches(version, &env.os_version) {
            return false;
        }
    }
    true
}

fn version_matches(pattern: &str, version: &str) -> bool {
    let Some(prefix) = pattern.strip_prefix('^') else { return false };
    let literal = prefix.replace("\\.", ".");
    if literal.chars().any(|c| "[]()*+?|\\$".contains(c)) {
        return false;
    }
    !version.is_empty() && version.starts_with(&literal)
}

fn features_match(rule: &Rule, env: &Env) -> bool {
    let Some(features) = &rule.features else { return true };
    features.iter().all(|(name, want)| env.features.get(name).copied().unwrap_or(false) == *want)
}

pub fn allowed(rules: Option<&[Rule]>, env: &Env) -> bool {
    let Some(rules) = rules else { return true };
    if rules.is_empty() {
        return true;
    }
    let mut allow = false;
    for rule in rules {
        if os_matches(rule, env) && features_match(rule, env) {
            allow = rule.action == "allow";
        }
    }
    allow
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mc::meta::OsRule;

    fn env(os: &'static str, arch: &'static str) -> Env {
        Env { os, arch, os_version: "10.0.22631".into(), features: HashMap::new() }
    }

    fn rule(action: &str, os: Option<&str>, arch: Option<&str>) -> Rule {
        Rule {
            action: action.into(),
            os: Some(OsRule { name: os.map(Into::into), arch: arch.map(Into::into), version: None }),
            features: None,
        }
    }

    #[test]
    fn evaluates_os_rules() {
        let only_mac = vec![rule("allow", Some("osx"), None)];
        assert!(allowed(Some(&only_mac), &env("osx", "x86_64")));
        assert!(!allowed(Some(&only_mac), &env("linux", "x86_64")));

        let not_mac = vec![Rule { action: "allow".into(), os: None, features: None }, rule("disallow", Some("osx"), None)];
        assert!(allowed(Some(&not_mac), &env("windows", "x86_64")));
        assert!(!allowed(Some(&not_mac), &env("osx", "arm64")));

        let x86 = vec![rule("allow", None, Some("x86"))];
        assert!(allowed(Some(&x86), &env("windows", "x86")));
        assert!(!allowed(Some(&x86), &env("windows", "x86_64")));
        assert!(allowed(None, &env("linux", "x86_64")));
    }

    #[test]
    fn evaluates_features() {
        let demo = vec![Rule {
            action: "allow".into(),
            os: None,
            features: Some([("is_demo_user".to_string(), true)].into_iter().collect()),
        }];
        assert!(!allowed(Some(&demo), &env("linux", "x86_64")));
        assert!(allowed(Some(&demo), &env("linux", "x86_64").feature("is_demo_user", true)));
    }

    #[test]
    fn matches_version_prefix() {
        assert!(version_matches("^10\\.", "10.0.22631"));
        assert!(!version_matches("^10\\.", "6.1"));
        assert!(!version_matches("^1[0-9]", "10.0"));
    }
}
