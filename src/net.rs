use serde::de::DeserializeOwned;
use std::io::Read;
use std::time::Duration;
use ureq::tls::{RootCerts, TlsConfig};
use ureq::{Agent, RequestBuilder};

pub const USER_AGENT: &str = concat!("CxLauncher/", env!("CARGO_PKG_VERSION"), " (github.com/anoncx/CxLauncher)");

const TEXT_LIMIT: u64 = 64 << 20;

#[derive(Clone)]
pub struct Http {
    agent: Agent,
    direct: Option<Agent>,
}

pub struct Reply {
    pub status: u16,
    pub body: String,
}

impl Reply {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn json<T: DeserializeOwned>(&self) -> Result<T, String> {
        serde_json::from_str(&self.body).map_err(|e| format!("unexpected answer ({e})"))
    }
}

const OFFLINE: [&str; 3] = ["the connection timed out", "you seem to be offline", "could not connect"];

fn reason(e: ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(code) => format!("server answered {code}"),
        ureq::Error::Timeout(_) => OFFLINE[0].into(),
        ureq::Error::HostNotFound => OFFLINE[1].into(),
        ureq::Error::ConnectionFailed => OFFLINE[2].into(),
        e => e.to_string(),
    }
}

pub fn is_offline(error: &str) -> bool {
    OFFLINE.iter().any(|o| error.contains(o))
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

fn base() -> ureq::config::ConfigBuilder<ureq::typestate::AgentScope> {
    Agent::config_builder().user_agent(USER_AGENT).http_status_as_error(false)
}

fn timeouts(b: ureq::config::ConfigBuilder<ureq::typestate::AgentScope>) -> ureq::config::ConfigBuilder<ureq::typestate::AgentScope> {
    b.timeout_connect(Some(Duration::from_secs(12)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(900)))
        .max_idle_connections_per_host(48)
        .tls_config(TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).build())
}

fn proxy_env() -> bool {
    ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"]
        .iter()
        .any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()))
}

static PROXY_FALLBACK_WARNED: std::sync::Once = std::sync::Once::new();

fn transport_failed(e: &ureq::Error) -> bool {
    matches!(
        e,
        ureq::Error::ConnectionFailed | ureq::Error::HostNotFound | ureq::Error::Timeout(_) | ureq::Error::ConnectProxyFailed(_) | ureq::Error::InvalidProxyUrl
    )
}

fn authorized<B>(req: RequestBuilder<B>, bearer: Option<&str>) -> RequestBuilder<B> {
    let req = req.header("Accept", "application/json");
    match bearer {
        Some(token) => req.header("Authorization", format!("Bearer {token}")),
        None => req,
    }
}

fn read(res: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<Reply, String> {
    let mut res = res.map_err(reason)?;
    let status = res.status().as_u16();
    let body = res.body_mut().with_config().limit(TEXT_LIMIT).read_to_string().map_err(reason)?;
    Ok(Reply { status, body })
}

impl Http {
    pub fn new() -> Self {
        let agent = timeouts(base()).build().into();
        let direct = proxy_env().then(|| timeouts(base().proxy(None)).build().into());
        Self { agent, direct }
    }

    #[cfg(test)]
    pub fn direct() -> Self {
        Self { agent: base().proxy(None).timeout_connect(Some(Duration::from_secs(5))).build().into(), direct: None }
    }

    pub(crate) fn send(&self, send: impl Fn(&Agent) -> Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
        match send(&self.agent) {
            Err(e) if transport_failed(&e) => match &self.direct {
                Some(direct) => {
                    PROXY_FALLBACK_WARNED.call_once(|| eprintln!("CxLauncher: proxy connection failed ({e}), retrying with a direct connection"));
                    send(direct)
                }
                None => send(&self.agent),
            },
            other => other,
        }
    }

    pub fn get(&self, url: &str, bearer: Option<&str>) -> Result<Reply, String> {
        read(self.send(|a| authorized(a.get(url), bearer).call()))
    }

    pub fn text(&self, url: &str) -> Result<String, String> {
        let reply = self.get(url, None)?;
        if !reply.ok() {
            return Err(format!("{} answered {}", host(url), reply.status));
        }
        Ok(reply.body)
    }

    pub fn json<T: DeserializeOwned>(&self, url: &str) -> Result<T, String> {
        let text = self.text(url)?;
        serde_json::from_str(&text).map_err(|e| format!("unexpected answer from {} ({e})", host(url)))
    }

    pub fn bytes(&self, url: &str, limit: u64) -> Result<Vec<u8>, String> {
        let mut res = self.send(|a| a.get(url).call()).map_err(reason)?;
        let status = res.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(format!("{} answered {status}", host(url)));
        }
        let mut out = Vec::new();
        res.body_mut().as_reader().take(limit).read_to_end(&mut out).map_err(|e| e.to_string())?;
        Ok(out)
    }

    pub fn post_form(&self, url: &str, pairs: &[(&str, &str)]) -> Result<Reply, String> {
        read(self.send(|a| authorized(a.post(url), None).header("Content-Type", "application/x-www-form-urlencoded").send(form(pairs))))
    }

    pub fn post_json(&self, url: &str, body: &serde_json::Value, bearer: Option<&str>, extra: &[(&str, &str)]) -> Result<Reply, String> {
        read(self.send(|a| {
            let mut req = authorized(a.post(url), bearer).header("Content-Type", "application/json");
            for (k, v) in extra {
                req = req.header(*k, *v);
            }
            req.send(body.to_string())
        }))
    }
}

pub fn host(url: &str) -> &str {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    rest.split(['/', '?']).next().unwrap_or(rest)
}

pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn form(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", encode(k), encode(v))).collect::<Vec<_>>().join("&")
}

pub fn query_param(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1.split('#').next()?;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == key {
            return Some(decode(v));
        }
    }
    None
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push(hi << 4 | lo);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_forms() {
        assert_eq!(form(&[("scope", "service::a b"), ("x", "1")]), "scope=service%3A%3Aa%20b&x=1");
    }

    #[test]
    fn reads_query() {
        let url = "https://login.live.com/oauth20_desktop.srf?code=M.C5_BAY.2.U.abc%21x&lc=1033";
        assert_eq!(query_param(url, "code").as_deref(), Some("M.C5_BAY.2.U.abc!x"));
        assert_eq!(query_param(url, "lc").as_deref(), Some("1033"));
        assert_eq!(query_param(url, "nope"), None);
        assert_eq!(query_param("https://x/?a=%2", "a").as_deref(), Some("%2"));
    }

    #[test]
    fn finds_host() {
        assert_eq!(host("https://api.modrinth.com/v2/search?q=1"), "api.modrinth.com");
    }

    #[test]
    fn retries_on_transport_failures_only() {
        for e in [
            ureq::Error::HostNotFound,
            ureq::Error::ConnectionFailed,
            ureq::Error::ConnectProxyFailed("x".into()),
            ureq::Error::InvalidProxyUrl,
            ureq::Error::Timeout(ureq::Timeout::Global),
        ] {
            assert!(transport_failed(&e), "{e:?} should fall back");
        }
        assert!(!transport_failed(&ureq::Error::StatusCode(500)));
    }
}
