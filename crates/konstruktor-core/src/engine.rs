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
//! advertise — and folding them together would mean a wizard mostly made of questions
//! that do not apply. What they share is how they reach a hub: an engine on this machine
//! can join a hub's network ([`attach`]), and one anywhere can join the mesh, where its
//! plugins reach the hub it is bound to over the tailnet.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_norway::Value;

use crate::config::mesh::{build_mesh_block, mesh_hostname, MeshBlock, MeshOptions, MESH_STATE_DIR};
use crate::connect::app::{self, AppEnvelope, AppManifest};
use crate::create::{now_rfc3339, CreateError, CreateEvent};
use crate::docker;
use crate::engine_probe::EngineKind;
use crate::generate::service::{list, map, s};
use crate::generate::write::write_generated_files;
use crate::generate::GeneratedFiles;
use crate::registry;

/// The image the engine runs. Pinned by tag, like every other image this app writes.
pub const DEPLOYER_IMAGE: &str = "jhnnsrs/deployer:next";

/// The compose service the engine runs under, and the name its config file is keyed by.
pub const DEPLOYER_SERVICE: &str = "deployer";

/// Where the daemon is reached, and where an engine's whole point lies: without the
/// socket it cannot start a single plugin. Which socket that is depends on the engine —
/// Podman has no `docker.sock` — so it is read off the discovered engine rather than
/// hardcoded, and passed down explicitly so the generators stay pure.

/// The engine's own config, in the deployment folder and inside the container.
pub const CONFIG_FILE: &str = "configs/deployer.yaml";
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
    /// A hub folder on this machine to attach to: the engine joins that hub's network, and
    /// so do the plugins it starts, which then reach the hub as `gateway` without leaving
    /// Docker. See [`attach`].
    #[serde(default)]
    pub hub: Option<String>,
    /// Put the engine on the mesh: a key is asked for with the grant, and the engine and
    /// every plugin it starts share a tailscale sidecar's network, so they reach the hub
    /// the engine is bound to over the tailnet from any machine. Combines with `hub`.
    #[serde(default)]
    pub mesh: bool,
}

/// What the engine's compose file is built from.
#[derive(Debug, Clone)]
pub struct EngineCompose<'a> {
    pub engine: EngineKind,
    /// The host side of the socket mount, when it is not the engine's usual one — a
    /// rootless daemon on Linux serves from under `$XDG_RUNTIME_DIR`, not `/var/run`.
    pub host_socket: Option<&'a str>,
    /// Where plugins are told to find the platform: the coordination server's fakts.
    pub coord_server: &'a str,
    /// The engine's own compose project, whose default network plugins join when the
    /// engine is not attached to a hub.
    pub project: &'a str,
    /// The attached hub's internal network, when there is one.
    pub hub_network: Option<&'a str>,
    /// The engine's own mesh membership, when it has one.
    pub mesh: Option<&'a MeshBlock>,
}

/// The compose key the attached hub's network goes under.
const HUB_NETWORK_KEY: &str = "hub";

/// Where an engine on the mesh keeps its mesh block. Engines have no profile, and the
/// compose file is rewritten on every attach, so the block needs a file of its own.
pub const MESH_FILE: &str = "configs/mesh.yaml";

pub struct CreatedEngine {
    pub path: PathBuf,
    pub record: registry::DeploymentRecord,
}

/// The `docker-compose.yaml` an engine deployment consists of.
///
/// One service. `restart: unless-stopped` rather than the hub's `on-failure` policy: an
/// engine is a long-running agent whose job is to be there when somebody installs a
/// plugin, and a machine that reboots should come back with it running.
///
/// The deployer starts every plugin container on `ARKITEKT_NETWORK` and points it at
/// `ARKITEKT_GATEWAY` (`arkitektio/deployer`, `app.py`). Left unset they fall back to a
/// network that does not exist here and to one particular coordination server, so both
/// are always written: the attached hub's network, or the engine's own default one, and
/// the coordination server this engine was authorized against.
///
/// On the mesh, a tailscale sidecar holds the network namespace, and the deployer lives
/// in it the way a hub's gateway does. Only what shares that namespace is on the tailnet
/// — a plugin merely beside it on a bridge network is not — so the deployer is told to
/// start plugins in it too, with `ARKITEKT_NETWORK_MODE`. A deployer from before that
/// variable still reads `ARKITEKT_NETWORK` and starts them beside it: local, but running.
pub fn build_engine_compose(compose: &EngineCompose<'_>) -> Value {
    let socket = compose.engine.container_socket();
    let host = compose.host_socket.unwrap_or(socket);
    let plugin_network = match compose.hub_network {
        Some(network) => network.to_string(),
        None => format!("{}_default", compose.project),
    };

    let mut environment = vec![
        (
            "ARKITEKT_GATEWAY",
            s(&crate::connect::wellknown::base_url(compose.coord_server)),
        ),
        ("ARKITEKT_NETWORK", s(&plugin_network)),
    ];
    if let Some(mesh) = compose.mesh {
        environment.push((
            "ARKITEKT_NETWORK_MODE",
            s(&format!("container:{}", sidecar_container(compose.project, mesh))),
        ));
    }

    let mut deployer = vec![
        ("image", s(DEPLOYER_IMAGE)),
        ("restart", s("unless-stopped")),
        ("stop_grace_period", s("2s")),
        ("environment", map(environment)),
        (
            "volumes",
            list(vec![
                // The socket is bind-mounted, not proxied: the deployer starts sibling
                // containers on this machine's daemon rather than running a daemon of its
                // own. Inside the container it stays at the classic path, which is where
                // the deployer looks.
                s(&format!("{host}:{socket}")),
                // Its identity, read-only. This is the file the device-code flow
                // produced: a client id and a refresh token, which is all the engine
                // needs to get itself an access token from then on.
                s(&format!("./{CONFIG_FILE}:{CONFIG_MOUNT}:ro")),
            ]),
        ),
    ];

    // Whoever holds the network namespace joins the networks: the sidecar on the mesh,
    // the deployer otherwise. `network_mode: service:` forbids `networks` on a member.
    let joins = compose
        .hub_network
        .map(|_| ("networks", list(vec![s("default"), s(HUB_NETWORK_KEY)])));

    let mut services = vec![];
    let mut document = vec![];
    match compose.mesh {
        Some(mesh) => {
            deployer.push(("network_mode", s(&format!("service:{}", mesh.host))));
            deployer.push(("depends_on", list(vec![s(&mesh.host)])));
            services.push((mesh.host.as_str(), sidecar_service(mesh, joins)));
            document.push((
                "volumes",
                map(vec![(mesh.volume_name.as_str(), map(vec![]))]),
            ));
        }
        None => deployer.extend(joins),
    }
    services.insert(0, (DEPLOYER_SERVICE, map(deployer)));

    if let Some(network) = compose.hub_network {
        // The hub's network is the hub's: declared external, so this project joins it
        // without ever creating or removing it — a `down` here must not take it away from
        // a running hub.
        document.push((
            "networks",
            map(vec![(
                HUB_NETWORK_KEY,
                map(vec![("external", Value::Bool(true)), ("name", s(network))]),
            )]),
        ));
    }
    document.insert(0, ("services", map(services)));
    map(document)
}

/// The engine's tailscale sidecar. Unlike a hub's it accepts the tailnet's DNS: the
/// plugins sharing its namespace are handed hub aliases that are MagicDNS names, and
/// they resolve through the `resolv.conf` tailscale writes here, which Docker gives every
/// container in the namespace. Names outside the tailnet are forwarded on, Docker's own
/// included.
fn sidecar_service(mesh: &MeshBlock, networks: Option<(&'static str, Value)>) -> Value {
    let mut environment: Vec<(&str, Value)> = mesh
        .sidecar_environment()
        .into_iter()
        .map(|(key, value)| (key, s(&value)))
        .collect();
    environment.push(("TS_ACCEPT_DNS", s("true")));

    let mut sidecar = vec![
        ("image", s(&mesh.image)),
        ("hostname", s(&mesh.hostname)),
        ("environment", map(environment)),
        (
            "volumes",
            list(vec![
                s(&format!("{}:{}", mesh.volume_name, MESH_STATE_DIR)),
                s("/dev/net/tun:/dev/net/tun"),
            ]),
        ),
        ("cap_add", list(vec![s("net_admin"), s("sys_module")])),
        // Comes back with the engine: plugins live in this namespace.
        ("restart", s("unless-stopped")),
    ];
    sidecar.extend(networks);
    map(sidecar)
}

/// The sidecar's container, as compose names it: plugins join its namespace by name.
fn sidecar_container(project: &str, mesh: &MeshBlock) -> String {
    format!("{project}-{}-1", mesh.host)
}

/// The mesh block of the engine in `dir`, when it is on the mesh.
pub fn engine_mesh(dir: &Path) -> Option<MeshBlock> {
    let text = std::fs::read_to_string(dir.join(MESH_FILE)).ok()?;
    serde_norway::from_str::<MeshBlock>(&text)
        .ok()
        .filter(|mesh| mesh.enabled)
}

/// Every file an engine deployment consists of, keyed by its path in the folder.
pub fn generate_engine_files(
    answers: &EngineAnswers,
    granted: &AppEnvelope,
    compose: &EngineCompose<'_>,
) -> GeneratedFiles {
    let engine = compose.engine;
    let mut files = GeneratedFiles::new();
    files.insert(
        "docker-compose.yaml".to_string(),
        crate::generate::dump(&build_engine_compose(compose)),
    );
    files.insert(
        CONFIG_FILE.to_string(),
        crate::generate::dump(&build_engine_config(answers, granted, engine)),
    );
    if let Some(mesh) = compose.mesh {
        files.insert(
            MESH_FILE.to_string(),
            serde_norway::to_string(mesh).expect("a mesh block serializes"),
        );
    }
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
fn build_engine_config(
    answers: &EngineAnswers,
    granted: &AppEnvelope,
    engine: EngineKind,
) -> Value {
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
        (
            "docker",
            map(vec![("socket", s(engine.container_socket()))]),
        ),
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

    // Before anything is asked of the coordination server: a hub that is not there is a
    // mistake in the answers, and finding out after somebody accepted a code is late.
    let hub_network = match answers.hub.as_deref().map(str::trim).filter(|h| !h.is_empty()) {
        Some(hub) => Some(hub_network_of(Path::new(hub))?),
        None => None,
    };

    on(CreateEvent::Building);

    // --- authorize ----------------------------------------------------------
    let manifest = build_engine_manifest(answers, Some(store.device_id.clone()));
    let grant = app::start(&answers.coord_server, &manifest, answers.mesh).await?;
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
        return Err(CreateError::AppAuthorization(
            app::AppAuthorizationError::NoRefreshToken {
                fields: granted.declared_fields(),
            },
        ));
    }

    on(CreateEvent::Granted {
        mesh_key: granted.mesh_key().is_some(),
    });

    // Asked for and not granted: written anyway, this engine would be one that cannot
    // do the one thing it was created for. Nothing is on disk yet, so stop here.
    let mesh = if answers.mesh {
        let (key, coord_url) = granted.mesh_key().ok_or(CreateError::EngineNoMeshKey)?;
        Some(build_mesh_block(&MeshOptions {
            hostname: mesh_hostname(&answers.identifier),
            auth_key: key.to_string(),
            coord_url: coord_url.map(str::to_string),
            login: granted.login(),
        }))
    } else {
        None
    };

    // --- write --------------------------------------------------------------
    let engine = crate::engine_probe::engine();
    let host_socket = docker::host_socket(&engine).await;
    let project = crate::compose::project_name(&dir.to_string_lossy());
    let files = generate_engine_files(
        answers,
        &granted,
        &EngineCompose {
            engine: engine.kind,
            host_socket: host_socket.as_deref(),
            coord_server: &answers.coord_server,
            project: &project,
            hub_network: hub_network.as_deref(),
            mesh: mesh.as_ref(),
        },
    );
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
        // The same start every deployment gets — including the refusal to start an engine
        // attached to a hub whose network does not exist yet.
        let log = |line: crate::compose::ComposeLine| on(CreateEvent::Log { line: line.line });
        if let Err(error) = crate::start::start(&dir, &log).await {
            on(CreateEvent::Log {
                line: error.to_string(),
            });
            return Err(CreateError::StartFailed);
        }
    }

    on(CreateEvent::Done {
        path: dir.to_string_lossy().to_string(),
    });

    Ok(CreatedEngine { path: dir, record })
}

/// The internal network of the hub in `dir` — what an engine attaches to.
fn hub_network_of(dir: &Path) -> Result<String, CreateError> {
    crate::profile::read_profile(dir)
        .map(|profile| profile.config.internal_network)
        .map_err(|_| {
            CreateError::Answers(format!(
                "{} does not hold a hub to attach the engine to",
                dir.display()
            ))
        })
}

/// The coordination server an engine was authorized against, out of its own config.
fn engine_coord_server(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(CONFIG_FILE)).ok()?;
    let config: Value = serde_norway::from_str(&text).ok()?;
    config
        .get("fakts")?
        .get("endpoint_url")?
        .as_str()
        .map(str::to_string)
}

/// Attaches the engine in `dir` to the hub in `hub` — or, with `None`, detaches it.
///
/// Only the compose file changes: the engine's identity is not touched, and nothing is
/// asked of the coordination server. The engine has to be recreated to pick it up, which
/// is the caller's — a running engine keeps its old network until then.
pub async fn attach(dir: &Path, hub: Option<&Path>) -> Result<(), CreateError> {
    if !dir.join(CONFIG_FILE).exists() {
        return Err(CreateError::Folder(format!(
            "{} does not hold a plugin engine",
            dir.display()
        )));
    }
    let hub_network = hub.map(hub_network_of).transpose()?;
    let coord_server = engine_coord_server(dir).ok_or_else(|| {
        CreateError::Folder(format!(
            "{} names no coordination server — authorize the engine again",
            dir.join(CONFIG_FILE).display()
        ))
    })?;

    let engine = crate::engine_probe::engine();
    let host_socket = docker::host_socket(&engine).await;
    let project = crate::compose::project_name(&dir.to_string_lossy());
    // On the mesh stays on the mesh: attaching is about which hub network it also joins.
    let mesh = engine_mesh(dir);
    let compose = build_engine_compose(&EngineCompose {
        engine: engine.kind,
        host_socket: host_socket.as_deref(),
        coord_server: &coord_server,
        project: &project,
        hub_network: hub_network.as_deref(),
        mesh: mesh.as_ref(),
    });
    let mut files = GeneratedFiles::new();
    files.insert(
        "docker-compose.yaml".to_string(),
        crate::generate::dump(&compose),
    );
    write_generated_files(dir, &files)?;
    Ok(())
}

/// The hub an engine is attached to, as the registry knows it — found by the network its
/// compose file joins, which is the record: nothing else is kept.
pub fn attached_hub(dir: &Path) -> Option<registry::DeploymentRecord> {
    let network = attached_network(dir)?;
    registry::load().deployments.into_iter().find(|record| {
        record.kind == "hub"
            && crate::profile::read_profile(Path::new(&record.path))
                .is_ok_and(|p| p.config.internal_network == network)
    })
}

/// The external network an engine's compose file joins, if any.
pub fn attached_network(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("docker-compose.yaml")).ok()?;
    let doc: Value = serde_norway::from_str(&text).ok()?;
    doc.get("networks")?
        .get(HUB_NETWORK_KEY)?
        .get("name")?
        .as_str()
        .map(str::to_string)
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
            hub: None,
            mesh: false,
        }
    }

    fn inputs(hub_network: Option<&str>) -> EngineCompose<'_> {
        EngineCompose {
            engine: EngineKind::Docker,
            host_socket: None,
            coord_server: "go.arkitekt.live",
            project: "myengine",
            hub_network,
            mesh: None,
        }
    }

    fn a_mesh() -> MeshBlock {
        build_mesh_block(&MeshOptions {
            hostname: "my-engine".into(),
            auth_key: "tskey-engine".into(),
            coord_url: Some("https://mesh.example.org".into()),
            login: Some("2-9-48".into()),
        })
    }

    /// On the mesh, the sidecar holds the namespace, the deployer lives in it, and the
    /// deployer is told to start plugins in it too — the only way they are on the tailnet.
    #[test]
    fn an_engine_on_the_mesh_runs_everything_in_the_sidecars_namespace() {
        let mesh = a_mesh();
        let mut compose_inputs = inputs(None);
        compose_inputs.mesh = Some(&mesh);
        let compose = build_engine_compose(&compose_inputs);

        let deployer = &compose["services"][DEPLOYER_SERVICE];
        assert_eq!(deployer["network_mode"].as_str(), Some("service:tailscale"));
        assert!(deployer.get("networks").is_none());
        assert_eq!(
            deployer["environment"]["ARKITEKT_NETWORK_MODE"].as_str(),
            Some("container:myengine-tailscale-1")
        );
        // Still written, for a deployer that does not know the mode yet.
        assert_eq!(
            deployer["environment"]["ARKITEKT_NETWORK"].as_str(),
            Some("myengine_default")
        );

        let sidecar = &compose["services"]["tailscale"];
        let env = &sidecar["environment"];
        assert_eq!(env["TS_AUTHKEY"].as_str(), Some("tskey-engine"));
        assert_eq!(env["TS_STATE_DIR"].as_str(), Some("/var/lib/tailscale/2-9-48"));
        assert_eq!(env["TS_ACCEPT_DNS"].as_str(), Some("true"));
        assert_eq!(
            env["TS_EXTRA_ARGS"].as_str(),
            Some("--login-server=https://mesh.example.org")
        );
        // The server tags the node by its key; nothing here may advertise one.
        let serialized = serde_norway::to_string(sidecar).unwrap();
        assert!(!serialized.contains("advertise-tags"), "{serialized}");
        assert!(compose["volumes"].get("tailscale_state").is_some());
    }

    /// On the mesh and attached: the sidecar, as the namespace's owner, joins the hub's
    /// network, so plugins reach the local hub as `gateway` and remote ones over the mesh.
    #[test]
    fn on_the_mesh_and_attached_the_sidecar_joins_the_hub_network() {
        let mesh = a_mesh();
        let mut compose_inputs = inputs(Some("young-dream"));
        compose_inputs.mesh = Some(&mesh);
        let compose = build_engine_compose(&compose_inputs);

        assert_eq!(
            compose["services"]["tailscale"]["networks"],
            serde_norway::from_str::<Value>("[default, hub]").unwrap()
        );
        assert!(compose["services"][DEPLOYER_SERVICE].get("networks").is_none());
        assert_eq!(compose["networks"]["hub"]["name"].as_str(), Some("young-dream"));
    }

    /// The block is kept beside the config, and an attach — which rewrites the compose
    /// file — keeps the engine on the mesh.
    #[tokio::test]
    async fn the_mesh_survives_an_attach() {
        let root = std::env::temp_dir().join(format!("konstruktor-mesh-{}", std::process::id()));
        let (engine_dir, hub_dir) = (root.join("engine"), root.join("hub"));
        std::fs::create_dir_all(&engine_dir).unwrap();
        std::fs::create_dir_all(&hub_dir).unwrap();
        let hub = crate::config::hub::build_hub_config(&Default::default());
        crate::profile::write_profile(&hub_dir, &crate::profile::hub_profile(hub)).unwrap();

        let mesh = a_mesh();
        let mut compose_inputs = inputs(None);
        compose_inputs.mesh = Some(&mesh);
        write_generated_files(
            &engine_dir,
            &generate_engine_files(&answers(), &granted(), &compose_inputs),
        )
        .unwrap();
        assert_eq!(engine_mesh(&engine_dir), Some(mesh));

        attach(&engine_dir, Some(&hub_dir)).await.expect("attached");
        let text = std::fs::read_to_string(engine_dir.join("docker-compose.yaml")).unwrap();
        let compose: Value = serde_norway::from_str(&text).unwrap();
        assert_eq!(
            compose["services"][DEPLOYER_SERVICE]["network_mode"].as_str(),
            Some("service:tailscale")
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// The socket is the whole feature: an engine without it can start no plugin.
    #[test]
    fn the_engine_gets_the_docker_socket() {
        let compose = build_engine_compose(&inputs(None));
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
            self_: None,
            mesh: None,
            extra: Default::default(),
        }
    }

    /// Left unset, the deployer starts plugins on a network that does not exist here and
    /// points them at one particular coordination server.
    #[test]
    fn plugins_are_told_where_to_run_and_whom_to_ask() {
        let compose = build_engine_compose(&inputs(None));
        let env = &compose["services"][DEPLOYER_SERVICE]["environment"];
        assert_eq!(env["ARKITEKT_GATEWAY"].as_str(), Some("https://go.arkitekt.live"));
        // Not attached: the engine's own default network, which its `up` creates.
        assert_eq!(env["ARKITEKT_NETWORK"].as_str(), Some("myengine_default"));
        assert!(compose.get("networks").is_none());
        assert!(compose["services"][DEPLOYER_SERVICE].get("networks").is_none());
    }

    /// Attached: the engine and every plugin it starts join the hub's network — declared
    /// external, so this project never creates or removes the hub's own.
    #[test]
    fn an_attached_engine_joins_the_hubs_network() {
        let compose = build_engine_compose(&inputs(Some("young-dream")));
        let deployer = &compose["services"][DEPLOYER_SERVICE];
        assert_eq!(deployer["environment"]["ARKITEKT_NETWORK"].as_str(), Some("young-dream"));
        assert_eq!(
            deployer["networks"],
            serde_norway::from_str::<Value>("[default, hub]").unwrap()
        );
        assert_eq!(compose["networks"]["hub"]["external"].as_bool(), Some(true));
        assert_eq!(compose["networks"]["hub"]["name"].as_str(), Some("young-dream"));
    }

    /// Attach, read back, detach — the compose file is the whole record.
    #[tokio::test]
    async fn attaching_and_detaching_round_trip() {
        let root = std::env::temp_dir().join(format!("konstruktor-attach-{}", std::process::id()));
        let (engine_dir, hub_dir) = (root.join("engine"), root.join("hub"));
        std::fs::create_dir_all(&engine_dir).unwrap();
        std::fs::create_dir_all(&hub_dir).unwrap();

        let hub = crate::config::hub::build_hub_config(&Default::default());
        crate::profile::write_profile(&hub_dir, &crate::profile::hub_profile(hub.clone()))
            .unwrap();
        write_generated_files(
            &engine_dir,
            &generate_engine_files(&answers(), &granted(), &inputs(None)),
        )
        .unwrap();

        attach(&engine_dir, Some(&hub_dir)).await.expect("attached");
        assert_eq!(attached_network(&engine_dir), Some(hub.internal_network.clone()));

        attach(&engine_dir, None).await.expect("detached");
        assert_eq!(attached_network(&engine_dir), None);

        // A folder with no hub in it is refused, not attached to nothing.
        assert!(attach(&engine_dir, Some(&root)).await.is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_engine_is_a_compose_file_and_its_identity() {
        let files = generate_engine_files(&answers(), &granted(), &inputs(None));
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
        let files = generate_engine_files(&answers(), &granted(), &inputs(None));
        let config = &files["configs/deployer.yaml"];

        assert!(config.contains("client_id: engine-client-id"));
        assert!(config.contains("refresh_token: the-long-lived-one"));
        assert!(
            !config.contains("expires-within-the-hour"),
            "the access token must not be written down"
        );

        // And it is mounted, or writing it would have been pointless.
        let compose = build_engine_compose(&inputs(None));
        let volumes = compose["services"][DEPLOYER_SERVICE]["volumes"]
            .as_sequence()
            .expect("volumes");
        assert_eq!(
            volumes[1].as_str(),
            Some("./configs/deployer.yaml:/workspace/config.yaml:ro")
        );
    }
}
