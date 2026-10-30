use crate::mc::Endpoints;
use crate::net::Http;
use regex::Regex;
use serde::Deserialize;
use std::sync::OnceLock;

const MAX_LINES: usize = 25_000;

#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    url: String,
    #[serde(default)]
    error: String,
}

pub fn redact(text: &str, home: Option<&str>) -> String {
    static TOKENS: OnceLock<Regex> = OnceLock::new();
    let tokens = TOKENS.get_or_init(|| Regex::new(r"(?i)(--accessToken\s+|--session\s+|access_token[=:]\s*|bearer\s+|token:)[A-Za-z0-9._\-]{16,}").unwrap());
    let mut out = tokens.replace_all(text, "${1}<hidden>").into_owned();
    if let Some(home) = home.filter(|h| h.len() > 3) {
        for form in [home.to_string(), home.replace('\\', "/"), home.replace('/', "\\")] {
            out = out.replace(&form, "<home>");
        }
    }
    out
}

pub fn prepare(text: &str, home: Option<&str>) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let kept = &lines[lines.len().saturating_sub(MAX_LINES)..];
    redact(&kept.join("\n"), home)
}

pub fn upload(http: &Http, ep: &Endpoints, text: &str) -> Result<String, String> {
    let reply = http.post_form(&ep.logs, &[("content", text)])?;
    let parsed: Reply = reply.json()?;
    if parsed.success && parsed.url.starts_with("https://") {
        Ok(parsed.url)
    } else if parsed.error.is_empty() {
        Err("the log service did not accept the log".into())
    } else {
        Err(parsed.error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_tokens_and_the_home_folder() {
        let log = "java --accessToken eyJhbGciOiJIUzI1NiJ9.abcdefghijklmnop --username Steve\nloading C:\\Users\\steve\\AppData\\x and C:/Users/steve/y";
        let clean = redact(log, Some("C:\\Users\\steve"));
        assert!(!clean.contains("eyJhbGci"));
        assert!(clean.contains("--accessToken <hidden>"));
        assert!(!clean.contains("steve\\AppData") && !clean.contains("Users/steve"));
        assert!(clean.contains("<home>\\AppData") && clean.contains("<home>/y"));
    }

    #[test]
    fn keeps_only_the_latest_lines() {
        let text: String = (0..MAX_LINES + 10).map(|i| format!("line {i}\n")).collect();
        let kept = prepare(&text, None);
        assert_eq!(kept.lines().count(), MAX_LINES);
        assert!(kept.starts_with("line 10"));
    }
}
