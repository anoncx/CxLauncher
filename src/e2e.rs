use crate::mc::Endpoints;
use crate::net::Http;
use serde_json::json;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

type Routes = Arc<Mutex<HashMap<String, (u16, Vec<u8>)>>>;

struct Mock {
    base: String,
    routes: Routes,
    bodies: Arc<Mutex<Vec<(String, String)>>>,
}

impl Mock {
    fn start() -> Mock {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes: Routes = Arc::default();
        let bodies: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
        let (r, bs) = (routes.clone(), bodies.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (r, bs) = (r.clone(), bs.clone());
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() {
                        return;
                    }
                    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let mut length = 0usize;
                    loop {
                        let mut header = String::new();
                        if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                            break;
                        }
                        if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                            length = v.trim().parse().unwrap_or(0);
                        }
                    }
                    let mut body = vec![0u8; length];
                    let _ = reader.read_exact(&mut body);
                    bs.lock().unwrap().push((path.clone(), String::from_utf8_lossy(&body).into_owned()));
                    let found = r.lock().unwrap().get(&path).cloned();
                    let mut stream = stream;
                    let (status, body) = found.unwrap_or((404, b"{}".to_vec()));
                    let _ = write!(stream, "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = stream.write_all(&body);
                });
            }
        });
        Mock { base, routes, bodies }
    }

    fn put(&self, path: &str, body: impl Into<Vec<u8>>) {
        self.reply(path, 200, body);
    }

    fn reply(&self, path: &str, status: u16, body: impl Into<Vec<u8>>) {
        self.routes.lock().unwrap().insert(path.to_string(), (status, body.into()));
    }

    fn body(&self, path: &str) -> String {
        self.bodies.lock().unwrap().iter().rev().find(|(p, _)| p == path).map(|(_, b)| b.clone()).unwrap_or_default()
    }

}

#[test]
fn signs_in_through_the_whole_microsoft_chain() {
    use crate::auth;
    let mock = Mock::start();
    let ep = Endpoints::local(&mock.base);
    let http = Http::direct();
    mock.put("/live/oauth20_connect.srf", json!({"user_code": "AB12CD34", "device_code": "dev", "verification_uri": "https://www.microsoft.com/link", "interval": 1, "expires_in": 900}).to_string());
    mock.reply("/live/oauth20_token.srf", 400, json!({"error": "authorization_pending"}).to_string());
    let code = auth::device_code(&http, &ep).unwrap();
    assert_eq!(code.user_code, "AB12CD34");
    assert!(auth::poll(&http, &ep, &code).unwrap().is_none());
    assert!(mock.body("/live/oauth20_token.srf").contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code"));

    mock.put("/live/oauth20_token.srf", json!({"access_token": "ms", "refresh_token": "refresh-1"}).to_string());
    let token = auth::poll(&http, &ep, &code).unwrap().unwrap();
    mock.put("/xbl/user/authenticate", json!({"Token": "xbl", "DisplayClaims": {"xui": [{"uhs": "4242"}]}}).to_string());
    mock.put("/xsts/xsts/authorize", json!({"Token": "xsts", "DisplayClaims": {"xui": [{"uhs": "4242"}]}}).to_string());
    mock.put("/services/authentication/login_with_xbox", json!({"access_token": "mc-token", "expires_in": 86400}).to_string());
    mock.put(
        "/services/minecraft/profile",
        json!({"id": "069a79f444e94726a5befca90e38aaf5", "name": "Tester", "skins": [{"state": "ACTIVE", "url": "http://textures.minecraft.net/texture/abc", "variant": "SLIM"}]}).to_string(),
    );
    let account = auth::minecraft(&http, &ep, &token, false).unwrap();
    assert_eq!(account.name, "Tester");
    assert_eq!(account.uuid, "069a79f4-44e9-4726-a5be-fca90e38aaf5");
    assert_eq!(account.token, "mc-token");
    assert_eq!(account.refresh, "refresh-1");
    assert!(account.slim);
    assert_eq!(account.skin, "https://textures.minecraft.net/texture/abc");
    assert!(auth::fresh(&account));
    assert!(mock.body("/xbl/user/authenticate").contains("\"RpsTicket\":\"t=ms\""));
    assert!(mock.body("/services/authentication/login_with_xbox").contains("XBL3.0 x=4242;xsts"));

    mock.put("/live/oauth20_token.srf", json!({"access_token": "ms2", "refresh_token": ""}).to_string());
    let renewed = auth::renew(&http, &ep, &account).unwrap();
    assert_eq!(renewed.refresh, "refresh-1");
    assert!(mock.body("/live/oauth20_token.srf").contains("grant_type=refresh_token"));

    mock.reply("/xsts/xsts/authorize", 401, json!({"XErr": 2148916238u64}).to_string());
    let child = auth::minecraft(&http, &ep, &token, false).unwrap_err();
    assert!(child.contains("child account"), "{child}");

    mock.reply("/live/oauth20_token.srf", 400, json!({"error": "invalid_grant"}).to_string());
    assert!(auth::renew(&http, &ep, &account).unwrap_err().contains("Sign in again"));
}

#[cfg(unix)]
mod launch {
    use super::Mock;
    use crate::download::{hex, Progress};
    use crate::game::{self, Game, Options, Session};
    use crate::install::{self, Request, Stage};
    use crate::mc::modmeta::write_jar;
    use crate::mc::Endpoints;
    use crate::net::Http;
    use crate::paths::Paths;
    use crate::store::Profile;
    use serde_json::json;
    use sha1::{Digest, Sha1};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    impl Mock {
        fn hits(&self, prefix: &str) -> usize {
            self.bodies.lock().unwrap().iter().filter(|(p, _)| p.starts_with(prefix)).count()
        }
    }

    fn sha1(data: &[u8]) -> String {
        let mut h = Sha1::new();
        h.update(data);
        hex(&h.finalize())
    }

    fn jar(dir: &Path, name: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
        let path = dir.join(name);
        write_jar(&path, files);
        std::fs::read(path).unwrap()
    }

    fn which_java() -> Option<PathBuf> {
        let out = std::process::Command::new("sh").args(["-c", "command -v java && command -v javac"]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let java = text.lines().next()?.trim().to_string();
        (out.status.success() && text.lines().count() == 2).then(|| std::fs::canonicalize(java).ok()).flatten()
    }

    fn compile_fake_game(work: &Path) -> Vec<u8> {
        let src = work.join("src/cx");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("FakeKnot.java"),
            r#"package cx;
    public class FakeKnot {
        public static void main(String[] a) {
            System.out.println("[00:00:00] [main/INFO]: FakeKnot started");
            System.out.println("[00:00:00] [main/INFO]: addMods=" + System.getProperty("fabric.addMods"));
            System.out.println("[00:00:00] [main/INFO]: args=" + String.join(" ", a));
            System.out.println("[00:00:00] [main/INFO]: cp=" + System.getProperty("java.class.path"));
            System.out.println("[00:00:00] [main/INFO]: emu=" + System.getProperty("FabricMcEmu"));
            System.err.println("[00:00:00] [main/WARN]: a warning");
            System.exit(0);
        }
    }
    "#,
        )
        .unwrap();
        let classes = work.join("classes");
        let status = std::process::Command::new("javac")
            .args(["--release", "17", "-d"])
            .arg(&classes)
            .arg(src.join("FakeKnot.java"))
            .status()
            .unwrap();
        assert!(status.success());
        let class = std::fs::read(classes.join("cx/FakeKnot.class")).unwrap();
        jar(work, "loader.jar", &[("cx/FakeKnot.class", &class), ("fabric.mod.json", br#"{"id": "fabricloader", "version": "0.19.3"}"#)])
    }

    fn serve_world(m: &Mock, work: &Path, java: &Path) {
        let b = &m.base;
        let lib = b"library bytes".to_vec();
        let client = jar(work, "client.jar", &[("version.json", b"{}")]);
        let loader = compile_fake_game(work);
        let objects: Vec<&[u8]> = vec![b"sound one", b"texture two", b"sound one"];
        let index = json!({"objects": {
            "minecraft/sounds/a.ogg": {"hash": sha1(objects[0]), "size": objects[0].len()},
            "minecraft/textures/b.png": {"hash": sha1(objects[1]), "size": objects[1].len()},
            "minecraft/sounds/copy.ogg": {"hash": sha1(objects[2]), "size": objects[2].len()}
        }})
        .to_string();
        for o in &objects {
            let h = sha1(o);
            m.put(&format!("/resources/{}/{h}", &h[..2]), o.to_vec());
        }
        m.put("/mojang/30.json", index.clone());
        let version = json!({
            "id": "26.2",
            "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "javaVersion": {"component": "java-runtime-test", "majorVersion": 17},
            "assetIndex": {"id": "30", "sha1": sha1(index.as_bytes()), "size": index.len(), "url": format!("{b}/mojang/30.json")},
            "assets": "30",
            "downloads": {"client": {"sha1": sha1(&client), "size": client.len(), "url": format!("{b}/mojang/client.jar")}},
            "arguments": {
                "game": ["--username", "${auth_player_name}", "--version", "${version_name}", "--gameDir", "${game_directory}",
                    "--assetsDir", "${assets_root}", "--assetIndex", "${assets_index_name}", "--uuid", "${auth_uuid}",
                    "--accessToken", "${auth_access_token}", "--userType", "${user_type}", "--versionType", "${version_type}",
                    {"rules": [{"action": "allow", "features": {"is_demo_user": true}}], "value": "--demo"},
                    {"rules": [{"action": "allow", "features": {"has_custom_resolution": true}}], "value": ["--width", "${resolution_width}", "--height", "${resolution_height}"]}],
                "jvm": ["-Djava.library.path=${natives_directory}", "-cp", "${classpath}"]
            },
            "libraries": [
                {"name": "org.example:lib:1.0", "downloads": {"artifact": {"path": "org/example/lib/1.0/lib-1.0.jar", "sha1": sha1(&lib), "size": lib.len(), "url": format!("{b}/libs/lib-1.0.jar")}}},
                {"name": "org.example:native:1.0:natives-windows", "rules": [{"action": "allow", "os": {"name": "windows"}}],
                 "downloads": {"artifact": {"path": "org/example/native/1.0/native-1.0-natives-windows.jar", "url": format!("{b}/libs/never.jar")}}}
            ]
        })
        .to_string();
        m.put("/mojang/26.2.json", version.clone());
        m.put("/mojang/version_manifest_v2.json", json!({"latest": {"release": "26.2", "snapshot": "26.2"}, "versions": [{"id": "26.2", "type": "release", "url": format!("{b}/mojang/26.2.json"), "sha1": sha1(version.as_bytes())}]}).to_string());
        m.put("/mojang/client.jar", client);
        m.put("/libs/lib-1.0.jar", lib);

        m.put("/fabric/v2/versions/loader/26.2", json!([{"loader": {"version": "0.19.4", "stable": false}}, {"loader": {"version": "0.19.3", "stable": true}}]).to_string());
        m.put(
            "/fabric/v2/versions/loader/26.2/0.19.3/profile/json",
            json!({
                "id": "fabric-loader-0.19.3-26.2",
                "inheritsFrom": "26.2",
                "mainClass": "cx.FakeKnot",
                "arguments": {"game": [], "jvm": ["-DFabricMcEmu= net.minecraft.client.main.Main "]},
                "libraries": [{"name": "net.fabricmc:fabric-loader:0.19.3", "url": format!("{b}/maven/"), "sha1": sha1(&loader), "size": loader.len()}]
            })
            .to_string(),
        );
        m.put("/maven/net/fabricmc/fabric-loader/0.19.3/fabric-loader-0.19.3.jar", loader);

        let launcher = format!("#!/bin/sh\nexec \"{}\" \"$@\"\n", java.display());
        let release = b"JAVA_VERSION=\"17.0.9\"\n".to_vec();
        m.put(
            "/runtime/manifest.json",
            json!({"files": {
                "bin": {"type": "directory"},
                "bin/java": {"type": "file", "executable": true, "downloads": {"raw": {"sha1": sha1(launcher.as_bytes()), "size": launcher.len(), "url": format!("{b}/runtime/java")}}},
                "release": {"type": "file", "downloads": {"raw": {"sha1": sha1(&release), "size": release.len(), "url": format!("{b}/runtime/release")}}},
                "bin/javalink": {"type": "link", "target": "java"},
                "../escape": {"type": "file", "downloads": {"raw": {"url": format!("{b}/runtime/never")}}}
            }})
            .to_string(),
        );
        m.put("/runtime/java", launcher);
        m.put("/runtime/release", release);
        m.put(
            "/runtime/all.json",
            json!({crate::mc::java::platform().unwrap(): {"java-runtime-test": [{"manifest": {"url": format!("{b}/runtime/manifest.json")}, "version": {"name": "17.0.9"}}]}}).to_string(),
        );

        let visuals = jar(work, "cxvisuals.jar", &[("fabric.mod.json", br#"{"id": "cxvisuals", "version": "0.1.0+26.2", "depends": {"minecraft": "~26.2"}}"#)]);
        m.put(
            "/client?mc=26.2",
            json!({"version": "0.1.0+26.2", "minecraft": "26.2", "loader": "0.19.0", "file": "cxvisuals-0.1.0+26.2.jar", "sha1": sha1(&visuals), "size": visuals.len(), "notes": ["First release"]}).to_string(),
        );
        m.put("/client/cxvisuals-0.1.0+26.2.jar", visuals);

        let api = jar(work, "api.jar", &[("fabric.mod.json", br#"{"id": "fabric-api", "version": "0.154.2+26.2"}"#)]);
        m.put(
            "/modrinth/v2/project/fabric-api/version?loaders=%5B%22fabric%22%5D&game_versions=%5B%2226.2%22%5D",
            json!([{"id": "v1", "project_id": "P7dR8mSH", "version_number": "0.154.2+26.2", "version_type": "release",
                "files": [{"url": format!("{b}/cdn/fabric-api-0.154.2+26.2.jar"), "filename": "fabric-api-0.154.2+26.2.jar", "primary": true, "size": api.len(), "hashes": {"sha1": sha1(&api)}}]}])
            .to_string(),
        );
        m.put("/cdn/fabric-api-0.154.2+26.2.jar", api);
    }

    #[test]
    fn installs_and_launches_against_a_mock_world() {
        let Some(java) = which_java() else {
            eprintln!("skipped: no java and javac on PATH");
            return;
        };
        let work = std::env::temp_dir().join(format!("cxl-e2e-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&work).unwrap();
        let mock = Mock::start();
        serve_world(&mock, &work, &java);

        let paths = Paths::new(work.join("home"));
        let ep = Endpoints::local(&mock.base);
        let http = Http::direct();
        let profile = Profile { id: "cxvisuals".into(), name: "CxVisuals".into(), game: "26.2".into(), client: true, ..Default::default() };
        let stages = Mutex::new(Vec::new());
        let progress = Progress::default();
        let request = Request { http: &http, ep: &ep, paths: &paths, profile: &profile, local_client: None, java: None, threads: 6, verify: false };

        let prepared = install::prepare(&request, &progress, &|s| stages.lock().unwrap().push(s)).unwrap();
        assert_eq!(*stages.lock().unwrap(), [Stage::Checking, Stage::Downloading, Stage::Starting]);
        assert_eq!(prepared.version.id, "fabric-loader-0.19.3-26.2");
        assert_eq!(progress.done_files.load(std::sync::atomic::Ordering::Relaxed), 9);
        assert!((progress.fraction() - 1.0).abs() < 1e-6);
        assert_eq!(mock.hits("/libs/never"), 0);
        assert_eq!(mock.hits("/runtime/never"), 0);
        assert!(!paths.root.join("runtime/escape").exists());
        assert!(paths.runtime("java-runtime-test").join("bin/javalink").symlink_metadata().is_ok());
        assert_eq!(std::fs::read_dir(paths.assets().join("objects")).unwrap().count(), 2);
        assert_eq!(prepared.mods.len(), 2);

        let session = Session { name: "Tester".into(), uuid: "069a79f4-44e9-4726-a5be-fca90e38aaf5".into(), token: "secret".into(), xuid: String::new(), offline: false };
        let options = Options { memory: 1024, width: 1280, height: 720, ..Default::default() };
        let args = game::arguments(&prepared, &session, &options);
        let mut running = Game::spawn(&prepared.java, &args, &prepared.game_dir, &game::Extras::default()).unwrap();
        let started = Instant::now();
        let code = loop {
            if let Some(code) = running.exited() {
                break code;
            }
            assert!(started.elapsed() < Duration::from_secs(30), "game did not exit");
            std::thread::sleep(Duration::from_millis(50));
        };
        std::thread::sleep(Duration::from_millis(200));
        let lines: Vec<String> = running.drain().into_iter().map(|(_, l)| l).collect();
        let text = lines.join("\n");
        assert_eq!(code, 0, "{text}");
        assert!(text.contains("FakeKnot started"), "{text}");
        assert!(text.contains("--username Tester --version fabric-loader-0.19.3-26.2"), "{text}");
        assert!(text.contains("--accessToken secret"), "{text}");
        assert!(text.contains("--uuid 069a79f444e94726a5befca90e38aaf5"), "{text}");
        assert!(text.contains("--width 1280 --height 720"), "{text}");
        assert!(!text.contains("--demo"), "{text}");
        assert!(text.contains("emu= net.minecraft.client.main.Main"), "{text}");
        assert!(text.contains("cxvisuals-0.1.0+26.2.jar") && text.contains("fabric-api-0.154.2+26.2.jar"), "{text}");
        assert!(text.contains("lib-1.0.jar") && text.contains("26.2.jar"), "{text}");
        assert!(text.contains("a warning"), "{text}");

        progress.reset();
        let again = install::prepare(&request, &progress, &|_| {}).unwrap();
        assert_eq!(progress.total_files.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert_eq!(again.mods.len(), 2);

        let lib = paths.libraries().join("org/example/lib/1.0/lib-1.0.jar");
        std::fs::write(&lib, b"LIBRARY BYTES").unwrap();
        let mods = install::mods_dir(&paths, "cxvisuals");
        std::fs::copy(paths.managed("fabric-api").join("fabric-api-0.154.2+26.2.jar"), mods.join("my-fabric-api.jar")).unwrap();
        progress.reset();
        let verify = Request { verify: true, local_client: None, java: None, ..request };
        let repaired = install::prepare(&verify, &progress, &|_| {}).unwrap();
        assert_eq!(progress.total_files.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(std::fs::read(&lib).unwrap(), b"library bytes");
        assert_eq!(repaired.mods.len(), 1);

        let vanilla = Profile { id: "plain".into(), name: "Plain".into(), vanilla: true, ..Default::default() };
        let plain = install::prepare(&Request { profile: &vanilla, local_client: None, java: None, ..request }, &progress, &|_| {}).unwrap();
        assert_eq!(plain.version.id, "26.2");
        assert_eq!(plain.version.main_class.as_deref(), Some("net.minecraft.client.main.Main"));
        assert!(plain.mods.is_empty() && plain.client.is_none());
        assert!(!game::arguments(&plain, &session, &options).iter().any(|a| a.starts_with("-Dfabric.addMods")));

        let wrong = Profile { id: "old".into(), game: "1.21.4".into(), client: true, ..Default::default() };
        let err = install::prepare(&Request { profile: &wrong, local_client: None, java: None, ..request }, &progress, &|_| {}).unwrap_err();
        assert!(err.contains("cannot get CxVisuals for Minecraft 1.21.4"), "{err}");

        let pack_file = work.join("pack.mrpack");
        let api_bytes = std::fs::read(mods.join("my-fabric-api.jar")).unwrap();
        let index = json!({"formatVersion": 1, "game": "minecraft", "versionId": "1.0", "name": "Mock Pack",
            "files": [{"path": "mods/fabric-api.jar", "hashes": {"sha1": sha1(&api_bytes)}, "downloads": [format!("{}/cdn/fabric-api-0.154.2+26.2.jar", mock.base)], "fileSize": api_bytes.len()}],
            "dependencies": {"minecraft": "26.2", "fabric-loader": "0.19.3"}}).to_string();
        write_jar(&pack_file, &[("modrinth.index.json", index.as_bytes()), ("overrides/options.txt", b"fov:90")]);
        let pack_dir = paths.profile("pack");
        let pack = crate::mc::mrpack::install(&http, &pack_file, &pack_dir, 4, &Progress::default()).unwrap();
        assert_eq!(pack.game, "26.2");
        assert_eq!(std::fs::read(pack_dir.join("mods/fabric-api.jar")).unwrap(), api_bytes);
        assert_eq!(std::fs::read(pack_dir.join("options.txt")).unwrap(), b"fov:90");

        mock.routes.lock().unwrap().clear();
        progress.reset();
        let offline = install::prepare(&request, &progress, &|_| {}).unwrap();
        assert_eq!(offline.version.id, "fabric-loader-0.19.3-26.2");

        let _ = std::fs::remove_dir_all(work);
    }
}
