//! A *plugin engine*: a deployment that runs plugin containers rather than services.
//!
//! Where a hub is a stack of Django services with a database and object storage behind a
//! gateway, an engine is one container — `jhnnsrs/deployer:next` — with the Docker socket
//! handed to it, so it can start and stop the plugin containers an organization installs
//! through Kabinet. It is a deployment in every other respect: its own folder, its own
//! compose project, its own row in the registry, and the same dashboard.
//!
//! The two paths are deliberately separate rather than one wizard with a switch. Almost
//! nothing a hub is asked applies to an engine — no services, no ports, no addresses to
//! advertise, no mesh — and folding them together would mean a wizard mostly made of
//! questions that do not apply.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_norway::Value;

use crate::connect::app::{self, AppEnvelope, AppManifest};
use crate::create::{now_rfc3339, CreateError, CreateEvent};
use crate::docker;
use crate::generate::service::{list, map, s};
use crate::generate::write::write_generated_files;
use crate::generate::GeneratedFiles;
use crate::registry;

/// The image the engine runs. Pinned by tag, like every other image this app writes.
pub const DEPLOYER_IMAGE: &str = "jhnnsrs/deployer:next";

/// The compose service the engine runs under, and the name its config file is keyed by.
pub const DEPLOYER_SERVICE: &str = "deployer";

/// Where the daemon is reached, and where an engine's whole point lies: without the
/// socket it cannot start a single plugin.
const DOCKER_SOCKET: &str = "/var/run/docker.sock";

/// The engine's own config, in the deployment folder and inside the container.
const CONFIG_FILE: &str = "configs/deployer.yaml";
const CONFIG_MOUNT: &str = "/workspace/config.yaml";

/// What an engine asks to be, when it asks. Reverse-DNS like every Arkitekt manifest.
const ENGINE_APP_IDENTIFIER: &str = "live.arkitekt.deployer";
const ENGINE_APP_VERSION: &str = "1.0.0";

/// What a deployer needs to be allowed to do: read what it should be running, and say
/// what it is running.
const ENGINE_SCOPES: [&str; 2] = ["read", "write"];

/// Everything a front end has to collect for an engine. Flat and serde-friendly, like
/// [`crate::create::HubAnswers`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineAnswers {
    pub dir: String,
    pub name: String,
    /// The coordination server the engine configures itself against.
    pub coord_server: String,
    /// How the engine is known there. Unique within the organization that accepts it.
    pub identifier: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Run `docker compose up -d` once everything is written.
    #[serde(default)]
    pub start: bool,
}

pub struct CreatedEngine {
    pub path: PathBuf,
    pub record: registry::DeploymentRecord,
}

/// The `docker-compose.yaml` an engine deployment consists of.
///
/// One service. `restart: unless-stopped` rather than the hub's `on-failure` policy: an
/// engine is a long-running agent whose job is to be there when somebody installs a
/// plugin, and a machine that reboots should come back with it running.
pub fn build_engine_compose() -> Value {
    map(vec![(
        "services",
        map(vec![(
            DEPLOYER_SERVICE,
            map(vec![
                ("image", s(DEPLOYER_IMAGE)),
                ("restart", s("unless-stopped")),
                ("stop_grace_period", s("2s")),
                (
                    "volumes",
                    list(vec![
                        // The socket is bind-mounted, not proxied: the deployer starts
                        // sibling containers on this machine's daemon rather than
                        // running a daemon of its own.
                        s(&format!("{DOCKER_SOCKET}:{DOCKER_SOCKET}")),
                        // Its identity, read-only. This is the file the device-code flow
                        // produced: a client id and a refresh token, which is all the
                        // engine needs to get itself an access token from then on.
                        s(&format!("./{CONFIG_FILE}:{CONFIG_MOUNT}:ro")),
                    ]),
                ),
            ]),
        )]),
    )])
}

/// Every file an engine deployment consists of, keyed by its path in the folder.
pub fn generate_engine_files(answers: &EngineAnswers, granted: &AppEnvelope) -> GeneratedFiles {
    let mut files = GeneratedFiles::new();
    files.insert(
        "docker-compose.yaml".to_string(),
        crate::generate::dump(&build_engine_compose()),
    );
    files.insert(
        CONFIG_FILE.to_string(),
        crate::generate::dump(&build_engine_config(answers, granted)),
    );
    files
}

/// The engine's identity, as the container reads it.
///
/// The two values that matter are the ones the grant produced: `client_id` and
/// `refresh_token`. Together they are a client that can mint its own access tokens for as
/// long as the organization lets it, which is why they — and not the access token, which
/// expires within the hour — are what gets written down and mounted.
///
/// The key names under `fakts` follow what a fakts-next client reads. They are the one
/// part of this file that was not handed over by the server; everything in it is.
fn build_engine_config(answers: &EngineAnswers, granted: &AppEnvelope) -> Value {
    let mut fakts = vec![
        (
            "endpoint_url",
            s(&crate::connect::wellknown::base_url(&answers.coord_server)),
        ),
        ("client_id", s(&granted.client_id)),
    ];
    if let Some(secret) = granted.client_secret.as_deref().filter(|v| !v.is_empty()) {
        fakts.push(("client_secret", s(secret)));
    }
    if let Some(refresh) = granted.refresh_token.as_deref().filter(|v| !v.is_empty()) {
        fakts.push(("refresh_token", s(refresh)));
    }

    map(vec![
        ("fakts", map(fakts)),
        (
            "app",
            map(vec![
                ("identifier", s(ENGINE_APP_IDENTIFIER)),
                ("version", s(ENGINE_APP_VERSION)),
                ("instance_id", s(answers.identifier.trim())),
            ]),
        ),
        ("docker", map(vec![("socket", s(DOCKER_SOCKET))])),
    ])
}

/// What an engine says about itself when it asks to be let in.
fn build_engine_manifest(answers: &EngineAnswers, node_id: Option<String>) -> AppManifest {
    AppManifest {
        identifier: ENGINE_APP_IDENTIFIER.to_string(),
        version: ENGINE_APP_VERSION.to_string(),
        description: answers
            .description
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(str::to_string),
        logo: None,
        scopes: ENGINE_SCOPES.iter().map(|s| s.to_string()).collect(),
        // One engine per machine per name: the identifier the wizard collects is what
        // tells two engines on the same coordination server apart.
        instance_id: answers.identifier.trim().to_string(),
        node_id,
    }
}

/// Check Docker → authorize → write → register → start, in that order.
///
/// Authorization comes before anything is written for the same reason it does for a hub:
/// what comes back is what the container is configured with. An engine is an *app*, so it
/// goes through the app device-code flow — a manifest describing itself, a code somebody
/// accepts in a browser, and an OAuth2 client in return — rather than the hub manifest
/// endpoint. The `client_id` and `refresh_token` from that grant are written into
/// `configs/deployer.yaml` and mounted into the container read-only.
pub async fn create_engine(
    answers: &EngineAnswers,
    cancel: &tokio_util::sync::CancellationToken,
    on: &(dyn Fn(CreateEvent) + Sync),
) -> Result<CreatedEngine, CreateError> {
    on(CreateEvent::CheckingDocker);
    let probe = docker::probe().await;
    if !probe.is_ready() {
        return Err(CreateError::Docker(crate::create::describe_docker(&probe)));
    }

    let dir = PathBuf::from(&answers.dir);
    std::fs::create_dir_all(&dir)?;

    let mut store = registry::load();
    let verdict = registry::inspect_folder(&store, &dir);
    if !verdict.can_create() {
        return Err(CreateError::Folder(verdict.describe()));
    }

    on(CreateEvent::Building);

    // --- authorize ----------------------------------------------------------
    let manifest = build_engine_manifest(answers, Some(store.device_id.clone()));
    let grant = app::start(&answers.coord_server, &manifest).await?;
    on(CreateEvent::Staged {
        user_code: grant.user_code.clone(),
        verification_uri_complete: grant.verification_uri_complete.clone(),
        expires_in: grant.expires_in,
    });

    let granted = app::wait_for_app(&grant, cancel, &|progress| {
        on(CreateEvent::Waiting {
            polls: progress.polls,
            seconds_left: progress.seconds_left,
        })
    })
    .await?;

    // The two values the container lives on. A grant that carries no refresh token
    // produces an engine that can never renew its access — and, because it is written
    // and started anyway, one that looks created and simply does not work. Refused here
    // instead, with what the server did send, since that names the key it used.
    if granted
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .is_none()
    {
        return Err(CreateError::Authorization(
            crate::connect::authorize::HubAuthorizationError::NoRefreshToken {
                fields: granted.declared_fields(),
            },
        ));
    }

    on(CreateEvent::Granted { mesh_key: false });

    // --- write --------------------------------------------------------------
    let files = generate_engine_files(answers, &granted);
    for name in files.keys() {
        on(CreateEvent::Writing { file: name.clone() });
    }
    write_generated_files(&dir, &files)?;

    let record = registry::register_kind(
        &mut store,
        "engine",
        &answers.name,
        &dir.to_string_lossy(),
        Some(answers.coord_server.trim().to_string()),
        Some(answers.identifier.trim().to_string()),
        now_rfc3339(),
    );
    let _ = registry::save(&store);

    if answers.start {
        on(CreateEvent::Starting);
        start(&dir, on)?;
    }

    on(CreateEvent::Done {
        path: dir.to_string_lossy().to_string(),
    });

    Ok(CreatedEngine { path: dir, record })
}

fn start(dir: &Path, on: &(dyn Fn(CreateEvent) + Sync)) -> Result<(), CreateError> {
    let output = std::process::Command::new("docker")
        .args(crate::compose::up())
        .current_dir(dir)
        .output()?;

    for line in String::from_utf8_lossy(&output.stderr).lines() {
        on(CreateEvent::Log {
            line: line.to_string(),
        });
    }
    if output.status.success() {
        Ok(())
    } else {
        Err(CreateError::StartFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answers() -> EngineAnswers {
        EngineAnswers {
            dir: "/tmp/engine".into(),
            name: "MyEngine".into(),
            coord_server: "go.arkitekt.live".into(),
            identifier: "my-engine".into(),
            description: None,
            start: false,
        }
    }

    /// The socket is the whole feature: an engine without it can start no plugin.
    #[test]
    fn the_engine_gets_the_docker_socket() {
        let compose = build_engine_compose();
        let volumes = compose["services"][DEPLOYER_SERVICE]["volumes"]
            .as_sequence()
            .expect("volumes");
        assert_eq!(
            volumes[0].as_str(),
            Some("/var/run/docker.sock:/var/run/docker.sock")
        );
        assert_eq!(
            compose["services"][DEPLOYER_SERVICE]["image"].as_str(),
            Some(DEPLOYER_IMAGE)
        );
    }

    fn granted() -> AppEnvelope {
        AppEnvelope {
            token_type: "bearer".into(),
            access_token: "expires-within-the-hour".into(),
            refresh_token: Some("the-long-lived-one".into()),
            expires_in: Some(3600),
            scope: None,
            client_id: "engine-client-id".into(),
            client_secret: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn an_engine_is_a_compose_file_and_its_identity() {
        let files = generate_engine_files(&answers(), &granted());
        let names: Vec<&str> = files.keys().map(String::as_str).collect();
        assert_eq!(names, ["configs/deployer.yaml", "docker-compose.yaml"]);
    }

    /// The guard that keeps a useless engine off the disk.
    #[test]
    fn a_grant_without_a_refresh_token_names_what_did_come_back() {
        let mut envelope = granted();
        envelope.refresh_token = None;
        let fields = envelope.declared_fields();
        assert!(fields.contains("access_token"), "got {fields}");
        assert!(fields.contains("client_id"), "got {fields}");
        assert!(!fields.contains("refresh_token"), "got {fields}");
    }

    /// The point of the whole flow: what the container is handed is the client and the
    /// refresh token, and *not* the access token, which is stale within the hour.
    #[test]
    fn the_container_is_handed_the_client_and_the_refresh_token() {
        let files = generate_engine_files(&answers(), &granted());
        let config = &files["configs/deployer.yaml"];

        assert!(config.contains("client_id: engine-client-id"));
        assert!(config.contains("refresh_token: the-long-lived-one"));
        assert!(
            !config.contains("expires-within-the-hour"),
            "the access token must not be written down"
        );

        // And it is mounted, or writing it would have been pointless.
        let compose = build_engine_compose();
        let volumes = compose["services"][DEPLOYER_SERVICE]["volumes"]
            .as_sequence()
            .expect("volumes");
        assert_eq!(
            volumes[1].as_str(),
            Some("./configs/deployer.yaml:/workspace/config.yaml:ro")
        );
    }
}
