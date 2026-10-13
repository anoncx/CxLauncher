pub mod args;
pub mod assets;
pub mod fabric;
pub mod forge;
pub mod java;
pub mod libraries;
pub mod loader;
pub mod meta;
pub mod modmeta;
pub mod modrinth;
pub mod mrpack;
pub mod rules;

#[derive(Clone, Debug)]
pub struct Endpoints {
    pub version_manifest: String,
    pub resources: String,
    pub java_runtimes: String,
    pub fabric_meta: String,
    pub quilt_meta: String,
    pub forge_promos: String,
    pub forge_maven: String,
    pub neoforge_versions: String,
    pub neoforge_maven: String,
    pub modrinth: String,
    pub client: String,
    pub cloud: String,
    pub live: String,
    pub xbox_user: String,
    pub xsts: String,
    pub services: String,
}

impl Endpoints {
    pub fn configured() -> Self {
        #[cfg(debug_assertions)]
        if let Some(base) = std::env::var("CXLAUNCHER_LOCAL").ok().filter(|b| !b.is_empty()) {
            return Self::local(base.trim_end_matches('/'));
        }
        Self::default()
    }
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            version_manifest: "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json".into(),
            resources: "https://resources.download.minecraft.net".into(),
            java_runtimes: "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json"
                .into(),
            fabric_meta: "https://meta.fabricmc.net/v2".into(),
            quilt_meta: "https://meta.quiltmc.org/v3".into(),
            forge_promos: "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json".into(),
            forge_maven: "https://maven.minecraftforge.net".into(),
            neoforge_versions: "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge".into(),
            neoforge_maven: "https://maven.neoforged.net/releases".into(),
            modrinth: "https://api.modrinth.com/v2".into(),
            client: "https://visuals.cxiy.dev/v2/client".into(),
            cloud: "https://visuals.cxiy.dev".into(),
            live: "https://login.live.com".into(),
            xbox_user: "https://user.auth.xboxlive.com".into(),
            xsts: "https://xsts.auth.xboxlive.com".into(),
            services: "https://api.minecraftservices.com".into(),
        }
    }
}

#[cfg(any(test, debug_assertions))]
impl Endpoints {
    pub fn local(base: &str) -> Self {
        Self {
            version_manifest: format!("{base}/mojang/version_manifest_v2.json"),
            resources: format!("{base}/resources"),
            java_runtimes: format!("{base}/runtime/all.json"),
            fabric_meta: format!("{base}/fabric/v2"),
            quilt_meta: format!("{base}/quilt/v3"),
            forge_promos: format!("{base}/forge/promotions_slim.json"),
            forge_maven: format!("{base}/forge/maven"),
            neoforge_versions: format!("{base}/neoforge/versions"),
            neoforge_maven: format!("{base}/neoforge/maven"),
            modrinth: format!("{base}/modrinth/v2"),
            client: format!("{base}/client"),
            cloud: format!("{base}/cloud"),
            live: format!("{base}/live"),
            xbox_user: format!("{base}/xbl"),
            xsts: format!("{base}/xsts"),
            services: format!("{base}/services"),
        }
    }
}
