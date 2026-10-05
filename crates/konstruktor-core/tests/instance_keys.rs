//! Instance keys: one Ed25519 key per service instance, its public half in the hub manifest,
//! its private half — and the trust the coordination server vouches for — in its config.

use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use konstruktor_core::catalog::{ServiceId, SERVICE_IDS};
use konstruktor_core::config::hub::HubConfig;
use konstruktor_core::connect::manifest::{build_hub_request, HubManifestOptions};
use konstruktor_core::generate::{generate_hub_files, IssuedIdentity};
use konstruktor_core::secrets::raw_public_key_b64;
use serde_norway::Value;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// The fixture profile predates instance keys: Rekuest carries a provenance pair, nobody an
/// instance key.
fn legacy_config() -> HubConfig {
    let text = std::fs::read_to_string(fixtures().join("hub_config.yaml")).expect("fixture");
    let profile: Value = serde_norway::from_str(&text).expect("parses");
    serde_norway::from_value(profile["config"].clone()).expect("deserializes")
}

const HUB_KEYS: &str = "https://coord.example.org/lok/.well-known/hub-keys/7";

fn issued() -> IssuedIdentity {
    IssuedIdentity {
        issuer: Some("https://coord.example.org".into()),
        jwks_url: Some("https://coord.example.org/lok/.well-known/jwks.json".into()),
        hub_keys_url: Some(HUB_KEYS.into()),
    }
}

fn config_yaml(files: &konstruktor_core::generate::GeneratedFiles, id: ServiceId) -> Value {
    let name = format!("configs/{}.yaml", id.as_str());
    serde_norway::from_str(&files[&name]).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn an_old_profile_gets_keys_and_rekuest_keeps_its_provenance_key() {
    let mut config = legacy_config();
    let provenance = config
        .rekuest
        .provenance_key_pair
        .clone()
        .expect("fixture has one");

    assert!(config.ensure_instance_keys());
    assert_eq!(config.rekuest.instance_key_pair.as_ref(), Some(&provenance));
    assert!(
        config.rekuest.provenance_key_pair.is_none(),
        "migrated, not duplicated"
    );
    for id in SERVICE_IDS {
        assert!(
            config.service(id).instance_key_pair.is_some(),
            "{id:?} has a key"
        );
    }
    let publics: std::collections::BTreeSet<_> = SERVICE_IDS
        .iter()
        .map(|id| {
            config
                .service(*id)
                .instance_key_pair
                .clone()
                .unwrap()
                .public_key
        })
        .collect();
    assert_eq!(
        publics.len(),
        SERVICE_IDS.len(),
        "every instance has its own key"
    );

    assert!(!config.ensure_instance_keys(), "minted once, then kept");
}

#[test]
fn the_manifest_carries_each_instances_raw_public_key() {
    let mut config = legacy_config();
    // The fixture's provenance pair is a placeholder, not a real key; mint real ones.
    config.rekuest.provenance_key_pair = None;
    config.ensure_instance_keys();
    let request = build_hub_request(
        &config,
        &HubManifestOptions {
            identifier: "lab-hub".into(),
            description: None,
            node_id: None,
            hosts: vec![],
            reachable_hosts: vec![],
            request_auth_key: false,
            expiration_seconds: None,
            ..Default::default()
        },
    );
    assert!(!request.hub.instances.is_empty());
    // Every service sends its key; the object store is no service of the trust bundle.
    for instance in request
        .hub
        .instances
        .iter()
        .filter(|i| i.identifier != "S3")
    {
        let key = instance
            .manifest
            .challenge_key
            .as_deref()
            .expect("every instance sends its key");
        assert_eq!(BASE64.decode(key).unwrap().len(), 32);
        let id = SERVICE_IDS
            .into_iter()
            .find(|id| instance.manifest.identifier == format!("live.arkitekt.{}", id.as_str()))
            .expect("a known service");
        let pair = config.service(id).instance_key_pair.as_ref().unwrap();
        assert_eq!(Some(key.to_string()), raw_public_key_b64(pair));
    }
}

#[test]
fn every_config_holds_its_own_key_and_trusts_the_hub_bundle() {
    let mut config = legacy_config();
    config.ensure_instance_keys();
    let files = generate_hub_files(&config, &issued());

    for id in [
        ServiceId::Mikro,
        ServiceId::Elektro,
        ServiceId::Kabinet,
        ServiceId::Fluss,
        ServiceId::Alpaka,
        ServiceId::Bank,
        ServiceId::Kuvert,
    ] {
        let block = config.service(id);
        if !(block.enabled && block.image.is_some()) {
            continue;
        }
        let yaml = config_yaml(&files, id);
        assert_eq!(
            yaml["instance"]["private_key"].as_str(),
            Some(
                block
                    .instance_key_pair
                    .as_ref()
                    .unwrap()
                    .private_key
                    .as_str()
            )
        );
        assert_eq!(
            yaml["instance"]["trust"]["jwks_uri"].as_str(),
            Some(HUB_KEYS)
        );
        assert!(
            yaml["rekuest_hook"].get("secret").is_none(),
            "no shared secret any more"
        );
        assert_eq!(
            yaml["rekuest_hook"]["rekuest_url"].as_str(),
            // takt, which serves the agent protocol the reports and signals belong to.
            Some("http://rekuest-takt:8080/rekuest")
        );
        let provenance = &yaml["authentikate"]["provenance"]["issuers"][0];
        assert_eq!(
            provenance["jwks_uri"].as_str(),
            Some(format!("{HUB_KEYS}?service=live.arkitekt.rekuest").as_str())
        );
    }

    let rekuest = config_yaml(&files, ServiceId::Rekuest);
    assert_eq!(rekuest["provenance"]["issuer"].as_str(), Some("rekuest"));
    assert!(
        rekuest["provenance"].get("private_key").is_none(),
        "the instance key signs provenance"
    );
    // The pair's two addresses, in the one file both read: where Rekuest reaches takt, and
    // where takt reaches Rekuest for its upkeep jobs.
    assert_eq!(
        rekuest["rekuest"]["takt_url"].as_str(),
        Some("http://rekuest-takt:8080/rekuest")
    );
    // Rekuest asks takt through the socket the two mount; the URL then only gives the path.
    assert_eq!(
        rekuest["rekuest"]["takt_socket"].as_str(),
        Some("/run/takt/internal.sock")
    );
    assert_eq!(
        rekuest["rekuest"]["server_url"].as_str(),
        Some("http://rekuest:80/rekuest")
    );
    // A service and a hook agent are separate entries, in separate lists, with separate
    // endpoints: neither refers to the other.
    let services = rekuest["rekuest"]["services"]
        .as_sequence()
        .expect("services");
    let hook_agents = rekuest["rekuest"]["hook_agents"]
        .as_sequence()
        .expect("hook agents");
    assert!(!services.is_empty() && services.len() == hook_agents.len());
    for service in services {
        assert!(service.get("secret").is_none() && service.get("hook_url").is_none());
        let name = service["name"].as_str().unwrap();
        assert_eq!(
            service["url"].as_str(),
            Some(format!("http://{name}:80/{name}/_rekuest/service").as_str())
        );
    }
    for agent in hook_agents {
        assert!(agent.get("secret").is_none());
        let name = agent["name"].as_str().unwrap();
        assert_eq!(
            agent["hook_url"].as_str(),
            Some(format!("http://{name}:80/{name}/_rekuest/hook").as_str())
        );
    }
    // Images from before the split still find the one list they read.
    let combined = rekuest["rekuest"]["service_agents"]
        .as_sequence()
        .expect("service agents");
    assert_eq!(combined.len(), hook_agents.len());
}

/// A hub the coordination server handed no `hub_keys_url` (not enrolled yet, or the e2e
/// hub, which is never authorized) still has to trust its own services: every config
/// carries the bundle inline, one key per enabled service, under its service.
#[test]
fn without_a_hub_keys_url_the_bundle_is_written_inline() {
    let mut config = legacy_config();
    // The fixture's provenance pair is a placeholder, not a real key; mint real ones.
    config.rekuest.provenance_key_pair = None;
    config.ensure_instance_keys();
    let files = generate_hub_files(&config, &IssuedIdentity::default());

    let enabled: Vec<ServiceId> = config
        .enabled_services()
        .into_iter()
        .filter(|id| config.service(*id).image.is_some())
        .collect();
    assert!(enabled.len() > 1, "the fixture enables several services");

    let rekuest_jwk = konstruktor_core::secrets::public_jwk(
        config.rekuest.instance_key_pair.as_ref().unwrap(),
        "live.arkitekt.rekuest",
    )
    .unwrap();

    for id in &enabled {
        let yaml = config_yaml(&files, *id);
        let trust = &yaml["instance"]["trust"];
        assert!(trust.get("jwks_uri").is_none(), "{id:?}");
        let keys = trust["jwks"]["keys"]
            .as_sequence()
            .expect("an inline bundle");
        assert_eq!(
            keys.len(),
            enabled.len(),
            "{id:?}: one key per enabled service"
        );

        let services: Vec<&str> = keys
            .iter()
            .map(|k| k["service"].as_str().unwrap())
            .collect();
        for other in &enabled {
            assert!(services.contains(&format!("live.arkitekt.{}", other.as_str()).as_str()));
        }
        for key in keys {
            assert_eq!(key["kty"].as_str(), Some("OKP"));
            assert_eq!(key["crv"].as_str(), Some("Ed25519"));
            assert_eq!(key["alg"].as_str(), Some("Ed25519"));
            assert_eq!(key["use"].as_str(), Some("sig"));
            assert_eq!(
                key["kid"].as_str().map(str::len),
                Some(43),
                "a SHA-256 thumbprint"
            );
        }
        // This service's own entry is its own public key.
        let own = keys
            .iter()
            .find(|k| k["service"].as_str() == Some(&format!("live.arkitekt.{}", id.as_str())))
            .unwrap();
        let expected = konstruktor_core::secrets::public_jwk(
            config.service(*id).instance_key_pair.as_ref().unwrap(),
            "",
        )
        .unwrap();
        assert_eq!(own["x"].as_str(), expected["x"].as_str());

        // Provenance is checked against Rekuest's key alone, inline as well.
        let provenance = &yaml["authentikate"]["provenance"]["issuers"][0];
        assert_eq!(provenance["kind"].as_str(), Some("jwks_dict"));
        assert_eq!(provenance["iss"].as_str(), Some("rekuest"));
        let provenance_keys = provenance["jwks"]["keys"].as_sequence().unwrap();
        assert_eq!(provenance_keys.len(), 1);
        assert_eq!(
            provenance_keys[0]["kid"].as_str(),
            rekuest_jwk["kid"].as_str()
        );
    }
}

/// A Rekuest key that cannot be read (the fixture's placeholder) is no reason to write a
/// provenance issuer with no keys: it falls back to Rekuest's own key set in the network.
#[test]
fn an_unreadable_rekuest_key_falls_back_to_its_key_set() {
    let mut config = legacy_config();
    config.ensure_instance_keys();
    let files = generate_hub_files(&config, &IssuedIdentity::default());
    let yaml = config_yaml(&files, ServiceId::Mikro);
    let provenance = &yaml["authentikate"]["provenance"]["issuers"][0];
    assert_eq!(provenance["kind"].as_str(), Some("jwks_uri"));
    assert_eq!(
        provenance["jwks_uri"].as_str(),
        Some("http://rekuest:80/rekuest/.well-known/jwks.json")
    );
}
