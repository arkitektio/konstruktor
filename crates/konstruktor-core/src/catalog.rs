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
    Bank,
    Kuvert,
    Dokuments,
    Lokate,
}

/// Declaration order, as `SERVICE_IDS` upstream. Not the generation order — see
/// [`HUB_SERVICE_ORDER`].
pub const SERVICE_IDS: [ServiceId; 12] = [
    ServiceId::Rekuest,
    ServiceId::Mikro,
    ServiceId::Fluss,
    ServiceId::Kabinet,
    ServiceId::Kraph,
    ServiceId::Elektro,
    ServiceId::Alpaka,
    ServiceId::Lovekit,
    ServiceId::Bank,
    ServiceId::Kuvert,
    ServiceId::Dokuments,
    ServiceId::Lokate,
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
            ServiceId::Bank => "bank",
            ServiceId::Kuvert => "kuvert",
            ServiceId::Dokuments => "dokuments",
            ServiceId::Lokate => "lokate",
        }
    }

    /// `get_buckets()` keys, in declaration order. The config field is `<purpose>_bucket`,
    /// and the order decides both the order buckets are created in `minio_init.yaml` and
    /// the order their routes appear in the Caddyfile — which is byte-compared.
    ///
    /// Read off what each service's GraphQL schema actually mounts from its vendored
    /// `datalayer.mutations` — every upload or access grant resolves its bucket by purpose
    /// and refuses one that is not configured — and not off the config models, which call
    /// most buckets optional. Elektro's settings even dereference `parquet` on boot and
    /// crash without it. Sparse stores live in `zarr`, so they need nothing of their own.
    pub fn bucket_purposes(self) -> &'static [&'static str] {
        match self {
            ServiceId::Mikro => &["media", "zarr", "parquet", "bigfile", "fabriks", "konnektion"],
            ServiceId::Elektro => &["media", "zarr", "parquet", "bigfile"],
            ServiceId::Kraph => &["media", "zarr", "bigfile"],
            // Statement exports (bank), raw messages and attachments (kuvert): their
            // datalayer declares `bigfile` and nothing else.
            ServiceId::Bank | ServiceId::Kuvert => &["bigfile"],
            // Dokuments keeps its files in `media` (its datalayer requires it); Lovekit and
            // Lokate store no objects, and get the media bucket every service is seeded with.
            _ => &["media"],
        }
    }

    /// Whether the service gets a `datalayer` block. The ones whose schema mounts a
    /// datalayer mutation: Rekuest serves media uploads too. A service that stores no
    /// objects itself still gets its bucket created, just no block.
    pub fn uses_datalayer(self) -> bool {
        matches!(
            self,
            ServiceId::Mikro
                | ServiceId::Kraph
                | ServiceId::Elektro
                | ServiceId::Rekuest
                | ServiceId::Bank
                | ServiceId::Kuvert
                | ServiceId::Dokuments
        )
    }
}

/// The order `diff.write_hub_files` feeds services to the generator, which is the order
/// they appear in the Caddyfile. Deliberately not declaration order. The experimental
/// services come last, so a hub without them keeps its Caddyfile byte for byte.
///
/// Lovekit is here since it has an image. Profiles written before that (and upstream's)
/// still say `lovekit: enabled: true` with no image, which never ran anything — so a
/// service only counts as running when it is enabled *and* has an image
/// ([`crate::config::hub::ServiceBlock::runs`]).
pub const HUB_SERVICE_ORDER: [ServiceId; 12] = [
    ServiceId::Rekuest,
    ServiceId::Kabinet,
    ServiceId::Mikro,
    ServiceId::Fluss,
    ServiceId::Elektro,
    ServiceId::Alpaka,
    ServiceId::Kraph,
    ServiceId::Bank,
    ServiceId::Kuvert,
    ServiceId::Lovekit,
    ServiceId::Dokuments,
    ServiceId::Lokate,
];

/// The services that vendor `rekuest-service`: Rekuest runs their periodic actions and
/// receives their signals, each call signed with the sender's instance key. A hub cannot
/// take its Rekuest out while one of these runs.
pub const HOOKED_SERVICES: [ServiceId; 7] = [
    ServiceId::Mikro,
    ServiceId::Elektro,
    ServiceId::Kabinet,
    ServiceId::Fluss,
    ServiceId::Alpaka,
    ServiceId::Bank,
    ServiceId::Kuvert,
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
    /// Whether the generator actually emits it. Every service does now; kept so a
    /// placeholder can be listed again without the front ends offering a dead switch.
    pub emitted: bool,
    /// Offered, but kept apart from the rest: new, personal-use services that a lab hub
    /// does not need. The wizard lists them under a collapsed "Experimental" section.
    pub experimental: bool,
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
                    "Live video and audio streams, over LiveKit",
                    "Broadcasts and live streams — from people and from apps, such as \
                     a microscope's camera — carried by a LiveKit media server that \
                     runs alongside it. Media flows directly on ports 2757/tcp and \
                     2758/udp, so it works on this machine's network, not across the \
                     internet or the mesh. Experimental.",
                ),
                ServiceId::Bank => (
                    "Bank",
                    "Bank accounts, transactions and budgets",
                    "Link bank and broker accounts (Enable Banking, Scalable Capital) or \
                     import statements, and get categories, budgets and recurring \
                     payments. Linking banks needs Enable Banking credentials in its \
                     config. Experimental, meant for personal use.",
                ),
                ServiceId::Kuvert => (
                    "Kuvert",
                    "Your mailboxes, synced and searchable",
                    "Link existing IMAP, Gmail or Outlook mailboxes to sync, search, \
                     organise and send mail. Kuvert hosts no mailboxes itself. \
                     Experimental.",
                ),
                ServiceId::Dokuments => (
                    "Dokuments",
                    "Documents, their pages and their text",
                    "Keep PDFs and other documents in datasets, page by page: each \
                     page's image, and the text an app recognised on it (OCR), so a \
                     document can be found by what it says. Experimental.",
                ),
                ServiceId::Lokate => (
                    "Lokate",
                    "A backup of your phone's location timeline",
                    "A self-hosted backup of a phone's location timeline: the phone \
                     records and segments its own points, visits and trips, and Lokate \
                     keeps a copy it can restore from. Every user sees only their own \
                     data. Experimental, meant for personal use.",
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
                emitted: true,
                experimental: matches!(
                    id,
                    ServiceId::Lovekit
                        | ServiceId::Bank
                        | ServiceId::Kuvert
                        | ServiceId::Dokuments
                        | ServiceId::Lokate
                ),
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
    fn every_service_is_emitted() {
        let unemitted: Vec<&str> = catalog()
            .iter()
            .filter(|s| !s.emitted)
            .map(|s| s.id.as_str())
            .collect();
        assert!(unemitted.is_empty(), "{unemitted:?}");
    }

    /// Every service is generated, so every one has a place in the generation order.
    #[test]
    fn every_service_has_a_generation_slot() {
        for id in SERVICE_IDS {
            assert!(HUB_SERVICE_ORDER.contains(&id), "{id:?}");
        }
    }

    #[test]
    fn the_defaults_are_what_the_wizard_pre_ticks() {
        let names: Vec<&str> = default_services().iter().map(|id| id.as_str()).collect();
        assert_eq!(
            names,
            ["rekuest", "mikro", "fluss", "kabinet", "kraph", "alpaka"]
        );
    }

    /// Elektro is the one stable service left off on purpose. Pinned so it does not change
    /// by accident.
    #[test]
    fn elektro_is_offered_but_not_pre_ticked() {
        let elektro = catalog()
            .into_iter()
            .find(|s| s.id == ServiceId::Elektro)
            .expect("elektro is in the catalog");
        assert!(elektro.emitted, "it must be offerable");
        assert!(!elektro.default, "but not chosen for people");
    }

    /// The experimental services can be switched on, but never are unless somebody asks,
    /// and the wizard keeps them apart from the rest.
    #[test]
    fn the_experimental_services_are_never_pre_ticked() {
        let experimental: Vec<&str> = catalog()
            .iter()
            .filter(|s| s.experimental)
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(
            experimental,
            ["lovekit", "bank", "kuvert", "dokuments", "lokate"]
        );
        for service in catalog().into_iter().filter(|s| s.experimental) {
            assert!(service.emitted, "{:?} must be offerable", service.id);
            assert!(!service.default, "{:?} must not be pre-ticked", service.id);
        }
    }
}
