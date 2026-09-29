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
    for instance in request.hub.instances.iter().filter(|i| i.identifier != "S3") {
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
            Some("http://rekuest:80/rekuest")
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
    let agents = rekuest["rekuest"]["service_agents"]
        .as_sequence()
        .expect("service agents");
    assert!(!agents.is_empty());
    for agent in agents {
        assert!(agent.get("secret").is_none());
        let service = agent["service"].as_str().unwrap();
        assert_eq!(
            agent["hook_url"].as_str(),
            Some(format!("http://{service}:80/{service}/_rekuest/hook").as_str())
        );
    }
}
