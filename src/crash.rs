use regex::Regex;
use std::sync::OnceLock;

const MAX_FINDINGS: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub title: String,
    pub advice: String,
}

fn finding(title: &str, advice: impl Into<String>) -> Finding {
    Finding { title: title.into(), advice: advice.into() }
}

fn mixin_owner(line: &str) -> Option<String> {
    static OWNER: OnceLock<Regex> = OnceLock::new();
    let owner = OWNER.get_or_init(|| Regex::new(r"([A-Za-z0-9_\-]+)\.mixins?\.json|mixins?\.([A-Za-z0-9_\-]+)\.json").unwrap());
    let found = owner.captures(line)?;
    found.get(1).or(found.get(2)).map(|m| m.as_str().to_string())
}

fn inspect(line: &str) -> Option<Finding> {
    if line.contains("UnsupportedClassVersionError") || line.contains("compiled by a more recent version of the Java Runtime") {
        return Some(finding("Java is too old for this version", "Leave Java on Managed in Settings, or pick a newer Java. Modern Minecraft versions need Java 21 or newer."));
    }
    if line.contains("OutOfMemoryError") {
        return Some(finding("Minecraft ran out of memory", "Raise the memory in Settings or in the profile's Advanced tab, and remove heavy mods or shaders."));
    }
    if line.contains("Mixin apply failed") || line.contains("MixinApplyError") || line.contains("InvalidInjectionException") || line.contains("Mixin transformation of") {
        let who = mixin_owner(line).map(|m| format!(" ({m})")).unwrap_or_default();
        return Some(finding("A mod does not fit this game", format!("A mod{who} failed to patch Minecraft. Update it, or remove it if it was made for another Minecraft version.")));
    }
    if line.contains("Duplicate mod") {
        return Some(finding("The same mod is installed twice", "Remove the extra copy from the mods folder."));
    }
    if line.contains("Could not find required mod") || (line.contains("Mod '") && line.contains("requires")) {
        return Some(finding("A mod is missing something it needs", line.trim().trim_start_matches('-').trim().to_string()));
    }
    if line.contains("GLFW error 65542") || line.contains("The driver does not appear to support OpenGL") || line.contains("Pixel format not accelerated") {
        return Some(finding("The graphics driver could not start OpenGL", "Update the graphics driver. On a laptop, turn on Discrete GPU in Settings."));
    }
    None
}

pub fn analyze<'a>(lines: impl IntoIterator<Item = &'a str>) -> Vec<Finding> {
    let mut out: Vec<Finding> = Vec::new();
    for line in lines {
        if let Some(found) = inspect(line) {
            if !out.iter().any(|f| f.title == found.title) {
                out.push(found);
                if out.len() == MAX_FINDINGS {
                    break;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_common_failures() {
        let log = [
            "[12:00:00] [main/INFO]: Loading",
            "java.lang.OutOfMemoryError: Java heap space",
            "Caused by: org.spongepowered.asm.mixin.injection.throwables.InvalidInjectionException: sodium.mixins.json:core.Foo",
            "Exception in thread \"main\" java.lang.UnsupportedClassVersionError: x has been compiled by a more recent version",
        ];
        let found = analyze(log);
        assert_eq!(found.len(), 3);
        assert!(found.iter().any(|f| f.title.contains("memory")));
        assert!(found.iter().any(|f| f.advice.contains("(sodium)")));
        assert!(found.iter().any(|f| f.title.contains("Java is too old")));
    }

    #[test]
    fn reports_each_problem_once_and_ignores_normal_output() {
        let log = ["java.lang.OutOfMemoryError", "java.lang.OutOfMemoryError: again", "[12:00:00] [main/INFO]: all good"];
        assert_eq!(analyze(log).len(), 1);
        assert!(analyze(["[12:00:00] [main/INFO]: all good"]).is_empty());
    }

    #[test]
    fn reads_missing_dependencies() {
        let found = analyze(["\t - Mod 'Foo' (foo) 1.0 requires any version of bar, which is missing!"]);
        assert_eq!(found[0].title, "A mod is missing something it needs");
        assert!(found[0].advice.starts_with("Mod 'Foo'"));
    }
}
