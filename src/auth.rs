use crate::mc::Endpoints;
use crate::net::{encode, Http, Reply};
use crate::paths::{load_json, write_private, Paths};
use crate::sys::now;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const CLIENT_ID: &str = "00000000402b5328";
const SCOPE: &str = "service::user.auth.xboxlive.com::MBI_SSL";
const REDIRECT: &str = "https://login.live.com/oauth20_desktop.srf";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Account {
    pub uuid: String,
    pub name: String,
    pub refresh: String,
    pub browser: bool,
    pub token: String,
    pub expires: u64,
    pub skin: String,
    pub slim: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Accounts {
    pub list: Vec<Account>,
}

impl Accounts {
    pub fn load(paths: &Paths) -> Self {
        load_json(&paths.accounts())
    }

    pub fn save(&self, paths: &Paths) {
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            let _ = write_private(&paths.accounts(), &bytes);
        }
    }

    pub fn upsert(&mut self, account: Account) {
        match self.list.iter_mut().find(|a| a.uuid == account.uuid) {
            Some(existing) => *existing = account,
            None => self.list.push(account),
        }
    }

    pub fn get(&self, uuid: &str) -> Option<&Account> {
        self.list.iter().find(|a| a.uuid == uuid)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct DeviceCode {
    pub user_code: String,
    pub device_code: String,
    pub verification_uri: String,
    #[serde(default = "five")]
    pub interval: u64,
    #[serde(default = "fifteen_minutes")]
    pub expires_in: u64,
}

fn five() -> u64 {
    5
}

fn fifteen_minutes() -> u64 {
    900
}

#[derive(Clone, Debug, Deserialize)]
pub struct MsToken {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct OAuthError {
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_description: String,
}

fn oauth_failure(reply: &Reply) -> String {
    match reply.json::<OAuthError>() {
        Ok(e) if e.error == "invalid_grant" => "Your Microsoft sign-in has expired. Sign in again.".into(),
        Ok(e) if !e.error_description.is_empty() => e.error_description,
        Ok(e) if !e.error.is_empty() => e.error,
        _ => format!("Microsoft answered {}", reply.status),
    }
}

fn token_url(ep: &Endpoints) -> String {
    format!("{}/oauth20_token.srf", ep.live.trim_end_matches('/'))
}

pub fn authorize_url(ep: &Endpoints) -> String {
    format!(
        "{}/oauth20_authorize.srf?client_id={CLIENT_ID}&response_type=code&scope={}&redirect_uri={}&prompt=login",
        ep.live.trim_end_matches('/'),
        encode(SCOPE),
        encode(REDIRECT)
    )
}

pub fn code_from_redirect(text: &str) -> Option<String> {
    let text = text.trim();
    if !text.contains("oauth20_desktop.srf") {
        return None;
    }
    crate::net::query_param(text, "code").filter(|c| !c.is_empty())
}

pub fn device_code(http: &Http, ep: &Endpoints) -> Result<DeviceCode, String> {
    let url = format!("{}/oauth20_connect.srf", ep.live.trim_end_matches('/'));
    let reply = http.post_form(&url, &[("client_id", CLIENT_ID), ("scope", SCOPE), ("response_type", "device_code")])?;
    if !reply.ok() {
        return Err(oauth_failure(&reply));
    }
    reply.json()
}

pub fn poll(http: &Http, ep: &Endpoints, code: &DeviceCode) -> Result<Option<MsToken>, String> {
    let reply = http.post_form(
        &token_url(ep),
        &[("client_id", CLIENT_ID), ("device_code", &code.device_code), ("grant_type", "urn:ietf:params:oauth:grant-type:device_code")],
    )?;
    if reply.ok() {
        return reply.json().map(Some);
    }
    match reply.json::<OAuthError>().map(|e| e.error).unwrap_or_default().as_str() {
        "authorization_pending" | "slow_down" => Ok(None),
        "expired_token" => Err("The code expired. Start again.".into()),
        "authorization_declined" | "access_denied" => Err("Sign-in was cancelled.".into()),
        _ => Err(oauth_failure(&reply)),
    }
}

pub fn exchange(http: &Http, ep: &Endpoints, code: &str) -> Result<MsToken, String> {
    let reply = http.post_form(
        &token_url(ep),
        &[("client_id", CLIENT_ID), ("code", code), ("grant_type", "authorization_code"), ("redirect_uri", REDIRECT), ("scope", SCOPE)],
    )?;
    if !reply.ok() {
        return Err(oauth_failure(&reply));
    }
    reply.json()
}

fn refresh_ms(http: &Http, ep: &Endpoints, account: &Account) -> Result<MsToken, String> {
    let mut pairs = vec![("client_id", CLIENT_ID), ("refresh_token", account.refresh.as_str()), ("grant_type", "refresh_token"), ("scope", SCOPE)];
    if account.browser {
        pairs.push(("redirect_uri", REDIRECT));
    }
    let reply = http.post_form(&token_url(ep), &pairs)?;
    if !reply.ok() {
        return Err(oauth_failure(&reply));
    }
    reply.json()
}

fn xbox_failure(reply: &Reply) -> String {
    let code = reply.json::<Value>().ok().and_then(|v| v.get("XErr").and_then(Value::as_u64)).unwrap_or(0);
    match code {
        2148916233 => "This Microsoft account has no Xbox profile yet. Sign in once at xbox.com, then try again.".into(),
        2148916235 => "Xbox Live is not available in your country.".into(),
        2148916236 | 2148916237 => "This account needs adult verification on xbox.com first.".into(),
        2148916238 => "This is a child account. An adult has to add it to a Microsoft family first.".into(),
        0 => format!("Xbox Live answered {}", reply.status),
        other => format!("Xbox Live refused the sign-in ({other})"),
    }
}

fn xbox_token(reply: &Reply) -> Result<(String, String), String> {
    let v: Value = reply.json()?;
    let token = v.get("Token").and_then(Value::as_str).ok_or("Xbox Live sent no token")?;
    let uhs = v.pointer("/DisplayClaims/xui/0/uhs").and_then(Value::as_str).ok_or("Xbox Live sent no user hash")?;
    Ok((token.to_string(), uhs.to_string()))
}

const XBOX_HEADERS: &[(&str, &str)] = &[("x-xbl-contract-version", "1")];

fn xbox_user(http: &Http, ep: &Endpoints, ms: &str) -> Result<String, String> {
    let url = format!("{}/user/authenticate", ep.xbox_user.trim_end_matches('/'));
    let mut last = None;
    for prefix in ["t=", "d="] {
        let body = json!({
            "Properties": {"AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": format!("{prefix}{ms}")},
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        });
        let reply = http.post_json(&url, &body, None, XBOX_HEADERS)?;
        if reply.ok() {
            return xbox_token(&reply).map(|(t, _)| t);
        }
        last = Some(reply);
    }
    Err(last.map(|r| xbox_failure(&r)).unwrap_or_default())
}

fn xsts(http: &Http, ep: &Endpoints, user: &str) -> Result<(String, String), String> {
    let url = format!("{}/xsts/authorize", ep.xsts.trim_end_matches('/'));
    let body = json!({
        "Properties": {"SandboxId": "RETAIL", "UserTokens": [user]},
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType": "JWT"
    });
    let reply = http.post_json(&url, &body, None, XBOX_HEADERS)?;
    if !reply.ok() {
        return Err(xbox_failure(&reply));
    }
    xbox_token(&reply)
}

#[derive(Debug, Deserialize)]
struct McLogin {
    access_token: String,
    #[serde(default)]
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct Skin {
    #[serde(default)]
    state: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    variant: String,
}

#[derive(Debug, Deserialize)]
struct McProfile {
    id: String,
    name: String,
    #[serde(default)]
    skins: Vec<Skin>,
}

fn services(ep: &Endpoints, path: &str) -> String {
    format!("{}{path}", ep.services.trim_end_matches('/'))
}

pub fn minecraft(http: &Http, ep: &Endpoints, ms: &MsToken, browser: bool) -> Result<Account, String> {
    let user = xbox_user(http, ep, &ms.access_token)?;
    let (xsts, uhs) = xsts(http, ep, &user)?;
    let reply = http.post_json(&services(ep, "/authentication/login_with_xbox"), &json!({"identityToken": format!("XBL3.0 x={uhs};{xsts}")}), None, &[])?;
    if !reply.ok() {
        return Err(match reply.status {
            403 => "Minecraft refused this sign-in. Try again in a moment.".into(),
            429 => "Too many sign-ins. Wait a minute and try again.".into(),
            s => format!("Minecraft answered {s}"),
        });
    }
    let login: McLogin = reply.json()?;
    let reply = http.get(&services(ep, "/minecraft/profile"), Some(&login.access_token))?;
    if reply.status == 404 {
        return Err("This account does not own Minecraft: Java Edition, or has not picked a name yet.".into());
    }
    if !reply.ok() {
        return Err(format!("Minecraft answered {}", reply.status));
    }
    let profile: McProfile = reply.json()?;
    let skin = profile.skins.iter().find(|s| s.state.eq_ignore_ascii_case("active")).or(profile.skins.first());
    Ok(Account {
        uuid: dashed(&profile.id),
        name: profile.name,
        refresh: ms.refresh_token.clone(),
        browser,
        token: login.access_token,
        expires: now() + if login.expires_in > 0 { login.expires_in } else { 86_400 },
        skin: skin.map(|s| s.url.replace("http://", "https://")).unwrap_or_default(),
        slim: skin.is_some_and(|s| s.variant.eq_ignore_ascii_case("slim")),
    })
}

pub fn fresh(account: &Account) -> bool {
    !account.token.is_empty() && account.expires > now() + 600
}

pub fn renew(http: &Http, ep: &Endpoints, account: &Account) -> Result<Account, String> {
    if account.refresh.is_empty() {
        return Err("Sign in again to play.".into());
    }
    let ms = refresh_ms(http, ep, account)?;
    let mut renewed = minecraft(http, ep, &ms, account.browser)?;
    if renewed.refresh.is_empty() {
        renewed.refresh = account.refresh.clone();
    }
    Ok(renewed)
}

pub fn dashed(id: &str) -> String {
    let id: String = id.chars().filter(|c| c.is_ascii_hexdigit()).collect::<String>().to_ascii_lowercase();
    if id.len() != 32 {
        return id;
    }
    format!("{}-{}-{}-{}-{}", &id[..8], &id[8..12], &id[12..16], &id[16..20], &id[20..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_redirect_codes() {
        assert_eq!(code_from_redirect(" https://login.live.com/oauth20_desktop.srf?code=M.C5_BL2.2.U.x-y&lc=1033 ").as_deref(), Some("M.C5_BL2.2.U.x-y"));
        assert_eq!(code_from_redirect("https://login.live.com/oauth20_desktop.srf?error=access_denied"), None);
        assert_eq!(code_from_redirect("https://example.com/?code=abc"), None);
    }

    #[test]
    fn dashes_uuids() {
        assert_eq!(dashed("069a79f444e94726a5befca90e38aaf5"), "069a79f4-44e9-4726-a5be-fca90e38aaf5");
        assert_eq!(dashed("069A79F4-44E9-4726-A5BE-FCA90E38AAF5"), "069a79f4-44e9-4726-a5be-fca90e38aaf5");
    }

    #[test]
    fn builds_authorize_url() {
        let url = authorize_url(&Endpoints::default());
        assert!(url.starts_with("https://login.live.com/oauth20_authorize.srf?client_id=00000000402b5328&response_type=code"));
        assert!(url.contains("redirect_uri=https%3A%2F%2Flogin.live.com%2Foauth20_desktop.srf"));
    }
}
