mod accounts;
mod console;
mod finder;
mod images;
mod import;
mod instance;
mod markdown;
mod mods;
mod play;
mod profiles;
mod settings;
mod updater;

use crate::auth::Accounts;
use crate::mc::Endpoints;
use crate::net::Http;
use crate::paths::Paths;
use crate::store::{Profile, Profiles, Settings};
use crate::{AppWindow, Theme};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel, Weak};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

#[derive(Clone)]
pub struct Core {
    pub http: Http,
    pub ep: Endpoints,
    pub paths: Paths,
}

#[derive(Default)]
struct Models {
    accounts: Rc<VecModel<crate::AccountItem>>,
    profiles: Rc<VecModel<crate::ProfileItem>>,
    installed: Rc<VecModel<crate::ModItem>>,
    results: Rc<VecModel<crate::FindItem>>,
    packs: Rc<VecModel<crate::FindItem>>,
    versions: Rc<VecModel<crate::VersionItem>>,
    log: Rc<VecModel<crate::LogLine>>,
    javas: Rc<VecModel<crate::Command>>,
    notes: Rc<VecModel<SharedString>>,
    links: Rc<VecModel<crate::ModLink>>,
    gallery: Rc<VecModel<crate::ModImage>>,
    body: Rc<VecModel<crate::BodyBlock>>,
    worlds: Rc<VecModel<crate::WorldItem>>,
    shots: Rc<VecModel<crate::ShotItem>>,
    categories: Rc<VecModel<crate::Command>>,
    sorts: Rc<VecModel<crate::Command>>,
    imports: Rc<VecModel<crate::ImportItem>>,
}

pub struct App {
    pub ui: Weak<AppWindow>,
    pub core: Core,
    pub settings: Settings,
    pub profiles: Profiles,
    pub accounts: Accounts,
    models: Models,
    toast_serial: i32,
    run: play::Run,
    console: console::Console,
    signin: accounts::Signin,
    heads: accounts::Heads,
    mods: mods::Mods,
    packs: finder::Finder,
    javas: Vec<crate::mc::java::Installed>,
    info: play::Info,
    version_filter: profiles::VersionFilter,
    import: import::Import,
    update: Option<crate::update::Release>,
    update_busy: bool,
    ticker: slint::Timer,
    timers: Vec<slint::Timer>,
}

thread_local! {
    static APP: RefCell<std::rc::Weak<RefCell<App>>> = const { RefCell::new(std::rc::Weak::new()) };
}

pub fn later(f: impl FnOnce(&mut App) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = APP.with(|a| a.borrow().upgrade()) {
            with(&app, f);
        }
    });
}

pub fn with(shared: &Rc<RefCell<App>>, f: impl FnOnce(&mut App) + 'static) {
    match shared.try_borrow_mut() {
        Ok(mut app) => f(&mut app),
        Err(_) => {
            let shared = Rc::downgrade(shared);
            slint::Timer::single_shot(Duration::ZERO, move || {
                if let Some(app) = shared.upgrade() {
                    with(&app, f);
                }
            });
        }
    }
}

pub fn background(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(f);
}

impl App {
    pub fn ui(&self) -> Option<AppWindow> {
        self.ui.upgrade()
    }

    pub fn toast(&mut self, text: impl Into<SharedString>) {
        self.toast_serial += 1;
        if let Some(ui) = self.ui() {
            ui.set_toast(text.into());
            ui.set_toast_serial(self.toast_serial);
        }
    }

    pub fn copy(&mut self, text: String, done: &str) {
        match arboard::Clipboard::new().and_then(|mut c| c.set_text(text)) {
            Ok(()) => self.toast(done.to_string()),
            Err(_) => self.toast("Could not reach the clipboard"),
        }
    }

    pub fn profile(&self) -> Profile {
        self.profiles.get(&self.settings.profile).or(self.profiles.list.first()).cloned().unwrap_or_default()
    }

    pub fn save(&self) {
        self.settings.save(&self.core.paths);
    }

    fn page(&self) -> i32 {
        self.ui().map(|u| u.get_page()).unwrap_or(0)
    }

    fn command(&mut self, id: &str) {
        let (verb, arg) = id.split_once(':').unwrap_or((id, ""));
        match verb {
            "page" => self.on_page(arg.parse().unwrap_or(0)),
            "play" => self.play(false),
            "repair" => self.play(true),
            "cancel" => self.cancel(),
            "stop" => self.stop(),
            "console" => self.go(3),
            "copy-log" => self.copy_log(),
            "open-logs" => crate::sys::reveal(&self.core.paths.profile(&self.profile().id).join("logs")),
            "install-update" => self.install_update(),
            "open-data" => crate::sys::reveal(&self.core.paths.root),
            "open-game-folder" => self.open_profile_folder(&self.profile().id),
            "open-profile-folder" => self.open_profile_folder(arg),
            "open-mods-folder" => self.open_mods_folder(),
            "signin" => self.signin_start(),
            "signin-close" => self.signin_close(),
            "offline-open" => self.offline_open(),
            "offline-add" => self.offline_add(arg),
            "signin-copy" => self.signin_copy(),
            "signin-open" => self.signin_open(),
            "signin-browser" => self.signin_browser(),
            "signin-web" => self.signin_web(),
            "signin-device" => self.signin_start(),
            "signout" => self.signout(),
            "account" => self.switch_account(arg),
            "profile" => self.switch_profile(arg),
            "new-profile" => self.edit_profile(None),
            "edit-profile" => self.edit_profile(Some(arg)),
            "play-profile" => self.play_profile(arg),
            "import-pack" => self.import_pack(),
            "import-open" => self.import_open(),
            "import-toggle" => self.import_toggle(arg),
            "import-all" => self.import_set_all(true),
            "import-none" => self.import_set_all(false),
            "import-run" => self.import_run(),
            "import-close" => self.import_close(),
            "use-fabric" => self.use_fabric(),
            "delete-profile" => self.delete_profile(arg),
            "boost" => self.boost(),
            "pick-jar" => self.pick_jar(),
            "install-mod" => self.detail_install(arg),
            "filter-mods" => {
                let (cat, sort) = arg.split_once('|').unwrap_or((arg, ""));
                self.set_filter(finder::Kind::Mod, cat, sort);
            }
            "filter-packs" => {
                let (cat, sort) = arg.split_once('|').unwrap_or((arg, ""));
                self.set_filter(finder::Kind::Pack, cat, sort);
            }
            "content" => self.set_content(arg.parse().unwrap_or(0)),
            "update-mods" => self.update_mods(),
            "update-pack" => self.update_pack(arg),
            "open-mod" => self.open_detail(finder::Kind::Mod, arg),
            "open-pack" => self.open_detail(finder::Kind::Pack, arg),
            "open-link" => crate::sys::open_url(arg),
            "filter-versions" => self.filter_versions(arg),
            "detail-close" => self.close_detail(),
            "duplicate-profile" => self.duplicate_profile(arg),
            "open-instance" => self.open_instance(arg),
            "open-world" => self.open_world(arg),
            "open-shot" => self.open_shots(arg),
            "open-shots" => self.open_shots(""),
            "backup-profile" => self.backup_profile(arg),
            "export-profile" => self.export_profile(arg),
            _ => {}
        }
    }

    fn go(&mut self, page: i32) {
        if let Some(ui) = self.ui() {
            ui.set_page(page);
        }
        self.on_page(page);
    }

    fn on_page(&mut self, page: i32) {
        match page {
            1 if self.models.packs.row_count() == 0 => self.find(finder::Kind::Pack, String::new(), true),
            2 => self.refresh_mods(),
            3 => self.push_log(),
            4 => self.detect_javas(),
            _ => {}
        }
    }

    fn startup(&mut self) {
        self.push_settings();
        self.push_profiles();
        self.push_accounts();
        self.load_heads();
        self.load_client_info();
        self.poll_online();
        self.push_play();
        self.push_log();
        self.import_check();
        self.check_update();
        let weak = self.ui.clone();
        let timer = slint::Timer::default();
        timer.start(slint::TimerMode::Repeated, Duration::from_secs(90), move || {
            if weak.upgrade().is_some() {
                later(|app| app.poll_online());
            }
        });
        self.timers.push(timer);
    }
}

pub fn run() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let paths = Paths::new(Paths::default_root());
    let _ = std::fs::create_dir_all(&paths.root);
    let settings = Settings::load(&paths);
    let profiles = Profiles::load(&paths);
    let accounts = Accounts::load(&paths);
    let models = Models::default();
    models.categories.set_vec(vec![
        crate::Command { id: "all".into(), title: "All categories".into(), detail: "".into() },
        crate::Command { id: "adventure".into(), title: "Adventure".into(), detail: "".into() },
        crate::Command { id: "optimization".into(), title: "Optimization".into(), detail: "".into() },
        crate::Command { id: "technology".into(), title: "Technology".into(), detail: "".into() },
        crate::Command { id: "magic".into(), title: "Magic".into(), detail: "".into() },
        crate::Command { id: "library".into(), title: "Library".into(), detail: "".into() },
    ]);
    models.sorts.set_vec(vec![
        crate::Command { id: "relevance".into(), title: "Relevance".into(), detail: "".into() },
        crate::Command { id: "downloads".into(), title: "Downloads".into(), detail: "".into() },
        crate::Command { id: "follows".into(), title: "Followers".into(), detail: "".into() },
        crate::Command { id: "newest".into(), title: "Newest".into(), detail: "".into() },
        crate::Command { id: "updated".into(), title: "Recently updated".into(), detail: "".into() },
    ]);
    ui.set_accounts(ModelRc::from(models.accounts.clone()));
    ui.set_profiles(ModelRc::from(models.profiles.clone()));
    ui.set_installed(ModelRc::from(models.installed.clone()));
    ui.set_results(ModelRc::from(models.results.clone()));
    ui.set_packs(ModelRc::from(models.packs.clone()));
    ui.set_versions(ModelRc::from(models.versions.clone()));
    ui.set_profile_javas(ModelRc::from(models.javas.clone()));
    ui.set_log_window(ModelRc::from(models.log.clone()));
    ui.set_javas(ModelRc::from(models.javas.clone()));
    ui.set_notes(ModelRc::from(models.notes.clone()));
    ui.set_links(ModelRc::from(models.links.clone()));
    ui.set_gallery(ModelRc::from(models.gallery.clone()));
    ui.set_body(ModelRc::from(models.body.clone()));
    ui.set_mod_categories(ModelRc::from(models.categories.clone()));
    ui.set_sorts(ModelRc::from(models.sorts.clone()));
    ui.set_worlds(ModelRc::from(models.worlds.clone()));
    ui.set_shots(ModelRc::from(models.shots.clone()));
    ui.set_imports(ModelRc::from(models.imports.clone()));
    ui.set_tile(images::dot_tile(16));
    ui.global::<Theme>().set_mode(settings.theme);
    ui.global::<Theme>().set_accent_index(settings.accent.clamp(0, 6));
    ui.global::<Theme>().set_effects(settings.effects);
    ui.global::<Theme>().set_calm(settings.reduce_motion);

    let shared = Rc::new(RefCell::new(App {
        ui: ui.as_weak(),
        core: Core { http: Http::new(), ep: Endpoints::configured(), paths },
        settings,
        profiles,
        accounts,
        models,
        toast_serial: 0,
        run: play::Run::Idle,
        console: console::Console::default(),
        signin: accounts::Signin::default(),
        heads: accounts::Heads::default(),
        mods: mods::Mods::default(),
        packs: finder::Finder::default(),
        javas: Vec::new(),
        info: play::Info { online: -1, ..Default::default() },
        version_filter: Default::default(),
        import: Default::default(),
        update: None,
        update_busy: false,
        ticker: slint::Timer::default(),
        timers: Vec::new(),
    }));
    APP.with(|a| *a.borrow_mut() = Rc::downgrade(&shared));
    wire(&ui, &shared);
    shared.borrow_mut().startup();
    ui.run()?;
    let mut app = shared.borrow_mut();
    app.shutdown();
    Ok(())
}

fn wire(ui: &AppWindow, shared: &Rc<RefCell<App>>) {
    let s = shared.clone();
    ui.on_command(move |id| {
        let id = id.to_string();
        with(&s, move |app| app.command(&id))
    });

    let s = shared.clone();
    ui.on_setting(move |k, v| {
        let (k, v) = (k.to_string(), v.to_string());
        with(&s, move |app| app.setting(&k, &v))
    });

    let s = shared.clone();
    ui.on_mod_toggle(move |file, on| {
        let file = file.to_string();
        with(&s, move |app| app.toggle_mod(&file, on))
    });

    let s = shared.clone();
    ui.on_mod_remove(move |file| {
        let file = file.to_string();
        with(&s, move |app| app.remove_mod(&file))
    });

    let s = shared.clone();
    ui.on_find(move |q, now| {
        let q = q.to_string();
        with(&s, move |app| app.find(finder::Kind::Mod, q, now))
    });

    let s = shared.clone();
    ui.on_find_packs(move |q, now| {
        let q = q.to_string();
        with(&s, move |app| app.find(finder::Kind::Pack, q, now))
    });

    let s = shared.clone();
    ui.on_install_pack(move |id| {
        let id = id.to_string();
        with(&s, move |app| app.install_pack(&id))
    });

    let s = shared.clone();
    ui.on_install(move |id| {
        let id = id.to_string();
        with(&s, move |app| app.install(&id))
    });

    let s = shared.clone();
    ui.on_log_view(move |first, rows| with(&s, move |app| app.view_log(first.max(0) as usize, rows.max(1) as usize)));

    let s = shared.clone();
    ui.on_log_filtered(move |f| with(&s, move |app| app.filter_log(f)));

    let s = shared.clone();
    ui.on_signin_paste(move |text| {
        let text = text.to_string();
        with(&s, move |app| app.signin_paste(&text))
    });

    let s = shared.clone();
    ui.on_profile_save(move |p| with(&s, move |app| app.save_profile(p)));

    let s = shared.clone();
    ui.window().on_close_requested(move || {
        with(&s, |app| app.save());
        slint::CloseRequestResponse::HideWindow
    });
}

pub fn first_letter(name: &str) -> SharedString {
    name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into()).into()
}

pub fn row_index<T: Clone + 'static>(model: &VecModel<T>, pred: impl Fn(&T) -> bool) -> Option<usize> {
    (0..model.row_count()).find(|&i| model.row_data(i).is_some_and(|r| pred(&r)))
}
