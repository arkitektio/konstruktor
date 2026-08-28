use serde::{Deserialize, Serialize};

/// Mirror of `arkitekt_next/server/services/__init__.py :: SERVICE_REGISTRY`, and of the
/// TypeScript `src/deployment/services.ts` this replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceId {
    Rekuest,
    Mikro,
    Fluss,
    Kabinet,
    Kraph,
    Elektro,
    Alpaka,
    Lovekit,
}

/// Declaration order, as `SERVICE_IDS` upstream. Not the generation order — see
/// [`HUB_SERVICE_ORDER`].
pub const SERVICE_IDS: [ServiceId; 8] = [
    ServiceId::Rekuest,
    ServiceId::Mikro,
    ServiceId::Fluss,
    ServiceId::Kabinet,
    ServiceId::Kraph,
    ServiceId::Elektro,
    ServiceId::Alpaka,
    ServiceId::Lovekit,
];

impl ServiceId {
    pub fn as_str(self) -> &'static str {
        match self {
            ServiceId::Rekuest => "rekuest",
            ServiceId::Mikro => "mikro",
            ServiceId::Fluss => "fluss",
            ServiceId::Kabinet => "kabinet",
            ServiceId::Kraph => "kraph",
            ServiceId::Elektro => "elektro",
            ServiceId::Alpaka => "alpaka",
            ServiceId::Lovekit => "lovekit",
        }
    }

    /// `get_buckets()` keys, in declaration order. The config field is `<purpose>_bucket`,
    /// and the order decides both the order buckets are created in `minio_init.yaml` and
    /// the order their routes appear in the Caddyfile — which is byte-compared.
    pub fn bucket_purposes(self) -> &'static [&'static str] {
        match self {
            ServiceId::Mikro => &["media", "zarr", "parquet", "bigfile"],
            ServiceId::Elektro => &["media", "zarr"],
            _ => &["media"],
        }
    }

    /// `_uses_datalayer`. A service that stores no objects itself still gets its buckets
    /// created — it just receives no `datalayer` block, and upstream's models reject one.
    pub fn uses_datalayer(self) -> bool {
        matches!(self, ServiceId::Mikro | ServiceId::Kraph | ServiceId::Elektro)
    }
}

/// The order `diff.write_hub_files` feeds services to the generator, which is the order
/// they appear in the Caddyfile. Deliberately not declaration order, and `lovekit` is
/// absent — it has no published image, so the generator never emits it.
pub const HUB_SERVICE_ORDER: [ServiceId; 7] = [
    ServiceId::Rekuest,
    ServiceId::Kabinet,
    ServiceId::Mikro,
    ServiceId::Fluss,
    ServiceId::Elektro,
    ServiceId::Alpaka,
    ServiceId::Kraph,
];

/// What a picker needs to show for each service. Display copy lives here rather than in
/// the frontend so the CLI's `--services` help and the wizard's list cannot drift.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceMeta {
    pub id: ServiceId,
    pub name: String,
    pub description: String,
    /// Pre-ticked when nothing else is said.
    pub default: bool,
    /// Whether the generator actually emits it. Lovekit has no published image, so
    /// ticking it would change nothing.
    pub emitted: bool,
}

pub fn catalog() -> Vec<ServiceMeta> {
    SERVICE_IDS
        .into_iter()
        .map(|id| {
            let (name, description) = match id {
                ServiceId::Rekuest => ("Rekuest", "Task orchestration and workflow execution"),
                ServiceId::Mikro => ("Mikro", "Microscopy data management and analysis"),
                ServiceId::Fluss => ("Fluss", "Workflow definition and management"),
                ServiceId::Kabinet => ("Kabinet", "Container and deployment management"),
                ServiceId::Kraph => ("Kraph", "Knowledge graph and data relationships"),
                ServiceId::Elektro => ("Elektro", "Electrophysiology data management"),
                ServiceId::Alpaka => ("Alpaka", "AI/ML model management"),
                ServiceId::Lovekit => (
                    "Lovekit",
                    "LiveKit integration for real-time communication",
                ),
            };
            ServiceMeta {
                id,
                name: name.to_string(),
                description: description.to_string(),
                default: matches!(
                    id,
                    ServiceId::Rekuest
                        | ServiceId::Mikro
                        | ServiceId::Fluss
                        | ServiceId::Kabinet
                        | ServiceId::Kraph
                ),
                emitted: !matches!(id, ServiceId::Lovekit),
            }
        })
        .collect()
}

/// The services pre-ticked when the caller says nothing.
pub fn default_services() -> Vec<ServiceId> {
    catalog()
        .into_iter()
        .filter(|s| s.default)
        .map(|s| s.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_lovekit_is_unemitted() {
        let unemitted: Vec<&str> = catalog()
            .iter()
            .filter(|s| !s.emitted)
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(unemitted, ["lovekit"]);
    }

    #[test]
    fn the_defaults_are_the_five_the_wizard_pre_ticks() {
        let names: Vec<&str> = default_services().iter().map(|id| id.as_str()).collect();
        assert_eq!(names, ["rekuest", "mikro", "fluss", "kabinet", "kraph"]);
    }
}
