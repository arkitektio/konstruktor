//! What the services host, and how the one that catalogues it is told.
//!
//! A service's image says which structures it holds and which signals it sends about them
//! (`hosts`). Nothing in the installer acts on that: it hands it, as it was said, to the
//! service of the hub that offers a `catalogue` job — Rekuest — in the facts that
//! service's config is written from. And when what there is to catalogue changes, that
//! job is what is owed: the service is not restarted for it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use konstruktor_core::catalog::ServiceId;
use konstruktor_core::config::hub::{HubConfig, HubConfigOptions};
use konstruktor_core::contract::{
    self, marks, owes_catalogue, Description, Descriptor, Hosts, Job, Said, Signal, Structure,
    CATALOGUE_JOB,
};
use konstruktor_core::generate::IssuedIdentity;
use konstruktor_core::lock::{self, Rendering};
use konstruktor_core::services::{self, ServicePlan};
use serde_norway::Value;

mod support;

/// What Mikro's image says it hosts.
fn mikro_hosts() -> Hosts {
    Hosts {
        structures: vec![Structure {
            identifier: "@mikro/image".into(),
            label: Some("Image".into()),
            description: None,
            descriptors: vec![Descriptor {
                key: "@mikro/n_channels".into(),
                kind: "INT".into(),
                description: Some("How many channels it has".into()),
            }],
        }],
        signals: vec![Signal {
            identifier: "@mikro/image".into(),
            kinds: vec!["CREATED".into(), "DELETED".into()],
            descriptors: vec!["@mikro/n_channels".into()],
            description: None,
        }],
    }
}

/// What the images say: Mikro hosting `hosts`, and Rekuest offering a catalogue job when
/// `catalogues`.
fn said(hosts: Hosts, catalogues: bool) -> Said {
    let mut said = support::said();
    said.get_mut("mikro").unwrap().hosts = hosts;
    if catalogues {
        said.get_mut("rekuest").unwrap().jobs.insert(
            CATALOGUE_JOB.to_string(),
            Job {
                command: vec!["arkitekt-service".into(), "run".into(), "catalogue".into()],
                ..Job::default()
            },
        );
    }
    said
}

fn hub() -> HubConfig {
    support::hub(&HubConfigOptions {
        device_id: "device".into(),
        services: Some(vec![ServiceId::Mikro, ServiceId::Kraph]),
        ..Default::default()
    })
}

fn told(config: &HubConfig, id: ServiceId, said: &Said) -> Value {
    contract::facts(config, id, &IssuedIdentity::default(), said)
}

/// The description is read as the image prints it, defaults and all.
#[test]
fn what_a_service_hosts_is_read_from_its_description() {
    let said: Description = serde_json::from_str(
        r#"{"contract": 2, "name": "mikro", "identifier": "live.arkitekt.mikro",
            "hosts": {
              "structures": [
                {"identifier": "@mikro/image", "label": "Image", "description": null,
                 "descriptors": [{"key": "@mikro/n_channels", "type": "INT", "description": null},
                                 {"key": "@mikro/name"}]}],
              "signals": [{"identifier": "@mikro/image", "descriptors": ["@mikro/n_channels"]}]},
            "source": {"repository": "https://github.com/arkitektio/mikro-server-next",
                       "revision": "abc123"}}"#,
    )
    .expect("a description");
    let structure = &said.hosts.structures[0];
    assert_eq!(structure.identifier, "@mikro/image");
    assert_eq!(structure.descriptors[0].kind, "INT");
    assert_eq!(
        structure.descriptors[1].kind, "ANY",
        "what it is when unsaid"
    );
    assert_eq!(said.hosts.signals[0].kinds, ["CREATED"], "the default");
    let source = said.source.expect("a source");
    assert_eq!(source.revision.as_deref(), Some("abc123"));
    assert_eq!(source.path, "/workspace", "where code sits when unsaid");

    // An image that says neither hosts nothing and names no source.
    let silent: Description =
        serde_json::from_str(r#"{"contract": 2, "name": "x", "identifier": "y"}"#).unwrap();
    assert!(silent.hosts.is_empty() && silent.source.is_none());
    assert!(!silent.catalogues());
}

/// The service that catalogues is told what each peer hosts, in the peer's own words; a
/// peer that hosts nothing carries nothing.
#[test]
fn the_cataloguing_service_is_told_what_its_peers_host() {
    let config = hub();
    let said = said(mikro_hosts(), true);
    let rekuest = told(&config, ServiceId::Rekuest, &said);

    let hosts = &rekuest["peers"]["mikro"]["hosts"];
    assert_eq!(
        hosts["structures"][0]["identifier"].as_str(),
        Some("@mikro/image")
    );
    assert_eq!(hosts["structures"][0]["label"].as_str(), Some("Image"));
    let descriptor = &hosts["structures"][0]["descriptors"][0];
    assert_eq!(descriptor["key"].as_str(), Some("@mikro/n_channels"));
    assert_eq!(descriptor["type"].as_str(), Some("INT"));
    assert_eq!(
        hosts["signals"][0]["kinds"],
        serde_norway::to_value(["CREATED", "DELETED"]).unwrap()
    );
    assert_eq!(
        hosts["signals"][0]["descriptors"][0].as_str(),
        Some("@mikro/n_channels")
    );
    // As the image said it, and so read back as what the image said.
    let back: Hosts = serde_norway::from_value(hosts.clone()).expect("the same shape");
    assert_eq!(back, mikro_hosts());

    // Kraph hosts nothing, and says nothing of it.
    assert!(rekuest["peers"]["kraph"].get("hosts").is_none());
    assert!(rekuest["peers"]["kraph"]["url"].is_string());
}

/// Nobody else has a use for it — and a release from before services said what they host
/// would refuse facts that mention it. So only a service that offers the job is told.
#[test]
fn only_a_service_that_catalogues_is_told() {
    let config = hub();
    let said_to_all = said(mikro_hosts(), true);
    // Kraph is Mikro's peer too, and offers no catalogue job.
    let kraph = told(&config, ServiceId::Kraph, &said_to_all);
    assert!(kraph["peers"]["mikro"].get("hosts").is_none());
    assert!(kraph["peers"]["rekuest"].get("hosts").is_none());

    // A Rekuest that offers no such job — an earlier release — is told nothing either.
    let earlier = said(mikro_hosts(), false);
    let rekuest = told(&config, ServiceId::Rekuest, &earlier);
    assert!(rekuest["peers"]["mikro"].get("hosts").is_none());
}

fn rendering(of: &contract::Marks) -> Rendering {
    Rendering {
        from: of.from.clone(),
        config: "the config's hash".into(),
        hosts: Some(of.hosts.clone()),
        apart: Some(of.apart.clone()),
    }
}

/// A release of a peer that hosts something else changes what Rekuest is told and nothing
/// else about it: its catalogue job is owed, and it is not restarted.
#[test]
fn a_change_in_what_is_hosted_owes_the_job_and_no_restart() {
    let config = hub();
    let before = marks(
        "rekuest:1",
        &told(&config, ServiceId::Rekuest, &said(mikro_hosts(), true)),
        "",
    );

    let mut more = mikro_hosts();
    more.structures.push(Structure {
        identifier: "@mikro/roi".into(),
        label: None,
        description: None,
        descriptors: Vec::new(),
    });
    let after = marks(
        "rekuest:1",
        &told(&config, ServiceId::Rekuest, &said(more, true)),
        "",
    );
    assert_ne!(before.from, after.from, "its config is written again");
    assert_ne!(before.hosts, after.hosts);
    assert_eq!(before.apart, after.apart, "nothing else about it moved");
    assert_eq!(
        owes_catalogue(Some(&rendering(&before)), &after, None),
        Some(true),
        "the job, and only the job"
    );

    // Hosting nothing any more is a change like any other.
    let none = marks(
        "rekuest:1",
        &told(&config, ServiceId::Rekuest, &said(Hosts::default(), true)),
        "",
    );
    assert_eq!(
        owes_catalogue(Some(&rendering(&before)), &none, None),
        Some(true)
    );
}

/// A change that leaves what is hosted alone owes no job: the service is restarted as it
/// always was, and nothing is catalogued again.
#[test]
fn a_change_that_alters_no_hosts_owes_nothing() {
    let config = hub();
    let said = said(mikro_hosts(), true);
    let facts = told(&config, ServiceId::Rekuest, &said);
    let before = marks("rekuest:1", &facts, "");

    // Another build of Rekuest itself, an operator's setting, a peer's new summary.
    let another_build = marks("rekuest:2", &facts, "");
    let overridden = marks("rekuest:1", &facts, "django:\n  debug: true\n");
    let mut reworded = said.clone();
    reworded.get_mut("mikro").unwrap().summary = "Something else".into();
    reworded
        .get_mut("mikro")
        .unwrap()
        .offers
        .endpoints
        .insert("something".into(), "_new".into());
    let another_offer = marks(
        "rekuest:1",
        &told(&config, ServiceId::Rekuest, &reworded),
        "",
    );
    for after in [&another_build, &overridden, &another_offer] {
        assert_ne!(before.from, after.from);
        assert_eq!(before.hosts, after.hosts);
        assert_ne!(before.apart, after.apart);
        assert_eq!(owes_catalogue(Some(&rendering(&before)), after, None), None);
    }

    // Written exactly as before, nothing is owed either.
    assert_eq!(
        owes_catalogue(Some(&rendering(&before)), &before, None),
        None
    );
    // And a service nobody is told hosts of — every one but the cataloguing one — never
    // owes one, whatever its peers come to host.
    let kraph_before = marks("kraph:1", &told(&config, ServiceId::Kraph, &said), "");
    let kraph_after = marks(
        "kraph:1",
        &told(
            &config,
            ServiceId::Kraph,
            &self::said(Hosts::default(), true),
        ),
        "",
    );
    assert_eq!(kraph_before, kraph_after);
}

/// A service that comes or goes changes both: Rekuest is restarted for the peer, as it
/// always was, and catalogues once it is back.
#[test]
fn a_service_that_comes_or_goes_owes_the_job_and_the_restart() {
    let with_mikro = hub();
    let said = said(mikro_hosts(), true);
    let before = marks(
        "rekuest:1",
        &told(&with_mikro, ServiceId::Rekuest, &said),
        "",
    );
    let mut without = with_mikro.clone();
    without.remove_service(ServiceId::Mikro);
    let after = marks("rekuest:1", &told(&without, ServiceId::Rekuest, &said), "");
    assert_ne!(before.hosts, after.hosts);
    assert_ne!(before.apart, after.apart);
    assert_eq!(
        owes_catalogue(Some(&rendering(&before)), &after, None),
        Some(false)
    );
}

/// What is owed stays owed until the job has run, and a later change of another kind
/// brings the restart back.
#[test]
fn what_is_owed_is_kept_until_it_is_run() {
    let config = hub();
    let facts = told(&config, ServiceId::Rekuest, &said(mikro_hosts(), true));
    let before = marks("rekuest:1", &facts, "");
    let overridden = marks("rekuest:1", &facts, "django:\n  debug: true\n");
    assert_eq!(
        owes_catalogue(Some(&rendering(&before)), &overridden, Some(true)),
        Some(false)
    );
    assert_eq!(
        owes_catalogue(Some(&rendering(&before)), &before, Some(true)),
        Some(true)
    );

    // A config written for the first time owes nothing: preparing the service's database
    // runs its setup, the catalogue included.
    assert_eq!(owes_catalogue(None, &before, None), None);
    // One written before the two were told apart owes the job when there is something to
    // tell, and is restarted as it would have been.
    let earlier = Rendering {
        from: "an earlier hash".into(),
        config: "the config's hash".into(),
        hosts: None,
        apart: None,
    };
    assert_eq!(owes_catalogue(Some(&earlier), &before, None), Some(false));
    let nothing = marks(
        "rekuest:1",
        &told(
            &config,
            ServiceId::Rekuest,
            &self::said(Hosts::default(), true),
        ),
        "",
    );
    assert_eq!(owes_catalogue(Some(&earlier), &nothing, None), None);
}

/// The restart that follows a rewritten config leaves out the service that only has
/// something to catalogue — and what moves with it — and keeps everybody else.
#[test]
fn a_service_that_only_has_to_catalogue_is_not_restarted() {
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "konstruktor-hosts-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let config = hub();
    let changed: Vec<String> = ["rekuest.yaml", "mikro.yaml", "Caddyfile"]
        .map(String::from)
        .to_vec();
    let plan = ServicePlan::default();

    // Nothing owed: the mechanism there always was.
    assert_eq!(
        services::restarts(&dir, &config, &changed, &plan),
        ["rekuest", "rekuest-takt", "mikro", "gateway"]
    );

    let owe = |only: bool| {
        let mut held = lock::read(&dir);
        held.recatalogue = BTreeMap::from([("rekuest".to_string(), only)]);
        lock::write(&dir, &held).unwrap();
    };
    // Only what it is told is hosted changed: neither Rekuest nor takt is restarted.
    owe(true);
    assert_eq!(
        services::restarts(&dir, &config, &changed, &plan),
        ["mikro", "gateway"]
    );
    // Something else changed too: it is restarted, and catalogues afterwards.
    owe(false);
    assert_eq!(
        services::restarts(&dir, &config, &changed, &plan),
        ["rekuest", "rekuest-takt", "mikro", "gateway"]
    );
    std::fs::remove_dir_all(&dir).ok();
}

fn rand_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}
