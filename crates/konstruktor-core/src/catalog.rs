use std::collections::BTreeSet;
use std::fmt;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A service a hub runs, by name: `rekuest`, `mikro`, or one this build has never heard of.
///
/// What a service *is* — what it needs from a hub, what it is registered as, how it is
/// started — its own image says ([`crate::contract`]), so nothing here has to list the
/// services there are. The name is all an id holds, and it is interned: every distinct
/// name is kept once for the life of the process, which is what lets an id stay `Copy`
/// and be passed around by value like the enum it used to be.
///
/// The services this build knows more about — a default image, a line of display text — are the [catalogue](catalog), and are addressable as constants
/// (`ServiceId::Rekuest`). A service outside it is every bit as much a service.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServiceId(&'static str);

#[allow(non_upper_case_globals)]
impl ServiceId {
    pub const Rekuest: ServiceId = ServiceId("rekuest");
    pub const Mikro: ServiceId = ServiceId("mikro");
    pub const Fluss: ServiceId = ServiceId("fluss");
    pub const Kabinet: ServiceId = ServiceId("kabinet");
    pub const Kraph: ServiceId = ServiceId("kraph");
    pub const Elektro: ServiceId = ServiceId("elektro");
    pub const Alpaka: ServiceId = ServiceId("alpaka");
    pub const Lovekit: ServiceId = ServiceId("lovekit");
    pub const Bank: ServiceId = ServiceId("bank");
    pub const Kuvert: ServiceId = ServiceId("kuvert");
    pub const Dokuments: ServiceId = ServiceId("dokuments");
    pub const Lokate: ServiceId = ServiceId("lokate");
}

/// The compose services a hub runs that are not its services: the infrastructure, and what
/// runs beside one service in particular. A service cannot take one of these names, or the
/// two would be one entry in the compose file. What runs beside a service under a name
/// made from its own (`rekuest-takt`) needs no entry: a service's name holds no hyphen.
const RESERVED_NAMES: [&str; 12] = [
    "db",
    "daten",
    "redis",
    "rustfs",
    "rustfs_init",
    "minio",
    "gateway",
    "lok",
    "tailscale",
    "reporter",
    "ollama",
    "livekit",
];

/// Why a name cannot be a service's.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidServiceName {
    #[error(
        "`{0}` cannot be a service's name: a name is lower-case letters, digits and `_`, and \
         starts with a letter"
    )]
    Shape(String),
    #[error(
        "`{0}` cannot be a service's name: a hub already runs something under it, beside its \
         services"
    )]
    Reserved(String),
}

impl ServiceId {
    /// The id of the service called `name`, whatever the name is.
    ///
    /// Not a check: a name that comes out of a profile this build wrote, or off a compose
    /// label, is taken as it is. One that comes from outside — a command line, an image's
    /// description — goes through [`Self::parse`] instead.
    pub fn named(name: &str) -> ServiceId {
        if let Some(known) = SERVICE_IDS.iter().find(|id| id.0 == name) {
            return *known;
        }
        static NAMES: OnceLock<Mutex<BTreeSet<&'static str>>> = OnceLock::new();
        let mut names = NAMES
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(held) = names.get(name) {
            return ServiceId(held);
        }
        // Leaked on purpose, once per distinct name: a hub has a handful of services, and
        // an id that borrowed its name could not be `Copy`.
        let held: &'static str = Box::leak(name.to_owned().into_boxed_str());
        names.insert(held);
        ServiceId(held)
    }

    /// The id of the service called `name`, if a hub can have a service of that name.
    ///
    /// The name becomes a compose service, a path on the gateway, the stem of every bucket
    /// and file of the service, and the first half of each of its databases' names
    /// ([`crate::config::hub::database_name`]) — so it is held to the strictest of those,
    /// a name Postgres takes unquoted: a lower-case letter, then lower-case letters,
    /// digits and `_`. No hyphen. And it must not be what the hub calls something else it
    /// runs.
    pub fn parse(name: &str) -> Result<ServiceId, InvalidServiceName> {
        let name = name.trim();
        if !crate::config::hub::plain_identifier(name) {
            return Err(InvalidServiceName::Shape(name.to_string()));
        }
        if RESERVED_NAMES.contains(&name) {
            return Err(InvalidServiceName::Reserved(name.to_string()));
        }
        Ok(ServiceId::named(name))
    }

    pub fn as_str(self) -> &'static str {
        self.0
    }

    /// What this build knows of the service beyond its name, if it is in the catalogue.
    pub fn known(self) -> Option<&'static Known> {
        KNOWN.iter().find(|known| known.id == self)
    }

    /// The image a new hub runs the service on, when nobody names another: the catalogue's.
    /// A service outside it has none, and runs the image it was named by.
    pub fn default_image(self) -> Option<&'static str> {
        self.known().map(|known| known.image)
    }
}

impl fmt::Debug for ServiceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl fmt::Display for ServiceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl Serialize for ServiceId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for ServiceId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Ok(ServiceId::named(&name))
    }
}

/// What this build knows of a service beyond what the service's image says of itself: the
/// things an installer has to bring to a hub that does not exist yet.
#[derive(Debug, Clone, Copy)]
pub struct Known {
    pub id: ServiceId,
    /// How a picker and the hub's manifest call it.
    pub name: &'static str,
    pub description: &'static str,
    pub purpose: &'static str,
    /// Pre-ticked when nothing else is said.
    pub default: bool,
    pub experimental: bool,
    /// The image a new hub is seeded with: the repository on a **major**, not on `latest`.
    /// See [`crate::config::hub::caught_up_image`] for the contract that tag stands for.
    pub image: &'static str,
}

/// The catalogue, in declaration order. Not the generation order — see
/// [`HUB_SERVICE_ORDER`].
///
/// Alpaka is on by default: a hub without it can still be asked questions, but nothing
/// answers them, and adding it later means regenerating the stack. Elektro is deliberately
/// *not* — it is for one kind of data, and a hub that will never hold traces gains a
/// container and a database for nothing. The experimental ones are offered, and never
/// switched on unless asked for.
const KNOWN: [Known; 12] = [
    Known {
        id: ServiceId::Rekuest,
        name: "Rekuest",
        description: "Task orchestration and workflow execution",
        purpose: "The one every other service checks against. It hands out the work, \
                  runs it wherever an app has registered itself, and signs what \
                  happened so the result can be traced back to the code and the \
                  person that produced it.",
        default: true,
        experimental: false,
        image: "jhnnsrs/rekuest:7",
    },
    Known {
        id: ServiceId::Mikro,
        name: "Mikro",
        description: "Microscopy data management and analysis",
        purpose: "Where images and their metadata live. Acquisitions, stacks, \
                  regions of interest and the results of analysing them, stored so \
                  they can be found again by what they are rather than by filename.",
        default: true,
        experimental: false,
        image: "jhnnsrs/mikro:7",
    },
    Known {
        id: ServiceId::Fluss,
        name: "Fluss",
        description: "Workflow definition and management",
        purpose: "The workflow editor and the graphs it produces. Take the tasks \
                  Rekuest knows about, wire them into a pipeline, and keep it as \
                  something that can be run again and shared.",
        default: true,
        experimental: false,
        image: "jhnnsrs/fluss:4",
    },
    Known {
        id: ServiceId::Kabinet,
        name: "Kabinet",
        description: "Container and deployment management",
        purpose: "The app store. It tracks which analysis containers exist, what \
                  each one offers, and lets them be installed into this hub without \
                  anybody touching a compose file.",
        default: true,
        experimental: false,
        image: "jhnnsrs/kabinet:6",
    },
    Known {
        id: ServiceId::Kraph,
        name: "Kraph",
        description: "Knowledge graph and data relationships",
        purpose: "The graph that ties the rest together: which sample an image came \
                  from, which experiment it belonged to, what was measured. For \
                  asking questions that span more than one dataset.",
        default: true,
        experimental: false,
        image: "jhnnsrs/kraph:2",
    },
    Known {
        id: ServiceId::Elektro,
        name: "Elektro",
        description: "Electrophysiology traces and recordings",
        purpose: "What Mikro is for images, Elektro is for electrophysiology: patch \
                  clamp and multi-electrode recordings, their stimuli and their \
                  metadata, stored so a trace can be found by what it is. Add it if \
                  this hub will hold recordings — it is off by default because a hub \
                  that will not gains a container and a database for nothing.",
        default: false,
        experimental: false,
        image: "jhnnsrs/elektro:5",
    },
    Known {
        id: ServiceId::Alpaka,
        name: "Alpaka",
        description: "Language models, chat and agents",
        purpose: "Language models, and the chat and agent interfaces over them, \
                  offered to the platform as another kind of task. It needs a \
                  provider to talk to: its settings can run an Ollama container \
                  alongside this hub, or point at one that already exists.",
        default: true,
        experimental: false,
        image: "jhnnsrs/alpaka:5",
    },
    Known {
        id: ServiceId::Lovekit,
        name: "Lovekit",
        description: "Live video and audio streams, over LiveKit",
        purpose: "Broadcasts and live streams — from people and from apps, such as \
                  a microscope's camera — carried by a LiveKit media server that \
                  runs alongside it. Media flows directly on ports 2757/tcp and \
                  2758/udp, so it works on this machine's network, not across the \
                  internet or the mesh. Experimental.",
        default: false,
        experimental: true,
        image: "jhnnsrs/lovekit:3",
    },
    Known {
        id: ServiceId::Bank,
        name: "Bank",
        description: "Bank accounts, transactions and budgets",
        purpose: "Link bank and broker accounts (Enable Banking, Scalable Capital) or \
                  import statements, and get categories, budgets and recurring \
                  payments. Linking banks needs Enable Banking credentials in its \
                  config. Experimental, meant for personal use.",
        default: false,
        experimental: true,
        image: "jhnnsrs/bank:5",
    },
    Known {
        id: ServiceId::Kuvert,
        name: "Kuvert",
        description: "Your mailboxes, synced and searchable",
        purpose: "Link existing IMAP, Gmail or Outlook mailboxes to sync, search, \
                  organise and send mail. Kuvert hosts no mailboxes itself. \
                  Experimental.",
        default: false,
        experimental: true,
        image: "jhnnsrs/kuvert:5",
    },
    Known {
        id: ServiceId::Dokuments,
        name: "Dokuments",
        description: "Documents, their pages and their text",
        purpose: "Keep PDFs and other documents in datasets, page by page: each \
                  page's image, and the text an app recognised on it (OCR), so a \
                  document can be found by what it says. Experimental.",
        default: false,
        experimental: true,
        image: "jhnnsrs/dokuments:2",
    },
    Known {
        id: ServiceId::Lokate,
        name: "Lokate",
        description: "A backup of your phone's location timeline",
        purpose: "A self-hosted backup of a phone's location timeline: the phone \
                  records and segments its own points, visits and trips, and Lokate \
                  keeps a copy it can restore from. Every user sees only their own \
                  data. Experimental, meant for personal use.",
        default: false,
        experimental: true,
        image: "jhnnsrs/lokate:3",
    },
];

/// The services of the catalogue, in declaration order: the ones `--services` and the
/// wizard offer by name. Not every service there is — a hub can run one that is not here.
pub const SERVICE_IDS: &[ServiceId] = &[
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

/// The order the catalogue's services are fed to the generator in, which is the order they
/// appear in the Caddyfile. Deliberately not declaration order. The experimental services
/// come last, so a hub without them keeps its Caddyfile byte for byte.
///
/// A service outside the catalogue has no place in this list; [`in_generation_order`] puts
/// those after it, by name.
pub const HUB_SERVICE_ORDER: &[ServiceId] = &[
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

/// `ids` in the order the generator takes services: the catalogue's in
/// [`HUB_SERVICE_ORDER`], then every other by name. Deterministic whatever order they came
/// in, and for a hub of catalogue services exactly the order it always was.
pub fn in_generation_order(ids: impl IntoIterator<Item = ServiceId>) -> Vec<ServiceId> {
    let mut out: Vec<ServiceId> = ids.into_iter().collect();
    out.sort_by_key(|id| {
        let slot = HUB_SERVICE_ORDER.iter().position(|known| known == id);
        (slot.unwrap_or(usize::MAX), id.as_str())
    });
    out.dedup();
    out
}

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
    KNOWN
        .iter()
        .map(|known| ServiceMeta {
            id: known.id,
            name: known.name.to_string(),
            description: known.description.to_string(),
            purpose: known
                .purpose
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            default: known.default,
            emitted: true,
            experimental: known.experimental,
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

    /// Every catalogue service is generated, so every one has a place in the generation
    /// order — and the table behind the catalogue lists exactly the same services.
    #[test]
    fn every_service_has_a_generation_slot() {
        for id in SERVICE_IDS {
            assert!(HUB_SERVICE_ORDER.contains(id), "{id:?}");
            assert!(id.known().is_some(), "{id:?}");
        }
        assert_eq!(SERVICE_IDS.len(), HUB_SERVICE_ORDER.len());
        let listed: Vec<ServiceId> = KNOWN.iter().map(|known| known.id).collect();
        assert_eq!(listed, SERVICE_IDS);
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

    /// A name is the same id however often it is asked for, a known one is its constant,
    /// and an id travels as the plain name.
    #[test]
    fn a_service_is_its_name() {
        assert_eq!(ServiceId::named("rekuest"), ServiceId::Rekuest);
        let example = ServiceId::named("example");
        assert_eq!(example, ServiceId::named("example"));
        assert_eq!(example.as_str(), "example");
        assert!(example.known().is_none() && example.default_image().is_none());
        assert_eq!(format!("{example} {example:?}"), "example example");
        assert_eq!(serde_json::to_string(&example).unwrap(), "\"example\"");
        assert_eq!(
            serde_json::from_str::<ServiceId>("\"mikro\"").unwrap(),
            ServiceId::Mikro
        );
        assert!(matches!(ServiceId::named("mikro"), ServiceId::Mikro));
    }

    /// A name from outside is held to what a compose service, a gateway path and a
    /// database can all be called, and never takes what the hub runs beside its services.
    #[test]
    fn an_invalid_or_reserved_name_is_refused() {
        for ok in ["example", "omero_ark", "my_service2", "mikro"] {
            assert_eq!(ServiceId::parse(ok).map(ServiceId::as_str), Ok(ok), "{ok}");
        }
        for bad in [
            "",
            "Example",
            "2fast",
            "-dash",
            "_under",
            "has space",
            "a/b",
            "é",
            // A hyphen is not a character a database name takes, and a service's name is
            // the first half of one.
            "omero-ark",
            "rekuest-takt",
            "example-takt",
        ] {
            assert!(
                matches!(ServiceId::parse(bad), Err(InvalidServiceName::Shape(_))),
                "{bad}"
            );
        }
        for taken in [
            "db",
            "redis",
            "rustfs",
            "rustfs_init",
            "gateway",
            "lok",
            "tailscale",
            "reporter",
            "ollama",
            "livekit",
        ] {
            let refused = ServiceId::parse(taken).unwrap_err();
            assert!(
                matches!(refused, InvalidServiceName::Reserved(_)),
                "{taken}"
            );
            assert!(refused.to_string().contains(taken), "{refused}");
        }
    }

    /// The catalogue's services keep the order they always had; the rest follow, by name.
    #[test]
    fn unknown_services_are_generated_after_the_catalogues() {
        let ordered = in_generation_order([
            ServiceId::named("zebra"),
            ServiceId::Mikro,
            ServiceId::named("example"),
            ServiceId::Rekuest,
            ServiceId::Kabinet,
        ]);
        let names: Vec<&str> = ordered.iter().map(|id| id.as_str()).collect();
        assert_eq!(names, ["rekuest", "kabinet", "mikro", "example", "zebra"]);
    }
}
