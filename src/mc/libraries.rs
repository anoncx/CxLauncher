use super::meta::Library;
use super::rules::Env;
use crate::download::Job;
use std::path::{Path, PathBuf};

pub const MOJANG_LIBRARIES: &str = "https://libraries.minecraft.net/";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coord {
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub classifier: Option<String>,
    pub ext: String,
}

impl Coord {
    pub fn parse(name: &str) -> Option<Self> {
        let (name, ext) = match name.split_once('@') {
            Some((n, e)) => (n, e.to_string()),
            None => (name, "jar".to_string()),
        };
        let mut parts = name.split(':');
        let group = parts.next()?.to_string();
        let artifact = parts.next()?.to_string();
        let version = parts.next()?.to_string();
        let classifier = parts.next().map(str::to_string);
        if group.is_empty() || artifact.is_empty() || version.is_empty() || parts.next().is_some() {
            return None;
        }
        Some(Self { group, artifact, version, classifier, ext })
    }

    pub fn key(&self) -> String {
        match &self.classifier {
            Some(c) => format!("{}:{}:{c}", self.group, self.artifact),
            None => format!("{}:{}", self.group, self.artifact),
        }
    }

    pub fn path(&self) -> String {
        let file = match &self.classifier {
            Some(c) => format!("{}-{}-{c}.{}", self.artifact, self.version, self.ext),
            None => format!("{}-{}.{}", self.artifact, self.version, self.ext),
        };
        format!("{}/{}/{}/{file}", self.group.replace('.', "/"), self.artifact, self.version)
    }
}

#[derive(Clone, Debug)]
pub struct NativeJar {
    pub path: PathBuf,
    pub exclude: Vec<String>,
}

#[derive(Default, Debug)]
pub struct Resolved {
    pub jobs: Vec<Job>,
    pub classpath: Vec<PathBuf>,
    pub natives: Vec<NativeJar>,
}

fn join(base: &str, path: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), path.trim_start_matches('/'))
}

pub fn resolve(libs: &[&Library], env: &Env, dir: &Path) -> Result<Resolved, String> {
    let mut out = Resolved::default();
    for lib in libs {
        let coord = Coord::parse(&lib.name).ok_or(format!("bad library name {}", lib.name))?;
        let artifact = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref());

        if let Some(classifier) = lib.natives.as_ref().and_then(|n| n.get(env.os)) {
            let classifier = classifier.replace("${arch}", env.bits());
            let native = lib.downloads.as_ref().and_then(|d| d.classifiers.as_ref()).and_then(|c| c.get(&classifier));
            let rel = native.and_then(|a| a.path.clone()).unwrap_or_else(|| Coord { classifier: Some(classifier.clone()), ..coord.clone() }.path());
            let path = dir.join(&rel);
            let url = match native {
                Some(a) if !a.url.is_empty() => a.url.clone(),
                _ => join(lib.url.as_deref().unwrap_or(MOJANG_LIBRARIES), &rel),
            };
            out.jobs.push(Job::new(url, path.clone()).sha1(native.and_then(|a| a.sha1.as_deref())).size(native.and_then(|a| a.size)));
            out.natives.push(NativeJar { path, exclude: lib.extract.as_ref().map(|e| e.exclude.clone()).unwrap_or_default() });
            if artifact.is_none() {
                continue;
            }
        }

        let rel = artifact.and_then(|a| a.path.clone()).unwrap_or_else(|| coord.path());
        let path = dir.join(&rel);
        let (url, sha1, size) = match artifact {
            Some(a) => (a.url.clone(), a.sha1.clone(), a.size),
            None => (join(lib.url.as_deref().unwrap_or(MOJANG_LIBRARIES), &rel), lib.sha1.clone(), lib.size),
        };
        if !url.is_empty() {
            out.jobs.push(Job::new(url, path.clone()).sha1(sha1.as_deref()).size(size));
        }
        out.classpath.push(path);
    }
    Ok(out)
}

pub fn extract_natives(natives: &[NativeJar], target: &Path) -> Result<(), String> {
    if natives.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(target).map_err(|e| e.to_string())?;
    for jar in natives {
        let file = std::fs::File::open(&jar.path).map_err(|e| format!("{}: {e}", jar.path.display()))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", jar.path.display()))?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            let name = entry.name().to_string();
            if entry.is_dir() || jar.exclude.iter().any(|x| name.starts_with(x.as_str())) {
                continue;
            }
            let Some(rel) = entry.enclosed_name() else { continue };
            let out = target.join(rel);
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut file = std::fs::File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mc::meta::{Artifact, LibDownloads};
    use std::collections::HashMap;

    #[test]
    fn parses_coordinates() {
        let c = Coord::parse("org.lwjgl:lwjgl:3.3.3:natives-windows").unwrap();
        assert_eq!(c.path(), "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-windows.jar");
        assert_eq!(c.key(), "org.lwjgl:lwjgl:natives-windows");
        let z = Coord::parse("net.fabricmc:intermediary:26.2@zip").unwrap();
        assert_eq!(z.path(), "net/fabricmc/intermediary/26.2/intermediary-26.2.zip");
        assert!(Coord::parse("broken").is_none());
    }

    #[test]
    fn resolves_maven_and_mojang_libraries() {
        let env = Env { os: "windows", arch: "x86_64", os_version: String::new(), features: HashMap::new() };
        let maven = Library { name: "net.fabricmc:fabric-loader:0.19.3".into(), url: Some("https://maven.fabricmc.net".into()), sha1: Some("ab".into()), ..Default::default() };
        let legacy = Library {
            name: "org.lwjgl.lwjgl:lwjgl-platform:2.9.4".into(),
            natives: Some([("windows".to_string(), "natives-windows-${arch}".to_string())].into_iter().collect()),
            downloads: Some(LibDownloads {
                artifact: None,
                classifiers: Some(
                    [("natives-windows-64".to_string(), Artifact { path: Some("n.jar".into()), sha1: None, size: Some(5), url: "https://x/n.jar".into() })]
                        .into_iter()
                        .collect(),
                ),
            }),
            ..Default::default()
        };
        let out = resolve(&[&maven, &legacy], &env, Path::new("/libs")).unwrap();
        assert_eq!(out.jobs[0].url, "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.19.3/fabric-loader-0.19.3.jar");
        assert_eq!(out.classpath, vec![PathBuf::from("/libs/net/fabricmc/fabric-loader/0.19.3/fabric-loader-0.19.3.jar")]);
        assert_eq!(out.natives.len(), 1);
        assert_eq!(out.jobs[1].url, "https://x/n.jar");
    }
}
