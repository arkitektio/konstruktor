use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::catalog::ServiceId;
use crate::config::hub::{
    build_hub_config, trusted_origins, HubConfig, HubConfigOptions, LokOptions, RunsFrom,
    ServiceOptions, StorageMode, LOCAL_COORD_SERVER,
};
use crate::config::mesh::{build_mesh_block, mesh_hostname, MeshOptions};
use crate::connect::authorize::{self, HubAuthorizationError};
use crate::connect::manifest::{build_hub_request, AdvertisedHost, HubManifestOptions};
use crate::credentials::{write_credentials, HubCredentials};
use crate::generate::generate_hub_files;
use crate::profile::{hub_profile, write_profile};
use crate::{compose, docker, git, registry};

/// Creating a hub, end to end — and the single place that orchestration lives.
///
/// Both front ends call this: the desktop app wraps it with a Tauri `Channel`, the CLI
/// with a printer. That is what keeps "the CLI does the same thing as the GUI" a fact
/// rather than a promise.

/// Where the mesh key comes from, if anywhere.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeshMode {
    None,
    /// Ask the coordination server to mint one while it accepts the hub. The default —
    /// see `crate::defaults::MESH_MODE`.
    #[default]
    Coordination,
    /// Use a key the user already holds.
    Manual,
}

/// Everything a front end has to collect. Deliberately flat and serde-friendly: the
/// desktop app hands this straight across the IPC boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubAnswers {
    pub dir: String,
    pub name: String,
    pub coord_server: String,
    pub identifier: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "local")]
    pub rekuest_server: String,
    pub services: Vec<ServiceId>,
    #[serde(default = "http_port")]
    pub http_port: u16,
    #[serde(default = "https_port")]
    pub https_port: u16,
    #[serde(default)]
    pub ssl: bool,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default = "admin")]
    pub global_admin: String,
    #[serde(default)]
    pub global_admin_password: Option<String>,
    #[serde(default)]
    pub global_description: Option<String>,
    pub hosts: Vec<AdvertisedHost>,
    /// Of `hosts`, the ones an external probe reached. Empty unless somebody checked.
    #[serde(default)]
    pub reachable_hosts: Vec<String>,
    #[serde(default)]
    pub mesh_mode: MeshMode,
    #[serde(default)]
    pub mesh_auth_key: Option<String>,
    #[serde(default)]
    pub mesh_coord_url: Option<String>,
    /// Reach the hub over the mesh and nothing else: no port is published on the host,
    /// `hosts` is ignored, and the manifest advertises the tailnet node and the gateway's
    /// name on the docker network. Needs a `mesh_mode` other than `none`.
    #[serde(default)]
    pub mesh_only: bool,
    /// Run `docker compose up -d` once everything is written.
    #[serde(default = "yes")]
    pub start: bool,
    /// Make this a *dev hub*: check every enabled service's repository out into
    /// `mounts/<service>` and mount it over the image's workspace, so the containers run
    /// the source on this machine rather than the code baked into the image. Needs git.
    #[serde(default)]
    pub dev_hub: bool,
    /// The branch to check out, for a dev hub. Left out, each repository's own default
    /// branch is used — they do not all agree on what it is called. A service that names
    /// its own branch in `service_options` wins over this.
    #[serde(default)]
    pub dev_branch: Option<String>,
    /// What was asked of individual services: whether each runs from a checkout of its
    /// source, and on which branch. The wizard asks this per service; `dev_hub` is the
    /// CLI's "all of them" and the two are a union.
    #[serde(default)]
    pub service_options: BTreeMap<ServiceId, ServiceOptions>,
    /// Where the database and object storage keep their data: the engine's own volumes
    /// (the default, and the fast one) or bind mounts in the deployment folder.
    #[serde(default)]
    pub storage: StorageMode,
    /// Images to run instead of the ones a new hub is seeded with, by compose service —
    /// `rekuest` → `jhnnsrs/rekuest:1.2.3`. For pinning what a test suite runs against,
    /// so an upstream `latest` that moved cannot fail a build it has nothing to do with.
    #[serde(default)]
    pub images: BTreeMap<String, String>,
    /// The same, for the services this hub happens to run: an entry naming one it does
    /// not is skipped rather than refused. This is what `$KONSTRUKTOR_IMAGES` fills —
    /// set once for a whole test run, in which different hubs run different services.
    #[serde(default)]
    pub default_images: BTreeMap<String, String>,
    /// What images said of themselves when they were asked which service they are
    /// (`--service-image`), by image: they are not asked a second time.
    #[serde(skip)]
    pub described: BTreeMap<String, crate::contract::Description>,
    /// What the coordination server is seeded with, on a self-contained hub
    /// (`coord_server: local`). Ignored on any other.
    #[serde(default)]
    pub seed: SeedAnswers,
}

/// The account, the organization and the redeem tokens a self-contained hub's own
/// coordination server starts out with. Everything has a default, so a hub created with
/// none of it said is still one an app can connect to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedAnswers {
    #[serde(default = "demo")]
    pub organization: String,
    #[serde(default = "demo")]
    pub user: String,
    /// Left out, a strong one is generated.
    #[serde(default)]
    pub user_password: Option<String>,
    /// Tokens to provision as given — for a caller that has to know them beforehand.
    #[serde(default)]
    pub redeem_tokens: Vec<String>,
    /// How many more to mint. One app redeems one token, so this is how many apps can
    /// connect unattended.
    #[serde(default = "one")]
    pub generated_redeem_tokens: usize,
}

impl Default for SeedAnswers {
    fn default() -> Self {
        Self {
            organization: demo(),
            user: demo(),
            user_password: None,
            redeem_tokens: Vec::new(),
            generated_redeem_tokens: one(),
        }
    }
}

fn demo() -> String {
    "demo".into()
}
fn one() -> usize {
    1
}

fn local() -> String {
    "local".into()
}
fn admin() -> String {
    "admin".into()
}
fn http_port() -> u16 {
    crate::defaults::HTTP_PORT
}
fn https_port() -> u16 {
    crate::defaults::HTTPS_PORT
}
fn yes() -> bool {
    crate::defaults::START
}

/// What the caller learns while a hub is being created.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum CreateEvent {
    CheckingDocker,
    Building,
    /// The device code is staged; show these and wait.
    Staged {
        user_code: String,
        verification_uri_complete: String,
        expires_in: u64,
    },
    Waiting {
        polls: u32,
        seconds_left: u64,
    },
    /// Accepted. `mesh_key` says whether a key came back with it — asking is not getting.
    Granted {
        mesh_key: bool,
    },
    Writing {
        file: String,
    },
    /// A dev hub's source is being checked out. One per service.
    Cloning {
        service: String,
        repo: String,
        branch: Option<String>,
    },
    Starting,
    Log {
        line: String,
    },
    Done {
        path: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum CreateError {
    #[error("Docker is not ready: {0}")]
    Docker(String),
    #[error("{0}")]
    Folder(String),
    /// The answers contradict each other, caught before anything is authorized.
    #[error("{0}")]
    Answers(String),
    /// A mesh-only hub was accepted without a mesh key, and has no other way in. Nothing
    /// was written.
    #[error(
        "The hub was accepted, but no mesh key was granted with it — and a mesh-only hub \
         has no other way in. Ask whoever accepts hubs to grant a mesh key, or create it \
         with addresses on this network."
    )]
    NoMeshKey,
    #[error(
        "The engine was accepted, but no mesh key was granted with it, so its plugins could \
         not reach hubs over the mesh. Nothing was written. Ask whoever accepted it to leave \
         mesh access allowed, check that the organization has a mesh — or create the engine \
         without one."
    )]
    EngineNoMeshKey,
    #[error(transparent)]
    Authorization(#[from] HubAuthorizationError),
    /// The app flow, which is how a plugin engine is claimed — separate from the hub's
    /// so its messages talk about an app rather than a hub.
    #[error(transparent)]
    AppAuthorization(#[from] crate::connect::app::AppAuthorizationError),
    #[error("Could not write the deployment: {0}")]
    Write(#[from] std::io::Error),
    #[error(
        "The deployment was written, but `docker compose up -d` failed. You can \
             retry with `konstruktor up`."
    )]
    StartFailed,
    /// The services were changed and the files rewritten, but bringing the stack to them
    /// failed.
    #[error(
        "The hub's services were changed and its files rewritten, but applying them failed: \
         {0}. Retry with `konstruktor hub services apply`."
    )]
    ApplyFailed(String),
    #[error(
        "The deployment is written and registered, but the source checkout failed: \
             {0}. Fix the checkout under `mounts/` and start it as usual."
    )]
    Clone(#[from] crate::git::CloneError),
}

pub struct CreatedHub {
    pub path: PathBuf,
    pub config: HubConfig,
    /// The grant, on a hub a coordination server accepted. A self-contained hub has none:
    /// it runs that server, and nobody was asked.
    pub credentials: Option<HubCredentials>,
    /// Whether a mesh key was actually granted.
    pub mesh_granted: bool,
}

/// Build → authorize → write → start, in that order and only that order.
///
/// The ordering is load-bearing. `build_hub_config` mints fresh secrets and a fresh
/// Ed25519 pair, so it runs exactly once: calling it again to fold in a mesh key would
/// describe a different hub than the one the coordination server accepted.
///
/// A self-contained hub (`coord_server: local`) skips the authorization, and with it the
/// only step that needs a person or a network: it runs the coordination server itself, so
/// there is nobody to ask. Everything else is the same path.
pub async fn create_hub(
    answers: &HubAnswers,
    cancel: &CancellationToken,
    on: &(dyn Fn(CreateEvent) + Sync),
) -> Result<CreatedHub, CreateError> {
    validate_identifier(&answers.identifier)?;
    validate_service_options(&answers.service_options)?;
    let self_contained = answers.coord_server.trim() == LOCAL_COORD_SERVER;
    if self_contained {
        validate_self_contained(answers)?;
    }
    if answers.mesh_only && answers.mesh_mode == MeshMode::None {
        return Err(CreateError::Answers(
            "A mesh-only hub needs a mesh — choose a way to join one, or advertise \
             addresses on this network instead."
                .into(),
        ));
    }
    if answers.mesh_mode == MeshMode::Manual
        && answers
            .mesh_auth_key
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
    {
        return Err(CreateError::Answers(
            "Joining a mesh with a key of your own needs the key.".into(),
        ));
    }

    on(CreateEvent::CheckingDocker);
    let probe = docker::probe().await;
    if !probe.is_ready() {
        return Err(CreateError::Docker(describe_docker(&probe)));
    }

    let dir = PathBuf::from(&answers.dir);
    std::fs::create_dir_all(&dir)?;

    let mut store = registry::load();
    let verdict = registry::inspect_folder(&store, &dir);
    if !verdict.can_create() {
        return Err(CreateError::Folder(verdict.describe()));
    }

    on(CreateEvent::Building);

    // A manually supplied key is known up front, so it goes into the profile that is
    // about to be authorized. A key from the coordination server does not exist yet.
    let manual_mesh = (answers.mesh_mode == MeshMode::Manual)
        .then(|| answers.mesh_auth_key.as_deref().map(str::trim))
        .flatten()
        .filter(|k| !k.is_empty())
        .map(|key| MeshOptions {
            hostname: mesh_hostname(&answers.identifier),
            auth_key: key.to_string(),
            coord_url: answers.mesh_coord_url.clone(),
            // A tailnet of the user's own: no login of the coordination server's to key by.
            login: None,
        });

    // Mesh-only publishes nothing on the host: the sidecar carries every request, so
    // there is no port to open, forward, or collide with something else on this machine.
    let (http_port, https_port) = if answers.mesh_only {
        (None, None)
    } else if self_contained && !answers.ssl {
        // One port: a hub that serves plain HTTP listens on nothing else, and a caller
        // that picked a free port for it should not have to pick a second one to waste.
        (Some(answers.http_port), None)
    } else {
        (Some(answers.http_port), Some(answers.https_port))
    };

    // Mesh-only advertises nothing on this machine's networks, whatever was picked.
    let hosts = if answers.mesh_only {
        Vec::new()
    } else {
        answers.hosts.clone()
    };

    let mut config = build_hub_config(&HubConfigOptions {
        device_id: store.device_id.clone(),
        coord_server: answers.coord_server.trim().to_string(),
        rekuest_server: answers.rekuest_server.clone(),
        services: Some(answers.services.clone()),
        http_port,
        https_port,
        ssl: answers.ssl,
        domain: answers.domain.clone(),
        global_admin: answers.global_admin.clone(),
        global_admin_password: answers.global_admin_password.clone(),
        global_description: answers.global_description.clone(),
        mesh: manual_mesh,
        lok: self_contained.then(|| seeded_lok(answers, &hosts)),
        dev_hub: answers.dev_hub,
        service_options: answers.service_options.clone(),
        storage: answers.storage,
        ..Default::default()
    });
    // Before the authorization and before anything is generated: the images are part of
    // what the hub *is*, and every later path reads them back out of the profile.
    let defaults = images_this_hub_runs(&config, &answers.default_images);
    apply_images(&mut config, &defaults)?;
    apply_images(&mut config, &answers.images)?;
    // A service outside the catalogue has no image but the one it is given.
    let imageless = config.imageless_services();
    if !imageless.is_empty() {
        return Err(CreateError::Answers(format!(
            "{} is not a service this konstruktor knows an image for. Name the image to run \
             it on: `--service-image IMAGE`, or `--image SERVICE=IMAGE`.",
            imageless
                .iter()
                .map(|host| format!("`{host}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    // The images are asked before anybody is asked to accept the hub, and before anything
    // is written: what each needs from the hub, how it is started and what it is
    // registered as are theirs to say, and both the manifest and the files are written
    // from that. An image that is not on this machine yet is fetched by the asking.
    on(CreateEvent::Log {
        line: "Asking the images what they are…".into(),
    });
    let said = crate::contract::describe_all(&config, &answers.described).await;
    let silent = crate::contract::undescribed(&config, &said);
    if !silent.is_empty() {
        return Err(CreateError::Answers(format!(
            "{} did not say what it is when run with no command: the image cannot be pulled, \
             or it is a release from before a service described itself. Nothing was written.",
            silent.join(", ")
        )));
    }
    crate::contract::acceptable(&config, &said).map_err(CreateError::Answers)?;
    // Now the hub can provide each with what it asked for: buckets, a key, its secrets.
    config.provide(&said);
    sources_are_known(&config)?;
    // LiveKit announces one of the addresses the hub is about to advertise.
    config.place_livekit(&host_names(&hosts));

    // --- authorize, unless there is nobody to ask ---------------------------
    let (mut config, credentials, mesh_granted) = if self_contained {
        (config, None, false)
    } else {
        let (config, credentials, mesh_granted) =
            authorize_new_hub(answers, config, &said, &store.device_id, &hosts, cancel, on).await?;
        (config, Some(credentials), mesh_granted)
    };

    // Now that the mesh name is known too: every address a browser may POST from.
    config.csrf_trusted_origins = Some(trusted_origins(&config, &host_names(&hosts)));

    // --- write --------------------------------------------------------------
    let identity = credentials
        .as_ref()
        .map(HubCredentials::issued_identity)
        .unwrap_or_default();
    let files = generate_hub_files(&config, &identity, &said);
    for name in files.keys() {
        on(CreateEvent::Writing { file: name.clone() });
    }

    write_profile(&dir, &hub_profile(config.clone()))
        .map_err(|e| CreateError::Write(std::io::Error::other(e.to_string())))?;
    if let Some(credentials) = &credentials {
        write_credentials(&dir, credentials)?;
    }
    crate::migrate::write_hub(&dir, &config, &files)?;
    // So the first start does not ask them again.
    crate::contract::remember(&dir, &config, &said)?;

    // --- register, so the desktop app sees it -------------------------------
    registry::register(
        &mut store,
        &answers.name,
        &dir.to_string_lossy(),
        Some(answers.coord_server.trim().to_string()),
        Some(answers.identifier.trim().to_string()),
        now_rfc3339(),
    );
    let _ = registry::save(&store);

    // --- check the source out, for the services that run from source --------
    //
    // Deliberately *after* the deployment is registered and before the stack is started.
    // Before `up`, because compose already declares the bind mounts and an empty
    // `mounts/<service>` would hand the container an empty workspace. After `register`,
    // because the device grant above is single-use: a clone that fails must leave a hub
    // the app can still see and the user can fix by hand, not an authorized folder
    // nothing knows about and no second run can reproduce.
    {
        let fallback = answers
            .dev_branch
            .as_deref()
            .map(str::trim)
            .filter(|b| !b.is_empty());
        let branch_of = |id: ServiceId| {
            answers
                .service_options
                .get(&id)
                .and_then(|asked| asked.branch.as_deref())
                .map(str::trim)
                .filter(|b| !b.is_empty())
                .or(fallback)
                .map(str::to_string)
        };
        check_sources_out(&dir, &config, &config.enabled_services(), &branch_of, on)?;
    }

    // --- start --------------------------------------------------------------
    if answers.start {
        on(CreateEvent::Starting);
        // The same start every front end runs — including the reporter fallback, so a
        // reporter image that cannot be pulled does not fail a hub that was just written.
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

    Ok(CreatedHub {
        path: dir,
        config,
        credentials,
        mesh_granted,
    })
}

/// The compose services this hub has an image to name for: everything in its stack, and
/// the services that are switched on and still waiting for theirs — one outside the
/// catalogue is not part of the stack until it is given the image it was named by.
fn compose_services(config: &HubConfig) -> Vec<String> {
    config
        .stack_images()
        .into_iter()
        .map(|(service, _)| service)
        .chain(config.imageless_services())
        .collect()
}

/// Of `images`, the entries naming a compose service this hub has.
fn images_this_hub_runs(
    config: &HubConfig,
    images: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let running = compose_services(config);
    images
        .iter()
        .filter(|(service, _)| running.contains(service))
        .map(|(service, image)| (service.clone(), image.clone()))
        .collect()
}

/// Points the named compose services at other images. A name the stack does not have is
/// refused rather than ignored: a pin that silently did nothing is worse than none.
fn apply_images(
    config: &mut HubConfig,
    images: &BTreeMap<String, String>,
) -> Result<(), CreateError> {
    let known = compose_services(config);
    for (service, image) in images {
        if !known.contains(service) {
            return Err(CreateError::Answers(format!(
                "This hub runs no service called `{service}` to give an image to — it \
                 runs {}.",
                known.join(", ")
            )));
        }
        if image.trim().is_empty() {
            return Err(CreateError::Answers(format!(
                "The image for `{service}` is empty."
            )));
        }
        config.set_service_image(service, image.trim());
    }
    Ok(())
}

/// What a self-contained hub cannot be, said before anything is written.
fn validate_self_contained(answers: &HubAnswers) -> Result<(), CreateError> {
    if answers.mesh_mode != MeshMode::None || answers.mesh_only {
        return Err(CreateError::Answers(
            "A hub that runs its own coordination server has no mesh unless it is given one, \
             and giving it one is not available yet. Create it with `--mesh none`; it is \
             reached at this machine's addresses."
                .into(),
        ));
    }
    if answers.hosts.is_empty() {
        return Err(CreateError::Answers(
            "A hub that runs its own coordination server needs an address to advertise \
             its services at."
                .into(),
        ));
    }
    let seed = &answers.seed;
    for (what, value) in [
        ("an organization", &seed.organization),
        ("a user", &seed.user),
    ] {
        if value.trim().is_empty() {
            return Err(CreateError::Answers(format!(
                "A hub that runs its own coordination server needs {what} to start with."
            )));
        }
    }
    if seed
        .redeem_tokens
        .iter()
        .any(|token| token.trim().is_empty())
    {
        return Err(CreateError::Answers(
            "A redeem token cannot be empty.".into(),
        ));
    }
    Ok(())
}

/// The coordination server a self-contained hub starts out with, from the answers.
fn seeded_lok(answers: &HubAnswers, hosts: &[AdvertisedHost]) -> LokOptions {
    let seed = &answers.seed;
    let mut redeem_tokens: Vec<String> = seed
        .redeem_tokens
        .iter()
        .map(|token| token.trim().to_string())
        .collect();
    redeem_tokens.extend(
        std::iter::repeat_with(crate::secrets::generate_redeem_token)
            .take(seed.generated_redeem_tokens),
    );
    LokOptions {
        hub_identifier: answers.identifier.trim().to_string(),
        hosts: hosts.to_vec(),
        organization: seed.organization.trim().to_string(),
        user: seed.user.trim().to_string(),
        user_password: seed.user_password.clone(),
        redeem_tokens,
        key_pair: None,
    }
}

/// Asks the coordination server to accept the hub, waits for somebody to, and folds what
/// came back into the profile. The one step of creating a hub that needs a person.
#[allow(clippy::too_many_arguments)]
async fn authorize_new_hub(
    answers: &HubAnswers,
    mut config: HubConfig,
    said: &crate::contract::Said,
    device_id: &str,
    hosts: &[AdvertisedHost],
    cancel: &CancellationToken,
    on: &(dyn Fn(CreateEvent) + Sync),
) -> Result<(HubConfig, HubCredentials, bool), CreateError> {
    let request = build_hub_request(
        &config,
        &HubManifestOptions {
            identifier: answers.identifier.trim().to_string(),
            description: answers
                .description
                .as_deref()
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .map(str::to_string),
            node_id: Some(device_id.to_string()),
            hosts: hosts.to_vec(),
            reachable_hosts: answers.reachable_hosts.clone(),
            request_auth_key: answers.mesh_mode == MeshMode::Coordination,
            // Declared up front even for a key that is only about to be minted: the
            // server resolves it against whichever node registers with that key.
            mesh_alias: answers.mesh_mode != MeshMode::None,
            // Every hub: plugins an attached engine starts sit on the hub's network and
            // reach it only as `gateway`. Scope local, so clients elsewhere skip it.
            internal_host: Some(config.gateway.host.clone()),
            expiration_seconds: None,
            // What each service is registered as, and with which scopes and roles.
            described: said.clone(),
        },
    );

    let grant = authorize::start(&answers.coord_server, &request).await?;
    on(CreateEvent::Staged {
        user_code: grant.user_code.clone(),
        verification_uri_complete: grant.verification_uri_complete.clone(),
        expires_in: grant.expires_in,
    });

    let envelope = authorize::wait_for_hub(&grant, cancel, &|progress| {
        on(CreateEvent::Waiting {
            polls: progress.polls,
            seconds_left: progress.seconds_left,
        })
    })
    .await?;

    let issued_key = envelope.mesh_grant().ionscale_auth_key.clone();
    let mesh_granted = issued_key.is_some();
    on(CreateEvent::Granted {
        mesh_key: mesh_granted,
    });

    // Fold a minted key into the config that was *just* accepted, rather than rebuilding.
    if answers.mesh_mode == MeshMode::Coordination {
        if let Some(key) = issued_key {
            config.mesh = Some(build_mesh_block(&MeshOptions {
                hostname: mesh_hostname(&answers.identifier),
                auth_key: key,
                coord_url: envelope.mesh_grant().ionscale_coord_url.clone(),
                login: envelope.login(),
            }));
        }
    }

    if answers.mesh_only {
        match config.mesh.as_mut() {
            Some(mesh) => mesh.mesh_only = true,
            // Accepted, but without a key: written as it stands, this hub would publish
            // no port and join no tailnet — reachable by nothing. Better to stop here,
            // with nothing on disk, than to write that.
            None => return Err(CreateError::NoMeshKey),
        }
    }

    enable_reporter(&mut config, &envelope);

    let credentials = HubCredentials {
        version: 1,
        server: answers.coord_server.trim().to_string(),
        identifier: answers.identifier.trim().to_string(),
        authorized_at: now_rfc3339(),
        issuer: grant.issuer.clone(),
        envelope: envelope.clone(),
        advertised_hosts: hosts.to_vec(),
    };
    Ok((config, credentials, mesh_granted))
}

/// Gets the source of each of `services` that runs from source to where compose mounts it
/// from: cloned into `mounts/<service>`, with the empty `config.yaml` placeholder put in
/// it — or, for a folder somebody named, nothing at all, since that is used where it is.
///
/// Driven by the profile, not by the answers: `mount_github` is where both `--dev` and a
/// single service's "run from source" end up, and it is the field the compose file's bind
/// mounts were written from. Cloning anything else — or less — is how a container gets
/// handed an empty workspace. A service that already has a checkout keeps it.
///
/// A fresh checkout is at the branch that was asked for; when none was and the image says
/// which commit it was built from, at that commit, detached — so that what runs from the
/// checkout is, to begin with, what ran from the image.
pub(crate) fn check_sources_out(
    dir: &Path,
    config: &HubConfig,
    services: &[ServiceId],
    branch_of: &dyn Fn(ServiceId) -> Option<String>,
    on: &(dyn Fn(CreateEvent) + Sync),
) -> Result<(), CreateError> {
    for id in services
        .iter()
        .copied()
        .filter(|id| config.service(*id).mount_github)
    {
        let service = config.service(id);
        let (repo, revision) = match service.runs_from() {
            Some(RunsFrom::Repository {
                repository,
                revision,
            }) => (repository, revision),
            Some(RunsFrom::Folder(folder)) => {
                on(CreateEvent::Log {
                    line: format!(
                        "{} runs from {folder}, used where it is — nothing is cloned, and \
                         nothing in it is touched",
                        service.host
                    ),
                });
                continue;
            }
            // Refused before anything was written (`sources_are_known`).
            None => continue,
        };
        let branch = branch_of(id);
        let into = git::checkout_dir(dir, &service.host);
        std::fs::create_dir_all(&into)?;

        on(CreateEvent::Cloning {
            service: service.host.clone(),
            repo: repo.to_string(),
            branch: branch.clone(),
        });

        let cloned = git::clone_service(&service.host, repo, branch.as_deref(), &into)?;
        if cloned {
            // Ours from here on, and written down as such: a checkout that was already
            // there is not, and deleting the hub goes by which is which.
            crate::lock::record_checkout(dir, &service.host)?;
        }
        if let (true, None, Some(revision)) = (cloned, &branch, revision) {
            git::checkout_revision(repo, &into, revision)?;
            on(CreateEvent::Log {
                line: format!(
                    "{} is checked out at {revision}, the commit its image was built from \
                     (detached: switch to a branch to work in it)",
                    service.host
                ),
            });
        }

        // The config is bind-mounted at `/workspace/config.yaml`, which is *inside* the
        // checkout. Docker creates a missing mount point itself, as root — so the file is
        // created here instead, owned by whoever owns the checkout.
        let placeholder = into.join("config.yaml");
        if !placeholder.exists() {
            std::fs::write(&placeholder, "")?;
        }

        if !cloned {
            on(CreateEvent::Log {
                line: format!("{} already has a checkout — left as it is", service.host),
            });
        }
    }
    Ok(())
}

/// Refuses a hub in which a service is to run from source and nothing says which: its
/// image does not say where its code came from, and nobody named a source for it.
pub(crate) fn sources_are_known(config: &HubConfig) -> Result<(), CreateError> {
    let lost = config.without_a_source();
    if lost.is_empty() {
        return Ok(());
    }
    let named = lost
        .iter()
        .map(|host| format!("`{host}`"))
        .collect::<Vec<_>>()
        .join(", ");
    Err(CreateError::Answers(format!(
        "{named} cannot run from source: its image does not say where its code came from. \
         Name the source — `--from-source {0}=URL[@BRANCH]` for a repository to clone, \
         `--from-source {0}=/a/folder` for one on this machine — or run it from its image. \
         Nothing was written.",
        lost[0]
    )))
}

/// The files a hub with these answers would be written to, without writing any of them.
///
/// Built from a throwaway config with a placeholder identity: the *names* do not depend on
/// the keys, and asking a coordination server for real ones is exactly what a preview must
/// not do.
pub fn preview_files(answers: &HubAnswers) -> Vec<String> {
    let config = crate::config::hub::build_hub_config(&crate::config::hub::HubConfigOptions {
        coord_server: answers.coord_server.trim().to_string(),
        rekuest_server: answers.rekuest_server.clone(),
        services: Some(answers.services.clone()),
        http_port: Some(answers.http_port),
        https_port: Some(answers.https_port),
        ssl: answers.ssl,
        // The bind mounts a source checkout adds live in the compose file, so the
        // preview only tells the truth if it knows which services asked for one.
        dev_hub: answers.dev_hub,
        service_options: answers.service_options.clone(),
        storage: answers.storage,
        ..Default::default()
    });
    // A preview asks no image anything: it names the files, and those do not depend on
    // what the images say.
    crate::generate::generate_hub_files(
        &config,
        &crate::generate::IssuedIdentity::default(),
        &Default::default(),
    )
    .into_keys()
    .collect()
}

/// The verdict on the container engine and what to do about it, as text. Worded once, in
/// `remedy`, for both front ends — the desktop app shows the same remedies as buttons.
pub fn describe_docker(probe: &docker::DockerProbe) -> String {
    crate::remedy::describe(probe)
}

/// Gives an authorized hub its health reporter — when the grant carries a refresh token,
/// which is the only way the reporter can log in as the hub once the access token lapses.
/// A reporter already in the profile keeps its image, so a pinned or rolled-back one
/// survives a re-authorization.
fn enable_reporter(config: &mut HubConfig, envelope: &crate::connect::authorize::HubEnvelope) {
    let can_log_in = envelope
        .refresh_token
        .as_deref()
        .is_some_and(|t| !t.is_empty());
    if can_log_in {
        config.reporter.get_or_insert_with(Default::default);
    }
}

/// An RFC 3339 timestamp without pulling in a date library for one call site.
pub(crate) fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Days since the epoch, converted with the civil-from-days algorithm.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The folder a fresh hub is offered: `MyHub` in the user's home, stepping past any name
/// already taken.
pub fn suggest_folder(base: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let store = registry::load();

    for attempt in 0..20 {
        let name = if attempt == 0 {
            base.to_string()
        } else {
            format!("{base}-{}", attempt + 1)
        };
        let candidate = home.join(&name);

        if !candidate.exists() {
            return Some(candidate);
        }
        if registry::inspect_folder(&store, &candidate).can_create() {
            return Some(candidate);
        }
    }
    None
}

/// The hub identifier a folder suggests: its name, slugified into the shape the
/// coordination server accepts (`^[a-zA-Z0-9][a-zA-Z0-9._-]*$`).
pub fn identifier_from_folder(path: &Path) -> String {
    let name = compose::basename(&path.to_string_lossy()).to_lowercase();

    // A *run* of disallowed characters folds to a single dash, matching the regex this
    // replaces (`[^a-z0-9._-]+`). Folding per character would turn "hub (2)" into
    // "hub--2" — dashes are allowed, so they are never collapsed themselves.
    let allowed =
        |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' || c == '-';
    let mut folded = String::with_capacity(name.len());
    let mut in_run = false;
    for c in name.chars() {
        if allowed(c) {
            folded.push(c);
            in_run = false;
        } else if !in_run {
            folded.push('-');
            in_run = true;
        }
    }

    let trimmed: String = folded
        .trim_start_matches(|c: char| !c.is_ascii_lowercase() && !c.is_ascii_digit())
        .trim_end_matches('-')
        .chars()
        .take(60)
        .collect();

    if trimmed.chars().count() >= 2 {
        trimmed
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_an_identifier_the_server_will_accept() {
        assert_eq!(
            identifier_from_folder(Path::new("/home/someone/MyHub")),
            "myhub"
        );
        assert_eq!(
            identifier_from_folder(Path::new("/home/someone/My Lab Hub")),
            "my-lab-hub"
        );
        assert_eq!(
            identifier_from_folder(Path::new("/home/someone/hub (2)")),
            "hub-2"
        );
        assert_eq!(
            identifier_from_folder(Path::new("/home/someone/lab.hub_2-a")),
            "lab.hub_2-a"
        );
        // A leading dot or dash would fail the server's pattern.
        assert_eq!(
            identifier_from_folder(Path::new("/home/someone/.hidden")),
            "hidden"
        );
        // Below the two-character minimum: better an empty field than a wrong one.
        assert_eq!(identifier_from_folder(Path::new("/home/someone/x")), "");
        assert_eq!(identifier_from_folder(Path::new("/home/someone/...")), "");
    }

    /// A pin reaches the profile, and with it everything that runs the image — and a pin
    /// for a service the stack does not have is an error, not a pin that did nothing.
    #[test]
    fn an_image_can_be_pinned_by_compose_service() {
        use crate::config::hub::{build_hub_config, HubConfigOptions};
        let mut config = build_hub_config(&HubConfigOptions {
            coord_server: LOCAL_COORD_SERVER.into(),
            ..Default::default()
        });
        let pins = BTreeMap::from([
            ("rekuest".to_string(), "jhnnsrs/rekuest:5.0.1".to_string()),
            ("lok".to_string(), " jhnnsrs/lok:3.1.0 ".to_string()),
        ]);
        apply_images(&mut config, &pins).unwrap();
        assert_eq!(
            config
                .service(crate::catalog::ServiceId::Rekuest)
                .image
                .as_deref(),
            Some("jhnnsrs/rekuest:5.0.1")
        );
        assert_eq!(config.running_lok().unwrap().image, "jhnnsrs/lok:3.1.0");
        // takt's image belongs to Rekuest's, so it moves with it.
        let compose = crate::generate::compose::build_compose(
            &config,
            &config.enabled_services(),
            &Default::default(),
        );
        assert_eq!(
            compose["services"]["rekuest-takt"]["image"],
            serde_norway::Value::from("jhnnsrs/rekuest-takt:5.0.1")
        );

        // A default set for a whole run names services this hub may not have; those are
        // skipped, where a pin given to this hub in particular is refused.
        let defaults = BTreeMap::from([
            ("mikro".to_string(), "jhnnsrs/mikro:6.0.0".to_string()),
            ("elektro".to_string(), "jhnnsrs/elektro:1.0.0".to_string()),
        ]);
        assert_eq!(
            images_this_hub_runs(&config, &defaults),
            BTreeMap::from([("mikro".to_string(), "jhnnsrs/mikro:6.0.0".to_string())])
        );

        // A service outside the catalogue has no image until it is given one this way.
        let mut hosting = build_hub_config(&HubConfigOptions {
            services: Some(vec![ServiceId::named("example")]),
            ..Default::default()
        });
        assert_eq!(hosting.imageless_services(), ["example"]);
        let named = BTreeMap::from([("example".to_string(), "example:1".to_string())]);
        apply_images(&mut hosting, &named).unwrap();
        assert!(hosting.imageless_services().is_empty());
        assert!(hosting.runs(ServiceId::named("example")));

        let unknown = BTreeMap::from([("rekuset".to_string(), "x".to_string())]);
        let refused = apply_images(&mut config, &unknown).unwrap_err().to_string();
        assert!(
            refused.contains("`rekuset`") && refused.contains("rekuest"),
            "{refused}"
        );
    }

    #[test]
    fn stamps_a_plausible_timestamp() {
        let now = now_rfc3339();
        assert_eq!(now.len(), 20, "{now}");
        assert!(now.ends_with('Z'));
        // The port happened well after 2020 and, one hopes, well before 2100.
        let year: i64 = now[..4].parse().expect("a year");
        assert!((2020..2100).contains(&year), "{now}");
    }

    #[test]
    fn identifiers_follow_the_wizard_rule() {
        for ok in ["lab-hub", "ab", "Lab.Hub_2", "9lives"] {
            assert!(validate_identifier(ok).is_ok(), "{ok}");
        }
        for bad in ["a", "-lab", "lab hub", "lab/hub", "", &"a".repeat(61)] {
            assert!(validate_identifier(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn service_answers_are_held_to_the_wizard_rules() {
        use crate::config::hub::OllamaChoice;
        let one = |options: ServiceOptions| BTreeMap::from([(ServiceId::Mikro, options)]);

        assert!(validate_service_options(&one(ServiceOptions {
            from_source: true,
            branch: Some("feature/new-thing".into()),
            ..Default::default()
        }))
        .is_ok());
        assert!(validate_service_options(&one(ServiceOptions {
            from_source: true,
            branch: Some("has space".into()),
            ..Default::default()
        }))
        .is_err());
        // A remote Ollama with no address would leave Alpaka with nothing to talk to.
        assert!(validate_service_options(&BTreeMap::from([(
            ServiceId::Alpaka,
            ServiceOptions {
                ollama: Some(OllamaChoice {
                    run_locally: false,
                    url: Some("  ".into()),
                }),
                ..Default::default()
            }
        )]))
        .is_err());
    }

    #[test]
    fn a_mesh_key_is_asked_for_only_when_it_is_needed() {
        use crate::config::hub::{build_hub_config, HubConfigOptions};
        let off_mesh = build_hub_config(&HubConfigOptions::default());
        let mut on_mesh = off_mesh.clone();
        on_mesh.mesh = Some(build_mesh_block(&MeshOptions {
            hostname: "lab".into(),
            auth_key: "k".into(),
            coord_url: None,
            login: None,
        }));
        let mut keyed = on_mesh.clone();
        keyed.mesh.as_mut().unwrap().login = Some("2-3-50".into());

        assert!(MeshKeyRequest::Auto.asks(&off_mesh));
        assert!(!MeshKeyRequest::Auto.asks(&on_mesh));
        // Keyed by login: which login the next grant is only shows once it is accepted.
        assert!(MeshKeyRequest::Auto.asks(&keyed));
        assert!(MeshKeyRequest::Fresh.asks(&on_mesh));
        assert!(!MeshKeyRequest::Never.asks(&off_mesh));
    }

    #[test]
    fn converts_a_known_day_correctly() {
        // 2000-03-01 is the epoch the civil-from-days algorithm is centred on.
        assert_eq!(civil_from_days(11017), (2000, 3, 1));
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }
}

/// A hub identifier the coordination server will take: 2 to 60 characters, starting with
/// a letter or digit, then letters, digits, dots, dashes and underscores. The wizard
/// holds the same rule; this is the one both front ends are held to.
pub fn validate_identifier(identifier: &str) -> Result<(), CreateError> {
    let id = identifier.trim();
    let len = id.chars().count();
    let starts_well = id.chars().next().is_some_and(|c| c.is_ascii_alphanumeric());
    let only_allowed = id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !(2..=60).contains(&len) {
        return Err(CreateError::Answers(format!(
            "The hub identifier must be 2 to 60 characters; `{id}` is {len}."
        )));
    }
    if !starts_well || !only_allowed {
        return Err(CreateError::Answers(format!(
            "The hub identifier `{id}` may only use letters, digits, dots, dashes and \
             underscores, and must start with a letter or digit."
        )));
    }
    Ok(())
}

/// What can be refused about one service's answers before anything is created — the rules
/// the wizard holds on its services step. A branch name is git's to validate; only shapes
/// git can never accept are refused, so a typo is caught before the clone is attempted.
pub fn validate_service_options(
    options: &BTreeMap<ServiceId, ServiceOptions>,
) -> Result<(), CreateError> {
    for (id, asked) in options {
        if let Some(branch) = asked
            .branch
            .as_deref()
            .map(str::trim)
            .filter(|b| !b.is_empty())
        {
            if branch
                .chars()
                .any(|c| c.is_whitespace() || "~^:?*[\\".contains(c))
            {
                return Err(CreateError::Answers(format!(
                    "`{branch}` is not a branch name git would accept (for {})",
                    id.as_str()
                )));
            }
        }
        // A folder somebody named is used where it is, so it has to be there.
        if let Some(source) = asked.source.as_deref().map(str::trim) {
            if crate::config::hub::is_local_folder(source) && !Path::new(source).is_dir() {
                return Err(CreateError::Answers(format!(
                    "{} was told to run from the folder {source}, and there is no such \
                     folder",
                    id.as_str()
                )));
            }
        }
        if let Some(ollama) = &asked.ollama {
            let url = ollama.url.as_deref().map(str::trim).unwrap_or("");
            if !ollama.run_locally && url.is_empty() {
                return Err(CreateError::Answers(format!(
                    "{} was told to use an Ollama that already exists, but not where it is",
                    id.as_str()
                )));
            }
        }
    }
    Ok(())
}

/// Whether re-authorizing asks the coordination server for a mesh key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeshKeyRequest {
    /// Ask when the hub is not on a mesh yet, or is on one through a key keyed by login.
    ///
    /// Which login a grant is only shows once it is accepted, and a key can only come
    /// with that same response — so a keyed hub always asks. Accepted as the same login,
    /// the sidecar finds its node state and ignores the spare key; as another (another
    /// user, organization or hub, whose earlier login may be revoked along with its node),
    /// it starts a fresh node with it. A hand-supplied key, or one from before servers
    /// said whose login it was, is left alone: a second key would only replace it.
    #[default]
    Auto,
    /// Ask regardless. The way back for a hub whose key expired before it ever joined —
    /// keys are single-use and live fifteen minutes.
    Fresh,
    /// Never ask.
    Never,
}

impl MeshKeyRequest {
    pub fn asks(self, config: &HubConfig) -> bool {
        match self {
            Self::Auto => match config.mesh.as_ref().filter(|m| m.enabled) {
                None => true,
                Some(mesh) => mesh.login.is_some(),
            },
            Self::Fresh => true,
            Self::Never => false,
        }
    }
}

/// The advertised hosts, by name.
fn host_names(hosts: &[AdvertisedHost]) -> Vec<String> {
    hosts.iter().map(|h| h.host.clone()).collect()
}

/// Re-authorizing a hub that already exists on disk.
///
/// This is how a hub moves to a different network, joins the mesh it was created without,
/// or — with [`ReauthorizeAnswers::services`] — gains or loses services. The profile is
/// reused verbatim — its secrets and provenance key are what the running services already
/// trust — and only the manifest is sent again.
///
/// The service configs are then regenerated, because the JWKS URL the coordination server
/// returns is what they verify inbound tokens against, and it may have moved.
#[derive(Debug, Clone)]
pub struct ReauthorizeAnswers {
    pub dir: PathBuf,
    pub coord_server: String,
    pub identifier: String,
    pub description: Option<String>,
    /// Ignored for a mesh-only hub, which advertises nothing on this machine's networks.
    pub hosts: Vec<AdvertisedHost>,
    /// Of `hosts`, the ones an external probe reached.
    pub reachable_hosts: Vec<String>,
    pub mesh_key: MeshKeyRequest,
    /// Services to add or take out with this authorization — see [`crate::services`].
    /// Checked before anything is sent, and folded into the profile only once accepted.
    pub services: Option<crate::services::ServiceChange>,
    /// What images said of themselves when they were asked which service they are
    /// (`hub services add --image`), by image: they are not asked a second time.
    pub described: BTreeMap<String, crate::contract::Description>,
}

/// What re-authorizing did, beyond the credentials it wrote.
#[derive(Debug, Clone)]
pub struct Reauthorized {
    pub credentials: HubCredentials,
    /// A mesh key was asked for.
    pub mesh_requested: bool,
    /// And one came back. It expires fifteen minutes after it was issued.
    pub mesh_granted: bool,
    /// The hub now reports its own health (the grant carried a refresh token).
    pub reporter_enabled: bool,
    /// What the service change did, when one was asked for.
    pub services: Option<crate::services::ServicePlan>,
}

pub async fn reauthorize(
    answers: &ReauthorizeAnswers,
    cancel: &CancellationToken,
    on: &(dyn Fn(CreateEvent) + Sync),
) -> Result<Reauthorized, CreateError> {
    let profile = crate::profile::read_profile(&answers.dir)
        .map_err(|e| CreateError::Folder(e.to_string()))?;
    let mut config = profile.config;
    // It ends by rewriting every generated file, which a hub of an older layout must not
    // have done to it in passing: its images would stay where they are.
    if let Some(reason) = crate::migrate::behind(&answers.dir, &config) {
        return Err(CreateError::Folder(reason));
    }
    if config.running_lok().is_some() {
        return Err(CreateError::Answers(
            "This hub runs its own coordination server, so there is nobody to authorize it \
             with — and its services are part of what that server was set up with. To change \
             them, create the hub again with the services you want."
                .into(),
        ));
    }
    // A service change is refused here, before anybody is sent to a browser, and applied
    // to this copy only: the profile on disk changes once the grant is accepted.
    // A release an update could not move to, because it asks for a key its service does
    // not hold: the key is minted here, where it is also sent to be vouched for. Into
    // this copy only, like everything else — it is the service's once this is accepted.
    let awaited = crate::lock::read(&answers.dir).awaiting_key;
    for host in &awaited {
        if let Some(id) = config.service_at(host) {
            let block = config.service_mut(id);
            if block.instance_key_pair.is_none() {
                block.instance_key_pair = Some(crate::secrets::generate_ed25519_key_pair());
            }
        }
    }
    let mut said = crate::contract::known(&answers.dir);
    let services = match &answers.services {
        Some(change) => {
            let plan = crate::services::plan(&config, &said, change)?;
            crate::services::apply_plan(&mut config, &plan);
            Some(plan)
        }
        None => None,
    };
    // A service just added has not said what it is on this hub yet, and the manifest is
    // written from that: it is asked now, before anybody is sent to a browser. What it
    // answers is written down only once the change is accepted.
    let unasked: Vec<ServiceId> = config
        .enabled_services()
        .into_iter()
        .filter(|id| !said.contains_key(&config.service(*id).host))
        .collect();
    if !unasked.is_empty() {
        on(CreateEvent::Log {
            line: "Asking the images what they are…".into(),
        });
        let asked = futures_util::future::join_all(unasked.iter().map(|id| {
            let image = config.service(*id).image.clone().unwrap_or_default();
            async move {
                match answers.described.get(&image) {
                    Some(said) => Some(said.clone()),
                    None => crate::contract::describe(&image).await,
                }
            }
        }))
        .await;
        for (id, description) in unasked.iter().zip(asked) {
            if let Some(description) = description {
                said.insert(config.service(*id).host.clone(), description);
            }
        }
    }
    let silent = crate::contract::undescribed(&config, &said);
    if !silent.is_empty() {
        return Err(CreateError::Answers(format!(
            "{} did not say what it is when run with no command: the image cannot be pulled, \
             or it is a release from before a service described itself. Nothing was changed.",
            silent.join(", ")
        )));
    }
    crate::contract::acceptable(&config, &said).map_err(CreateError::Answers)?;
    // With every description in hand, the one refusal a plan could not make on its own:
    // a service added in this very change may be one Rekuest cannot be taken out beside.
    if let Some(plan) = &services {
        if plan.removed.contains(&ServiceId::Rekuest) {
            let hooked = crate::services::hooked_by_rekuest(&config, &said, &plan.services);
            if !hooked.is_empty() {
                return Err(CreateError::Answers(format!(
                    "Rekuest runs the periodic work and receives the signals of {} — remove \
                     those too, or keep Rekuest",
                    hooked
                        .iter()
                        .map(|id| id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        }
    }
    // What a service asked for and does not have yet — a service just added, a release
    // that declares more — is provided now; the profile is rewritten below, so a key is
    // minted once and sent to the coordination server with this very request.
    config.provide(&said);
    sources_are_known(&config)?;
    validate_identifier(&answers.identifier)?;

    let store = registry::load();

    let request_auth_key = answers.mesh_key.asks(&config);
    // A hub about to get a key, or already on the mesh, keeps declaring its mesh alias —
    // re-authorizing would otherwise drop it.
    let on_mesh = request_auth_key || config.mesh.as_ref().is_some_and(|m| m.enabled);
    let mesh_only = config
        .mesh
        .as_ref()
        .is_some_and(|m| m.enabled && m.mesh_only);
    // Mesh-only publishes no port, so an address on this machine's networks would be
    // advertised to clients with nothing listening behind it — whoever asked for it.
    let hosts = if mesh_only {
        Vec::new()
    } else {
        answers.hosts.clone()
    };
    // The hub may have moved networks, or just gained Lovekit: either way LiveKit
    // announces one of the addresses advertised now.
    config.place_livekit(&host_names(&hosts));

    on(CreateEvent::Building);
    let request = build_hub_request(
        &config,
        &HubManifestOptions {
            identifier: answers.identifier.trim().to_string(),
            description: answers
                .description
                .as_deref()
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .map(str::to_string),
            node_id: Some(store.device_id.clone()),
            hosts: hosts.clone(),
            reachable_hosts: answers.reachable_hosts.clone(),
            request_auth_key,
            mesh_alias: on_mesh,
            internal_host: Some(config.gateway.host.clone()),
            expiration_seconds: None,
            // What the services' images have said of themselves on this hub: what each is
            // registered as, with its own scopes and roles.
            described: said.clone(),
        },
    );

    let grant = authorize::start(&answers.coord_server, &request).await?;
    on(CreateEvent::Staged {
        user_code: grant.user_code.clone(),
        verification_uri_complete: grant.verification_uri_complete.clone(),
        expires_in: grant.expires_in,
    });

    let envelope = authorize::wait_for_hub(&grant, cancel, &|progress| {
        on(CreateEvent::Waiting {
            polls: progress.polls,
            seconds_left: progress.seconds_left,
        })
    })
    .await?;

    let mesh_granted = envelope.mesh_grant().ionscale_auth_key.is_some();
    on(CreateEvent::Granted {
        mesh_key: mesh_granted,
    });

    if request_auth_key {
        if let Some(key) = envelope.mesh_grant().ionscale_auth_key.clone() {
            let mut block = build_mesh_block(&MeshOptions {
                hostname: mesh_hostname(&answers.identifier),
                auth_key: key,
                coord_url: envelope.mesh_grant().ionscale_coord_url.clone(),
                login: envelope.login(),
            });
            // A fresh key is not a change of mind about how the hub is reached.
            block.mesh_only = mesh_only;
            config.mesh = Some(block);
        }
    }

    enable_reporter(&mut config, &envelope);
    // Added to, never narrowed: an origin somebody put into the profile by hand stays.
    let mut origins = config.csrf_trusted_origins.take().unwrap_or_default();
    for origin in trusted_origins(&config, &host_names(&hosts)) {
        if !origins.contains(&origin) {
            origins.push(origin);
        }
    }
    config.csrf_trusted_origins = Some(origins);
    let reporter_enabled = config.reporter.as_ref().is_some_and(|r| r.enabled);

    let credentials = HubCredentials {
        version: 1,
        server: answers.coord_server.trim().to_string(),
        identifier: answers.identifier.trim().to_string(),
        authorized_at: now_rfc3339(),
        issuer: grant.issuer.clone(),
        envelope,
        advertised_hosts: hosts,
    };

    // Generation first: a profile this app did not write could fail here, and a
    // half-updated folder is worse than an unchanged one.
    let files = generate_hub_files(&config, &credentials.issued_identity(), &said);
    for name in files.keys() {
        on(CreateEvent::Writing { file: name.clone() });
    }

    write_profile(&answers.dir, &hub_profile(config.clone()))
        .map_err(|e| CreateError::Write(std::io::Error::other(e.to_string())))?;
    write_credentials(&answers.dir, &credentials)?;
    crate::migrate::write_hub(&answers.dir, &config, &files)?;
    // So the next start does not ask a service that was just added again.
    crate::contract::remember(&answers.dir, &config, &said)?;
    if !awaited.is_empty() {
        let mut held = crate::lock::read(&answers.dir);
        held.awaiting_key.clear();
        crate::lock::write(&answers.dir, &held)?;
    }

    // The registry record now describes the wrong hub: the identifier is editable on the
    // authorize screen, the coordination server can differ, and `last_generated_at` has to
    // move with the files that were just rewritten — the dashboard compares it against
    // `authorized_at`, which the credentials above have just pushed forward.
    //
    // Loaded again rather than reusing the snapshot from the top: authorization waits on a
    // human, and saving a copy read before that wait would drop anything registered in the
    // meantime.
    let mut store = registry::load();
    registry::record_regeneration(
        &mut store,
        &answers.dir.to_string_lossy(),
        Some(credentials.server.clone()),
        Some(credentials.identifier.clone()),
        now_rfc3339(),
    );
    let _ = registry::save(&store);

    on(CreateEvent::Done {
        path: answers.dir.to_string_lossy().to_string(),
    });
    Ok(Reauthorized {
        credentials,
        mesh_requested: request_auth_key,
        mesh_granted: request_auth_key && mesh_granted,
        reporter_enabled,
        services,
    })
}

#[cfg(test)]
mod source_tests {
    //! Running a service from its source: which source, where it is mounted, and what a
    //! fresh checkout is at.

    use super::*;
    use crate::config::hub::{build_hub_config, HubConfigOptions, RunsFrom, ServiceOptions};
    use crate::contract::{Said, Source};
    use crate::generate::compose::build_compose;
    use crate::support;
    use std::process::Command;

    fn example() -> ServiceId {
        ServiceId::named("example")
    }

    /// What the images say, with `example`'s saying where its code came from.
    fn said_with(source: Option<Source>) -> Said {
        let mut example = support::example();
        example.source = source;
        let mut said = support::said();
        said.insert("example".to_string(), example);
        said
    }

    /// A hub of `example` alone, run from source as `asked` says, with what its image says
    /// taken in.
    fn hub(asked: ServiceOptions, said: &Said) -> HubConfig {
        let mut config = build_hub_config(&HubConfigOptions {
            services: Some(vec![example()]),
            rekuest_server: "none".into(),
            service_options: BTreeMap::from([(example(), asked)]),
            ..Default::default()
        });
        config.set_service_image("example", "example:1");
        config.provide(said);
        config
    }

    fn from_source() -> ServiceOptions {
        ServiceOptions {
            from_source: true,
            ..Default::default()
        }
    }

    fn mounts(config: &HubConfig, said: &Said) -> Vec<String> {
        let compose = build_compose(config, &config.enabled_services(), said);
        compose["services"]["example"]["volumes"]
            .as_sequence()
            .expect("volumes")
            .iter()
            .map(|volume| volume.as_str().unwrap().to_string())
            .collect()
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "konstruktor-source-{tag}-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn git(at: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(at)
            .args([
                "-c",
                "user.name=konstruktor",
                "-c",
                "user.email=konstruktor@example.org",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A repository with two commits on `main` and a third on `feature`: the commits, in
    /// that order.
    fn repository() -> (PathBuf, [String; 3]) {
        let repo = scratch("repo");
        git(&repo, &["init", "--quiet", "--initial-branch", "main"]);
        let commit = |file: &str| {
            std::fs::write(repo.join(file), file).unwrap();
            git(&repo, &["add", "."]);
            git(&repo, &["commit", "--quiet", "-m", file]);
            git(&repo, &["rev-parse", "HEAD"])
        };
        let first = commit("first");
        let second = commit("second");
        git(&repo, &["checkout", "--quiet", "-b", "feature"]);
        let third = commit("third");
        git(&repo, &["checkout", "--quiet", "main"]);
        (repo, [first, second, third])
    }

    /// A service nobody has heard of runs from source like any other, once its image says
    /// where its code came from: cloned from there, mounted where the image keeps it.
    #[test]
    fn an_unknown_service_runs_from_the_source_its_image_names() {
        let said = said_with(Some(Source {
            repository: "https://git.example.org/me/example".into(),
            revision: Some("abc123".into()),
            path: "/srv/example".into(),
        }));
        let config = hub(from_source(), &said);
        let block = config.service(example());

        assert_eq!(
            block.runs_from(),
            Some(RunsFrom::Repository {
                repository: "https://git.example.org/me/example",
                revision: Some("abc123"),
            })
        );
        assert!(sources_are_known(&config).is_ok());
        let mounts = mounts(&config, &said);
        // The checkout, where the image says its code sits; the config where every
        // service looks for it.
        assert!(
            mounts.contains(&"./mounts/example:/srv/example".to_string()),
            "{mounts:?}"
        );
        assert!(
            mounts.contains(&"./configs/example.yaml:/workspace/config.yaml".to_string()),
            "{mounts:?}"
        );
        // And the profile remembers it, as the image said it.
        let text = serde_norway::to_string(&config).unwrap();
        let back: HubConfig = serde_norway::from_str(&text).unwrap();
        assert_eq!(back, config);

        // Left unsaid, the code sits in the workspace.
        let said = said_with(Some(
            serde_json::from_str(r#"{"repository": "https://git.example.org/me/example"}"#)
                .unwrap(),
        ));
        let config = hub(from_source(), &said);
        assert_eq!(config.service(example()).source_path(), "/workspace");
        assert!(mounts_of(&config, &said).contains(&"./mounts/example:/workspace".to_string()));

        // A service that runs from its image mounts no source at all.
        let config = hub(ServiceOptions::default(), &said);
        assert!(mounts_of(&config, &said)
            .iter()
            .all(|mount| !mount.contains("mounts/")));
    }

    fn mounts_of(config: &HubConfig, said: &Said) -> Vec<String> {
        mounts(config, said)
    }

    /// A service whose image does not say where its code came from cannot run from source
    /// unless somebody says — and is refused naming the flag that does.
    #[test]
    fn a_service_with_no_source_is_refused_naming_the_flag() {
        let said = said_with(None);
        let config = hub(from_source(), &said);
        assert_eq!(config.without_a_source(), ["example"]);
        let refused = sources_are_known(&config).unwrap_err().to_string();
        assert!(refused.contains("`example`"), "{refused}");
        assert!(
            refused.contains("--from-source example=URL[@BRANCH]"),
            "{refused}"
        );
        assert!(
            refused.contains("--from-source example=/a/folder"),
            "{refused}"
        );

        // `--dev` is the same question, asked of every service.
        let mut dev = build_hub_config(&HubConfigOptions {
            services: Some(vec![example()]),
            rekuest_server: "none".into(),
            dev_hub: true,
            ..Default::default()
        });
        dev.set_service_image("example", "example:1");
        dev.provide(&said);
        assert!(sources_are_known(&dev).is_err());

        // Run from its image, it needs none.
        assert!(sources_are_known(&hub(ServiceOptions::default(), &said)).is_ok());
    }

    /// A repository somebody names is cloned in place of the image's — without the
    /// image's revision, which is a commit of another repository.
    #[test]
    fn a_named_repository_takes_the_place_of_the_images() {
        let said = said_with(Some(Source {
            repository: "https://git.example.org/me/example".into(),
            revision: Some("abc123".into()),
            path: "/srv/example".into(),
        }));
        let asked = ServiceOptions {
            source: Some("https://git.example.org/fork/example".into()),
            ..from_source()
        };
        let config = hub(asked.clone(), &said);
        assert_eq!(
            config.service(example()).runs_from(),
            Some(RunsFrom::Repository {
                repository: "https://git.example.org/fork/example",
                revision: None,
            })
        );
        // Still mounted where the image keeps its code.
        assert!(mounts(&config, &said).contains(&"./mounts/example:/srv/example".to_string()));

        // And it is all a service whose image says nothing needs.
        let silent = said_with(None);
        let config = hub(asked, &silent);
        assert!(sources_are_known(&config).is_ok());
        assert!(mounts(&config, &silent).contains(&"./mounts/example:/workspace".to_string()));
    }

    /// A folder somebody names is used where it is: mounted from there, with the service's
    /// config beside it rather than in it, nothing cloned and nothing written into it.
    #[test]
    fn a_named_folder_is_used_in_place_and_left_as_it_is() {
        let folder = scratch("folder");
        std::fs::write(folder.join("manage.txt"), "mine").unwrap();
        let named = folder.to_string_lossy().to_string();
        let said = said_with(None);
        let config = hub(
            ServiceOptions {
                source: Some(named.clone()),
                ..from_source()
            },
            &said,
        );
        assert_eq!(
            config.service(example()).runs_from(),
            Some(RunsFrom::Folder(&named))
        );
        assert!(sources_are_known(&config).is_ok());

        let mounts = mounts(&config, &said);
        assert!(
            mounts.contains(&format!("{named}:/workspace:ro")),
            "{mounts:?}"
        );
        // A file mounted into the folder would be created in it: the config is mounted
        // outside it, and the service told where.
        assert!(
            mounts.contains(&"./configs/example.yaml:/hub/config.yaml".to_string()),
            "{mounts:?}"
        );
        let compose = build_compose(&config, &config.enabled_services(), &said);
        assert_eq!(
            compose["services"]["example"]["environment"]["ARKITEKT_CONFIG_FILE"].as_str(),
            Some("/hub/config.yaml")
        );

        // Getting the sources in place clones nothing and touches nothing.
        let dir = scratch("hub");
        let narrated = std::sync::Mutex::new(Vec::new());
        check_sources_out(&dir, &config, &[example()], &|_| None, &|event| {
            narrated.lock().unwrap().push(format!("{event:?}"))
        })
        .unwrap();
        assert!(!dir.join("mounts").exists(), "nothing is cloned");
        let left: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(left, ["manage.txt"], "nothing was put into the folder");
        let narrated = narrated.lock().unwrap().join("\n");
        assert!(narrated.contains("used where it is"), "{narrated}");

        // A folder that is not there is refused before anything is created.
        // Absolute on this platform, whichever it is: `/no/such` names no folder on Windows.
        let nowhere = std::env::temp_dir().join("konstruktor-no-such-folder-anywhere");
        let missing = BTreeMap::from([(
            example(),
            ServiceOptions {
                source: Some(nowhere.to_string_lossy().into_owned()),
                ..from_source()
            },
        )]);
        assert!(validate_service_options(&missing).is_err());
        std::fs::remove_dir_all(&folder).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A fresh checkout is at the commit the image says it was built from, detached, and
    /// says so — unless a branch was asked for, which wins. One that is there is left.
    #[test]
    fn a_fresh_checkout_is_at_the_revision_the_image_names() {
        if !git::probe().is_ready() {
            eprintln!("skipping: no git on this machine");
            return;
        }
        let (repo, [first, second, third]) = repository();
        let said = said_with(Some(Source {
            repository: repo.to_string_lossy().to_string(),
            revision: Some(first.clone()),
            path: "/workspace".into(),
        }));
        let config = hub(from_source(), &said);
        let head = |dir: &Path| git(&dir.join("mounts/example"), &["rev-parse", "HEAD"]);

        // Nobody asked for a branch: the image's commit, detached.
        let dir = scratch("at-revision");
        let narrated = std::sync::Mutex::new(Vec::new());
        check_sources_out(&dir, &config, &[example()], &|_| None, &|event| {
            narrated.lock().unwrap().push(format!("{event:?}"))
        })
        .unwrap();
        assert_eq!(head(&dir), first);
        let checkout = git::read_checkout(
            "example",
            &repo.to_string_lossy(),
            &dir.join("mounts/example"),
        );
        assert!(checkout.detached, "{checkout:?}");
        let said_so = narrated.lock().unwrap().join("\n");
        assert!(
            said_so.contains(&first) && said_so.contains("detached"),
            "{said_so}"
        );
        // The placeholder the config is mounted over is there, as for any checkout.
        assert!(dir.join("mounts/example/config.yaml").is_file());

        // A second time it is left exactly where somebody may have moved it.
        git(
            &dir.join("mounts/example"),
            &["checkout", "--quiet", &second],
        );
        check_sources_out(&dir, &config, &[example()], &|_| None, &|_| {}).unwrap();
        assert_eq!(head(&dir), second);

        // A branch that was asked for is what is checked out, whatever the image names.
        let on_branch = scratch("on-branch");
        check_sources_out(
            &on_branch,
            &config,
            &[example()],
            &|_| Some("feature".to_string()),
            &|_| {},
        )
        .unwrap();
        assert_eq!(head(&on_branch), third);
        let checkout = git::read_checkout(
            "example",
            &repo.to_string_lossy(),
            &on_branch.join("mounts/example"),
        );
        assert_eq!(checkout.branch.as_deref(), Some("feature"));

        // An image that names no commit gives the repository's own default branch.
        let unsaid = said_with(Some(Source {
            repository: repo.to_string_lossy().to_string(),
            revision: None,
            path: "/workspace".into(),
        }));
        let at_default = scratch("default");
        check_sources_out(
            &at_default,
            &hub(from_source(), &unsaid),
            &[example()],
            &|_| None,
            &|_| {},
        )
        .unwrap();
        assert_eq!(head(&at_default), second);

        // A commit the repository does not have is an error that says so, not a checkout
        // of something else.
        let wrong = said_with(Some(Source {
            repository: repo.to_string_lossy().to_string(),
            revision: Some("0000000000000000000000000000000000000000".into()),
            path: "/workspace".into(),
        }));
        let nowhere = scratch("nowhere");
        assert!(check_sources_out(
            &nowhere,
            &hub(from_source(), &wrong),
            &[example()],
            &|_| None,
            &|_| {}
        )
        .is_err());

        for dir in [repo, dir, on_branch, at_default, nowhere] {
            std::fs::remove_dir_all(dir).ok();
        }
    }
}
