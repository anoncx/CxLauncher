#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Loader {
    #[default]
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

impl Loader {
    pub const ALL: [Loader; 4] = [Loader::Fabric, Loader::Quilt, Loader::Forge, Loader::NeoForge];

    pub fn parse(id: &str) -> Self {
        Self::ALL.into_iter().find(|l| l.id() == id.trim()).unwrap_or_default()
    }

    pub fn id(self) -> &'static str {
        match self {
            Loader::Fabric => "fabric",
            Loader::Quilt => "quilt",
            Loader::Forge => "forge",
            Loader::NeoForge => "neoforge",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Loader::Fabric => "Fabric",
            Loader::Quilt => "Quilt",
            Loader::Forge => "Forge",
            Loader::NeoForge => "NeoForge",
        }
    }

    pub fn pack_key(self) -> &'static str {
        match self {
            Loader::Fabric => "fabric-loader",
            Loader::Quilt => "quilt-loader",
            Loader::Forge => "forge",
            Loader::NeoForge => "neoforge",
        }
    }

    pub fn has_meta_profile(self) -> bool {
        matches!(self, Loader::Fabric | Loader::Quilt)
    }

    pub fn runs_client(self) -> bool {
        self == Loader::Fabric
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_ids() {
        for loader in Loader::ALL {
            assert_eq!(Loader::parse(loader.id()), loader);
        }
        assert_eq!(Loader::parse(""), Loader::Fabric);
    }
}
