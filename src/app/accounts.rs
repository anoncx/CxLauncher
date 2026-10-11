use super::images::{pixels, Pixels};
use super::{background, later, App};
use crate::auth::{self, Account, DeviceCode, MsToken};
use crate::paths::write_atomic;
use crate::skin;
use crate::AccountItem;
use slint::{Image, SharedString};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Signin {
    generation: Arc<AtomicU64>,
    code: Option<DeviceCode>,
    watcher: Option<slint::Timer>,
    busy: bool,
}

#[derive(Default)]
pub struct Heads {
    heads: HashMap<String, Image>,
    figures: HashMap<String, Image>,
    loading: Vec<String>,
}

fn skin_pixels(core: &super::Core, account: &Account) -> (Pixels, Pixels) {
    let cache = core.paths.cache().join("skins").join(format!("{}.png", account.uuid));
    let fetched = (!account.skin.is_empty()).then(|| core.http.bytes(&account.skin, 1 << 20).ok()).flatten();
    if let Some(bytes) = &fetched {
        let _ = write_atomic(&cache, bytes);
    }
    let bytes = fetched.or_else(|| std::fs::read(&cache).ok());
    let img = bytes.and_then(|b| skin::decode(&b)).unwrap_or_else(skin::steve);
    (pixels(&skin::head(&img)), pixels(&skin::body(&img, account.slim)))
}

impl App {
    fn generation(&self) -> (Arc<AtomicU64>, u64) {
        let g = self.signin.generation.clone();
        let now = g.load(Ordering::SeqCst);
        (g, now)
    }

    fn signin_mode(&mut self, mode: i32) {
        if let Some(ui) = self.ui() {
            ui.set_signin_mode(mode);
            ui.set_signin_open(true);
        }
        if mode == 2 {
            self.watch_clipboard();
        } else {
            self.signin.watcher = None;
        }
    }

    fn signin_failed(&mut self, error: String) {
        self.signin.busy = false;
        if let Some(ui) = self.ui() {
            ui.set_signin_error(error.into());
        }
        self.signin_mode(4);
    }

    pub fn signin_start(&mut self) {
        let generation = self.signin.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.signin.busy = false;
        self.signin.code = None;
        self.signin_mode(0);
        let core = self.core.clone();
        let current = self.signin.generation.clone();
        background(move || {
            let result = auth::device_code(&core.http, &core.ep);
            later(move |app| {
                if current.load(Ordering::SeqCst) != generation {
                    return;
                }
                match result {
                    Ok(code) => app.show_code(code),
                    Err(_) => app.signin_mode(2),
                }
            });
        });
    }

    fn show_code(&mut self, code: DeviceCode) {
        if let Some(ui) = self.ui() {
            ui.set_signin_code(code.user_code.clone().into());
            ui.set_signin_link(code.verification_uri.trim_start_matches("https://").trim_start_matches("www.").to_string().into());
        }
        self.signin.code = Some(code.clone());
        self.signin_mode(1);
        let (current, generation) = self.generation();
        let core = self.core.clone();
        background(move || {
            let deadline = Instant::now() + Duration::from_secs(code.expires_in);
            let mut wait = code.interval.max(2);
            while Instant::now() < deadline {
                std::thread::sleep(Duration::from_secs(wait));
                if current.load(Ordering::SeqCst) != generation {
                    return;
                }
                match auth::poll(&core.http, &core.ep, &code) {
                    Ok(Some(token)) => return later(move |app| app.finish_signin(token, false)),
                    Ok(None) => {}
                    Err(e) if crate::net::is_offline(&e) => wait = (wait + 2).min(15),
                    Err(e) => return later(move |app| app.signin_failed(e)),
                }
            }
            later(|app| app.signin_failed("The code expired. Start again.".into()));
        });
    }

    fn finish_signin(&mut self, token: MsToken, browser: bool) {
        if self.signin.busy {
            return;
        }
        self.signin.busy = true;
        self.signin_mode(3);
        let (current, generation) = self.generation();
        let core = self.core.clone();
        background(move || {
            let result = auth::minecraft(&core.http, &core.ep, &token, browser);
            later(move |app| {
                if current.load(Ordering::SeqCst) != generation {
                    return;
                }
                match result {
                    Ok(account) => {
                        app.signin.busy = false;
                        let name = account.name.clone();
                        app.store_account(account, true);
                        app.signin_close();
                        app.toast(format!("Signed in as {name}"));
                    }
                    Err(e) => app.signin_failed(e),
                }
            });
        });
    }

    fn signin_with_code(&mut self, code: String) {
        if self.signin.busy {
            return;
        }
        self.signin.busy = true;
        self.signin_mode(3);
        let (current, generation) = self.generation();
        let core = self.core.clone();
        background(move || {
            let result = auth::exchange(&core.http, &core.ep, &code);
            later(move |app| {
                if current.load(Ordering::SeqCst) != generation {
                    return;
                }
                app.signin.busy = false;
                match result {
                    Ok(token) => app.finish_signin(token, true),
                    Err(e) => app.signin_failed(e),
                }
            });
        });
    }

    fn watch_clipboard(&mut self) {
        if self.signin.watcher.is_some() {
            return;
        }
        let mut clipboard = arboard::Clipboard::new().ok();
        let mut last = String::new();
        let timer = slint::Timer::default();
        timer.start(slint::TimerMode::Repeated, Duration::from_millis(700), move || {
            let Some(text) = clipboard.as_mut().and_then(|c| c.get_text().ok()) else { return };
            if text == last {
                return;
            }
            last = text;
            if let Some(code) = auth::code_from_redirect(&last) {
                later(move |app| app.signin_with_code(code));
            }
        });
        self.signin.watcher = Some(timer);
    }

    pub fn signin_paste(&mut self, text: &str) {
        if let Some(code) = auth::code_from_redirect(text) {
            self.signin_with_code(code);
        }
    }

    pub fn signin_browser(&mut self) {
        self.signin.generation.fetch_add(1, Ordering::SeqCst);
        self.signin.busy = false;
        self.signin_mode(2);
    }

    pub fn signin_web(&mut self) {
        crate::sys::open_url(&auth::authorize_url(&self.core.ep));
    }

    pub fn signin_copy(&mut self) {
        let Some(code) = self.signin.code.as_ref().map(|c| c.user_code.clone()) else { return };
        self.copy(code, "Code copied");
    }

    pub fn signin_open(&mut self) {
        let Some(code) = self.signin.code.clone() else { return };
        self.copy(code.user_code, "Code copied, paste it on the page");
        crate::sys::open_url(&code.verification_uri);
    }

    pub fn signin_close(&mut self) {
        self.signin.generation.fetch_add(1, Ordering::SeqCst);
        self.signin.watcher = None;
        self.signin.busy = false;
        if let Some(ui) = self.ui() {
            ui.set_signin_open(false);
        }
    }

    pub fn store_account(&mut self, account: Account, activate: bool) {
        let uuid = account.uuid.clone();
        let skin_changed = self.accounts.get(&uuid).is_none_or(|old| old.skin != account.skin || old.slim != account.slim);
        self.accounts.upsert(account);
        self.accounts.save(&self.core.paths);
        if activate {
            self.settings.account = uuid.clone();
            self.save();
        }
        if skin_changed {
            self.heads.heads.remove(&uuid);
            self.heads.figures.remove(&uuid);
        }
        self.push_accounts();
        self.load_heads();
    }

    pub fn signout(&mut self) {
        let Some(active) = self.active_account() else { return };
        self.accounts.list.retain(|a| a.uuid != active.uuid);
        self.accounts.save(&self.core.paths);
        let _ = std::fs::remove_file(self.core.paths.cache().join("skins").join(format!("{}.png", active.uuid)));
        self.settings.account = self.accounts.list.first().map(|a| a.uuid.clone()).unwrap_or_default();
        self.save();
        self.push_accounts();
        self.toast(format!("Signed out of {}", active.name));
    }

    pub fn switch_account(&mut self, uuid: &str) {
        if self.accounts.get(uuid).is_some() {
            self.settings.account = uuid.to_string();
            self.save();
            self.push_accounts();
        }
    }

    pub fn load_heads(&mut self) {
        let missing: Vec<Account> = self
            .accounts
            .list
            .iter()
            .filter(|a| !self.heads.heads.contains_key(&a.uuid) && !self.heads.loading.contains(&a.uuid))
            .cloned()
            .collect();
        if missing.is_empty() {
            return;
        }
        self.heads.loading.extend(missing.iter().map(|a| a.uuid.clone()));
        let core = self.core.clone();
        background(move || {
            let done: Vec<(String, Pixels, Pixels)> = missing
                .iter()
                .map(|a| {
                    let (head, body) = skin_pixels(&core, a);
                    (a.uuid.clone(), head, body)
                })
                .collect();
            later(move |app| {
                for (uuid, head, body) in done {
                    app.heads.loading.retain(|u| u != &uuid);
                    app.heads.heads.insert(uuid.clone(), Image::from_rgba8(head));
                    app.heads.figures.insert(uuid, Image::from_rgba8(body));
                }
                app.push_accounts();
            });
        });
    }

    pub fn push_accounts(&mut self) {
        let Some(ui) = self.ui() else { return };
        let active = self.active_account();
        let uuid = active.as_ref().map(|a| a.uuid.clone()).unwrap_or_default();
        let items: Vec<AccountItem> = self
            .accounts
            .list
            .iter()
            .map(|a| AccountItem {
                uuid: a.uuid.as_str().into(),
                name: a.name.as_str().into(),
                head: self.heads.heads.get(&a.uuid).cloned().unwrap_or_default(),
                active: a.uuid == uuid,
            })
            .collect();
        self.models.accounts.set_vec(items);
        ui.set_has_account(active.is_some());
        ui.set_account_name(active.as_ref().map(|a| SharedString::from(a.name.as_str())).unwrap_or_default());
        ui.set_account_head(self.heads.heads.get(&uuid).cloned().unwrap_or_default());
        let figure = self.heads.figures.get(&uuid).cloned().unwrap_or_else(|| Image::from_rgba8(pixels(&skin::body(&skin::steve(), false))));
        ui.set_figure(figure);
    }
}
