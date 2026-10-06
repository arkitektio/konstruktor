//! What the tests know of the services, since nothing in the library does any more.
//!
//! A hub is built from what its images say of themselves, and a test has no image to ask.
//! So the descriptions the twelve services of the catalogue give are written down here,
//! once, with the facts the generator used to hold in tables of its own: which buckets
//! each stores into, that each holds an instance key, which ones Rekuest hooks into, their
//! scopes and roles. A test that needs a hub with those facts passes [`said`] explicitly;
//! one that passes nothing gets a hub nothing was said about.
//!
//! Shared by the library's unit tests and by every integration test (`mod support;`).
#![allow(dead_code)]

use std::collections::BTreeMap;

use konstruktor_core::catalog::{ServiceId, SERVICE_IDS};
use konstruktor_core::config::hub::{build_hub_config, HubConfig, HubConfigOptions};
use konstruktor_core::contract::{Description, Job, Needs, Offers, Said, Scope, CONTRACT};

fn scopes(pairs: &[(&str, &str)]) -> Vec<Scope> {
    pairs
        .iter()
        .map(|(key, description)| Scope {
            key: (*key).to_string(),
            description: (*description).to_string(),
        })
        .collect()
}

fn words(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_string()).collect()
}

/// The buckets a service stores into, by purpose, in the order it declares them.
fn storage_of(id: ServiceId) -> &'static [&'static str] {
    match id {
        ServiceId::Mikro => &[
            "media",
            "zarr",
            "parquet",
            "bigfile",
            "fabriks",
            "konnektion",
        ],
        ServiceId::Elektro => &["media", "zarr", "parquet", "bigfile"],
        ServiceId::Kraph => &["media", "zarr", "bigfile"],
        ServiceId::Bank | ServiceId::Kuvert => &["bigfile"],
        _ => &["media"],
    }
}

/// The services that vendor `rekuest-service`: Rekuest runs their periodic actions and
/// receives their signals.
pub const HOOKED: [ServiceId; 7] = [
    ServiceId::Mikro,
    ServiceId::Elektro,
    ServiceId::Kabinet,
    ServiceId::Fluss,
    ServiceId::Alpaka,
    ServiceId::Bank,
    ServiceId::Kuvert,
];

fn roles_of(id: ServiceId) -> Vec<Scope> {
    scopes(match id {
        ServiceId::Rekuest => &[
            ("agent", "Can act as a workflow agent"),
            ("caller", "Can call remote procedures"),
            ("admin", "Full administrative access"),
        ],
        ServiceId::Mikro => &[
            ("admin", "Full administrative access"),
            ("user", "Standard user access"),
            ("viewer", "Read-only access to images"),
            ("uploader", "Can upload new images"),
        ],
        ServiceId::Fluss => &[
            ("admin", "Full administrative access"),
            ("user", "Standard user access"),
            ("designer", "Can design workflows"),
            ("viewer", "Read-only access"),
        ],
        ServiceId::Kabinet => &[
            ("admin", "Full administrative access"),
            ("deployer", "Can deploy containers"),
            ("user", "Standard user access"),
            ("viewer", "Read-only access"),
        ],
        ServiceId::Kraph => &[
            ("admin", "Full administrative access"),
            ("user", "Standard user access"),
            ("editor", "Can edit graph data"),
            ("viewer", "Read-only access"),
        ],
        ServiceId::Elektro => &[
            ("admin", "Full administrative access"),
            ("user", "Standard user access"),
            ("analyst", "Can analyze recordings"),
            ("viewer", "Read-only access"),
        ],
        ServiceId::Alpaka => &[
            ("admin", "Full administrative access"),
            ("user", "Standard user access"),
            ("modeler", "Can manage ML models"),
            ("viewer", "Read-only access"),
        ],
        // None declares roles of its own; their upload grants use the datalayer's
        // default roles.
        _ => &[],
    })
}

fn scopes_of(id: ServiceId) -> Vec<Scope> {
    scopes(match id {
        ServiceId::Rekuest => &[
            ("rekuest_agent", "Act as an agent"),
            ("rekuest_call", "Call other apps with rekuest"),
            ("read", "Read access to rekuest resources"),
            ("write", "Write access to rekuest resources"),
        ],
        ServiceId::Mikro => &[
            ("mikro_read", "Read images from the database"),
            ("mikro_write", "Write images to the database"),
            ("read_image", "Read image data"),
            ("read", "Generic read access"),
            ("write", "Generic write access"),
        ],
        ServiceId::Fluss => &[
            ("fluss_read", "Read workflow definitions"),
            ("fluss_write", "Create and modify workflows"),
            ("fluss_execute", "Execute workflows"),
            ("read", "Generic read access"),
            ("write", "Generic write access"),
        ],
        ServiceId::Kabinet => &[
            ("kabinet_add_repo", "Add repositories to the database"),
            ("kabinet_deploy", "Deploy containers"),
            ("kabinet_read", "Read container definitions"),
            ("read", "Generic read access"),
            ("write", "Generic write access"),
        ],
        ServiceId::Kraph => &[
            ("kraph_read", "Read graph data"),
            ("kraph_write", "Write graph data"),
            ("kraph_query", "Execute graph queries"),
            ("read", "Generic read access"),
            ("write", "Generic write access"),
        ],
        ServiceId::Elektro => &[
            ("elektro_read", "Read electrophysiology data"),
            ("elektro_write", "Write electrophysiology data"),
            ("elektro_analyze", "Run analysis on recordings"),
            ("read", "Generic read access"),
            ("write", "Generic write access"),
        ],
        ServiceId::Alpaka => &[
            ("alpaka_infer", "Run inference on models"),
            ("alpaka_train", "Train ML models"),
            ("alpaka_manage", "Manage model registry"),
            ("read", "Generic read access"),
            ("write", "Generic write access"),
        ],
        ServiceId::Bank => &[
            ("bank_read", "Read bank accounts, transactions and budgets"),
            (
                "bank_write",
                "Link accounts, import statements and edit budgets",
            ),
        ],
        ServiceId::Kuvert => &[
            ("kuvert_read", "Read synced mail"),
            ("kuvert_write", "Link mailboxes, organise and send mail"),
        ],
        ServiceId::Dokuments => &[
            (
                "dokuments_read",
                "Read documents, their pages and their text",
            ),
            (
                "dokuments_write",
                "Add documents and write their pages and text",
            ),
        ],
        ServiceId::Lokate => &[
            ("lokate_read", "Read your backed-up location timeline"),
            ("lokate_write", "Back up your location timeline"),
        ],
        _ => &[],
    })
}

/// The line the hub's manifest carried for each service.
fn summary_of(id: ServiceId) -> &'static str {
    match id {
        ServiceId::Rekuest => "Task orchestration and workflow execution",
        ServiceId::Mikro => "Microscopy data management and analysis",
        ServiceId::Fluss => "Workflow definition and management",
        ServiceId::Kabinet => "Container and deployment management",
        ServiceId::Kraph => "Knowledge graph and data relationships",
        ServiceId::Elektro => "Electrophysiology data management",
        ServiceId::Alpaka => "AI/ML model management",
        ServiceId::Lovekit => "Live video and audio streams, over LiveKit",
        ServiceId::Bank => "Bank accounts, transactions and budgets",
        ServiceId::Kuvert => "Your mailboxes, synced and searchable",
        ServiceId::Dokuments => "Documents, their pages and their text",
        ServiceId::Lokate => "A backup of your phone's location timeline",
        _ => "",
    }
}

/// What one of the catalogue's services says of itself.
pub fn description_of(id: ServiceId) -> Description {
    let hooked = HOOKED.contains(&id);
    Description {
        contract: CONTRACT,
        name: id.as_str().to_string(),
        summary: summary_of(id).to_string(),
        identifier: format!("live.arkitekt.{}", id.as_str()),
        needs: Needs {
            storage: words(storage_of(id)),
            scopes: scopes_of(id),
            roles: roles_of(id),
            instance_key: true,
            peers: if hooked {
                words(&["rekuest"])
            } else {
                Vec::new()
            },
            secrets: if id == ServiceId::Kuvert {
                words(&["fernet"])
            } else {
                Vec::new()
            },
            ..Needs::default()
        },
        offers: Offers {
            health: "ht".into(),
            endpoints: if hooked {
                BTreeMap::from([
                    (
                        "rekuest_service".to_string(),
                        "_rekuest/service".to_string(),
                    ),
                    ("rekuest_hook".to_string(), "_rekuest/hook".to_string()),
                ])
            } else {
                BTreeMap::new()
            },
        },
        ..Description::default()
    }
}

/// What the twelve services of the catalogue say of themselves, by compose service.
pub fn said() -> Said {
    SERVICE_IDS
        .iter()
        .map(|id| (id.as_str().to_string(), description_of(*id)))
        .collect()
}

/// A hub built from `options`, with every service provided what [`said`] says it needs:
/// what `hub create` arrives at once the images have answered.
pub fn hub(options: &HubConfigOptions) -> HubConfig {
    let mut config = build_hub_config(options);
    config.provide(&said());
    config
}

/// A service no catalogue lists, as its image would describe it: one bucket, one secret,
/// no instance key, scopes and roles of its own, and a command to be started with.
pub fn example() -> Description {
    Description {
        contract: CONTRACT,
        name: "example".into(),
        summary: "A minimal service, to read and to copy.".into(),
        identifier: "org.example.service".into(),
        serve: words(&["example-server", "serve"]),
        debug: words(&["example-server", "debug"]),
        jobs: BTreeMap::from([(
            "migrate".to_string(),
            Job {
                command: words(&["example-server", "migrate"]),
                ..Job::default()
            },
        )]),
        prepare: Some("migrate".into()),
        needs: Needs {
            storage: words(&["archive"]),
            scopes: scopes(&[("example_read", "Read examples")]),
            roles: scopes(&[("curator", "Can curate examples")]),
            instance_key: false,
            secrets: words(&["signing"]),
            ..Needs::default()
        },
        offers: Offers {
            health: "healthz".into(),
            endpoints: BTreeMap::new(),
        },
        ..Description::default()
    }
}
