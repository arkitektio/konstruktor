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
        matches!(
            self,
            ServiceId::Mikro | ServiceId::Kraph | ServiceId::Elektro
        )
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
    /// What the service is actually for, in the words somebody deciding whether they
    /// need it would use. The one-line `description` names it; this says who wants it.
    pub purpose: String,
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
            let (name, description, purpose) = match id {
                ServiceId::Rekuest => (
                    "Rekuest",
                    "Task orchestration and workflow execution",
                    "The one every other service checks against. It hands out the work, \
                     runs it wherever an app has registered itself, and signs what \
                     happened so the result can be traced back to the code and the \
                     person that produced it.",
                ),
                ServiceId::Mikro => (
                    "Mikro",
                    "Microscopy data management and analysis",
                    "Where images and their metadata live. Acquisitions, stacks, \
                     regions of interest and the results of analysing them, stored so \
                     they can be found again by what they are rather than by filename.",
                ),
                ServiceId::Fluss => (
                    "Fluss",
                    "Workflow definition and management",
                    "The workflow editor and the graphs it produces. Take the tasks \
                     Rekuest knows about, wire them into a pipeline, and keep it as \
                     something that can be run again and shared.",
                ),
                ServiceId::Kabinet => (
                    "Kabinet",
                    "Container and deployment management",
                    "The app store. It tracks which analysis containers exist, what \
                     each one offers, and lets them be installed into this hub without \
                     anybody touching a compose file.",
                ),
                ServiceId::Kraph => (
                    "Kraph",
                    "Knowledge graph and data relationships",
                    "The graph that ties the rest together: which sample an image came \
                     from, which experiment it belonged to, what was measured. For \
                     asking questions that span more than one dataset.",
                ),
                ServiceId::Elektro => (
                    "Elektro",
                    "Electrophysiology traces and recordings",
                    "What Mikro is for images, Elektro is for electrophysiology: patch \
                     clamp and multi-electrode recordings, their stimuli and their \
                     metadata, stored so a trace can be found by what it is. Add it if \
                     this hub will hold recordings — it is off by default because a hub \
                     that will not gains a container and a database for nothing.",
                ),
                ServiceId::Alpaka => (
                    "Alpaka",
                    "Language models, chat and agents",
                    "Language models, and the chat and agent interfaces over them, \
                     offered to the platform as another kind of task. It needs a \
                     provider to talk to: its settings can run an Ollama container \
                     alongside this hub, or point at one that already exists.",
                ),
                ServiceId::Lovekit => (
                    "Lovekit",
                    "LiveKit integration for real-time communication",
                    "Live video and audio between people using the platform, over \
                     LiveKit. Not published yet.",
                ),
            };
            ServiceMeta {
                id,
                name: name.to_string(),
                description: description.to_string(),
                purpose: purpose.split_whitespace().collect::<Vec<_>>().join(" "),
                // Alpaka is on by default: a hub without it can still be asked
                // questions, but nothing answers them, and adding it later means
                // regenerating the stack. Elektro is deliberately *not* — it is for one
                // kind of data, and a hub that will never hold traces gains a container
                // and a database for nothing.
                default: matches!(
                    id,
                    ServiceId::Rekuest
                        | ServiceId::Mikro
                        | ServiceId::Fluss
                        | ServiceId::Kabinet
                        | ServiceId::Kraph
                        | ServiceId::Alpaka
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
    fn the_defaults_are_what_the_wizard_pre_ticks() {
        let names: Vec<&str> = default_services().iter().map(|id| id.as_str()).collect();
        assert_eq!(
            names,
            ["rekuest", "mikro", "fluss", "kabinet", "kraph", "alpaka"]
        );
    }

    /// Elektro is the one emitted service left off on purpose, and Lovekit the one that
    /// cannot be switched on at all. Pinned so neither changes by accident.
    #[test]
    fn elektro_is_offered_but_not_pre_ticked() {
        let elektro = catalog()
            .into_iter()
            .find(|s| s.id == ServiceId::Elektro)
            .expect("elektro is in the catalog");
        assert!(elektro.emitted, "it must be offerable");
        assert!(!elektro.default, "but not chosen for people");
    }
}
