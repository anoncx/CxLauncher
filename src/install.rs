use crate::client::{self, Client};
use crate::download::{self, Progress};
use crate::mc::meta::{self, Version};
use crate::mc::rules::Env;
use crate::mc::{assets, fabric, java, libraries, modmeta, modrinth, Endpoints};
use crate::net::Http;
use crate::paths::Paths;
use crate::store::Profile;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Checking,
    Downloading,
    Starting,
}

pub struct Request<'a> {
    pub http: &'a Http,
    pub ep: &'a Endpoints,
    pub paths: &'a Paths,
    pub profile: &'a Profile,
    pub local_client: Option<PathBuf>,
    pub java: Option<PathBuf>,
    pub threads: usize,
    pub verify: bool,
}

#[derive(Debug)]
pub struct Prepared {
    pub version: Version,
    pub game: String,
    pub java: PathBuf,
    pub java_major: u32,
    pub classpath: Vec<PathBuf>,
    pub natives: PathBuf,
    pub mods: Vec<PathBuf>,
    pub game_dir: PathBuf,
    pub libraries: PathBuf,
    pub assets: PathBuf,
    pub game_assets: PathBuf,
    pub asset_index: String,
    pub client: Option<Client>,
}

pub fn mods_dir(paths: &Paths, profile: &str) -> PathBuf {
    paths.profile(profile).join("mods")
}

#[allow(dead_code)]
pub fn content_dir(paths: &Paths, profile: &str, content: &str) -> PathBuf {
    match content {
        "resourcepacks" | "shaderpacks" => paths.profile(profile).join(content),
        _ => mods_dir(paths, profile),
    }
}

pub fn jars(dir: &Path) -> Vec<PathBuf> {
    files_ext(dir, "jar")
}

pub fn files_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == ext)).collect())
        .unwrap_or_default();
    out.sort();
    out
}

fn installed_ids(dir: &Path) -> HashSet<String> {
    jars(dir).iter().filter_map(|j| modmeta::read(j, false)).map(|m| m.id).collect()
}

fn prune(dir: &Path, keep: &[PathBuf]) {
    for jar in jars(dir) {
        if !keep.contains(&jar) {
            let _ = std::fs::remove_file(jar);
        }
    }
}

pub fn choose_game(profile: &Profile, client: Option<&Client>, latest: &str) -> String {
    let wanted = profile.game.trim();
    if !wanted.is_empty() {
        return wanted.to_string();
    }
    match client {
        Some(c) => c.minecraft.clone(),
        None if !latest.is_empty() => latest.to_string(),
        None => client::GAME.into(),
    }
}

pub fn prepare(req: &Request, progress: &Progress, stage: &dyn Fn(Stage)) -> Result<Prepared, String> {
    let Request { http, ep, paths, profile, .. } = *req;
    stage(Stage::Checking);
    progress.reset();

    let client = match (profile.uses_client(), &req.local_client) {
        (false, _) => None,
        (true, Some(jar)) => Some(client::from_jar(jar.clone())?),
        (true, None) => Some(client::from_cloud(http, ep, paths)?),
    };
    let manifest = meta::manifest(http, ep, paths).ok();
    let latest = manifest.as_ref().map(|m| m.latest.release.clone()).unwrap_or_default();
    let game = choose_game(profile, client.as_ref(), &latest);
    if let Some(c) = client.as_ref().filter(|c| c.minecraft != game) {
        return Err(format!("{} is made for Minecraft {}, this profile uses {game}. Switch the version or turn {} off.", c.name, c.minecraft, c.name));
    }

    let base = meta::version(http, paths, manifest.as_ref(), &game)?;
    let version = if profile.vanilla {
        base
    } else {
        let loader = match profile.loader.trim() {
            "" => fabric::latest_loader(http, ep, paths, &game, client.as_ref().and_then(|c| c.loader.as_deref()))?,
            pinned => pinned.to_string(),
        };
        fabric::profile(http, ep, paths, &game, &loader)?.over(base)
    };

    let env = Env::current();
    let libs = libraries::resolve(&version.active_libraries(&env), &env, &paths.libraries())?;
    let mut jobs = libs.jobs;
    let mut classpath = libs.classpath;

    let jar = paths.version_jar(&game);
    let artifact = version.downloads.as_ref().and_then(|d| d.client.as_ref()).ok_or("the version file has no game download")?;
    jobs.push(download::Job::new(&artifact.url, jar.clone()).sha1(artifact.sha1.as_deref()).size(artifact.size));
    classpath.push(jar);

    let game_dir = paths.profile(&profile.id);
    let index = version.asset_index.as_ref().ok_or("the version file has no asset index")?;
    let assets = assets::plan(http, index, &paths.assets(), &ep.resources, &game_dir)?;
    jobs.extend(assets.jobs);

    let needed = version.java_major();
    let (java, java_major, runtime) = match &req.java {
        Some(custom) => {
            let found = java::probe(custom).ok_or(format!("No Java found at {}", custom.display()))?;
            if found.major < needed {
                return Err(format!("Minecraft {game} needs Java {needed}, the Java picked in Settings is {}", found.major));
            }
            (custom.clone(), found.major, None)
        }
        None => {
            let component = version.java_version.as_ref().and_then(|j| j.component.clone()).ok_or("the version file names no Java runtime")?;
            let rt = java::plan(http, ep, paths, &component)?;
            jobs.extend(rt.jobs.iter().cloned());
            (rt.java.clone(), needed, Some(rt))
        }
    };

    let mods_dir = mods_dir(paths, &profile.id);
    std::fs::create_dir_all(&mods_dir).map_err(|e| format!("cannot create {}: {e}", mods_dir.display()))?;
    let present = installed_ids(&mods_dir);
    let mut mods = Vec::new();
    if let Some(client) = &client {
        if !present.contains("cxvisuals") {
            if let Some(job) = &client.job {
                jobs.push(job.clone());
            }
            mods.push(client.jar.clone());
        }
        for need in &client.requires {
            if present.contains(&need.project) {
                continue;
            }
            let list = modrinth::versions(http, ep, paths, &need.project, &game)?;
            let found = modrinth::pick(list, need.version.as_deref()).ok_or(format!("{} is not out for Minecraft {game} yet", need.project))?;
            let job = found.job(&paths.managed(&need.project)).ok_or(format!("{} has no usable file", need.project))?;
            mods.push(job.path.clone());
            jobs.push(job);
        }
    }

    stage(Stage::Downloading);
    download::run(http, jobs, req.threads, progress, req.verify)?;

    stage(Stage::Starting);
    let natives = paths.natives(&version.id);
    std::fs::create_dir_all(&natives).map_err(|e| e.to_string())?;
    libraries::extract_natives(&libs.natives, &natives)?;
    assets::place(&assets.copies)?;
    if let Some(rt) = &runtime {
        java::finish(rt)?;
    }
    if let Some(c) = client.as_ref().filter(|c| !c.local) {
        prune(&paths.managed("cxvisuals"), std::slice::from_ref(&c.jar));
    }
    for need in client.iter().flat_map(|c| &c.requires) {
        prune(&paths.managed(&need.project), &mods);
    }

    Ok(Prepared {
        game,
        java,
        java_major,
        classpath,
        natives,
        mods,
        game_dir,
        libraries: paths.libraries(),
        assets: paths.assets(),
        game_assets: assets.root,
        asset_index: index.id.clone(),
        client,
        version,
    })
}
