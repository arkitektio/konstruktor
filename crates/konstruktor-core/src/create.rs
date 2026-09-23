use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::catalog::ServiceId;
use crate::config::hub::{
    build_hub_config, HubConfig, HubConfigOptions, ServiceOptions, StorageMode,
};
use crate::config::mesh::{build_mesh_block, mesh_hostname, MeshOptions};
use crate::connect::authorize::{self, HubAuthorizationError};
use crate::connect::manifest::{build_hub_request, AdvertisedHost, HubManifestOptions};
use crate::credentials::{write_credentials, HubCredentials};
use crate::generate::generate_hub_files;
use crate::generate::write::write_generated_files;
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
    #[error(
        "The deployment is written and registered, but the source checkout failed: \
             {0}. Fix the checkout under `mounts/` and start it as usual."
    )]
    Clone(#[from] crate::git::CloneError),
}

pub struct CreatedHub {
    pub path: PathBuf,
    pub config: HubConfig,
    pub credentials: HubCredentials,
    /// Whether a mesh key was actually granted.
    pub mesh_granted: bool,
}

/// Build → authorize → write → start, in that order and only that order.
///
/// The ordering is load-bearing. `build_hub_config` mints fresh secrets and a fresh
/// Ed25519 pair, so it runs exactly once: calling it again to fold in a mesh key would
/// describe a different hub than the one the coordination server accepted.
pub async fn create_hub(
    answers: &HubAnswers,
    cancel: &CancellationToken,
    on: &(dyn Fn(CreateEvent) + Sync),
) -> Result<CreatedHub, CreateError> {
    validate_identifier(&answers.identifier)?;
    validate_service_options(&answers.service_options)?;
    if answers.mesh_only && answers.mesh_mode == MeshMode::None {
        return Err(CreateError::Answers(
            "A mesh-only hub needs a mesh — choose a way to join one, or advertise \
             addresses on this network instead."
                .into(),
        ));
    }
    if answers.mesh_mode == MeshMode::Manual
        && answers.mesh_auth_key.as_deref().map(str::trim).unwrap_or("").is_empty()
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
    } else {
        (Some(answers.http_port), Some(answers.https_port))
    };

    // Mesh-only advertises nothing on this machine's networks, whatever was picked.
    let hosts = if answers.mesh_only {
        Vec::new()
    } else {
        answers.hosts.clone()
    };

    let config = build_hub_config(&HubConfigOptions {
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
        dev_hub: answers.dev_hub,
        service_options: answers.service_options.clone(),
        storage: answers.storage,
        ..Default::default()
    });

    // --- authorize ----------------------------------------------------------
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
            request_auth_key: answers.mesh_mode == MeshMode::Coordination,
            // Declared up front even for a key that is only about to be minted: the
            // server resolves it against whichever node registers with that key.
            mesh_alias: answers.mesh_mode != MeshMode::None,
            // Every hub: plugins an attached engine starts sit on the hub's network and
            // reach it only as `gateway`. Scope local, so clients elsewhere skip it.
            internal_host: Some(config.gateway.host.clone()),
            expiration_seconds: None,
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
    let mut config = config;
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

    // --- write --------------------------------------------------------------
    let credentials = HubCredentials {
        version: 1,
        server: answers.coord_server.trim().to_string(),
        identifier: answers.identifier.trim().to_string(),
        authorized_at: now_rfc3339(),
        issuer: grant.issuer.clone(),
        envelope: envelope.clone(),
        advertised_hosts: hosts,
    };

    let files = generate_hub_files(&config, &credentials.issued_identity());
    for name in files.keys() {
        on(CreateEvent::Writing { file: name.clone() });
    }

    write_profile(&dir, &hub_profile(config.clone()))
        .map_err(|e| CreateError::Write(std::io::Error::other(e.to_string())))?;
    write_credentials(&dir, &credentials)?;
    write_generated_files(&dir, &files)?;

    // --- register, so the desktop app sees it -------------------------------
    registry::register(
        &mut store,
        &answers.name,
        &dir.to_string_lossy(),
        Some(credentials.server.clone()),
        Some(credentials.identifier.clone()),
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

        // Driven by the profile, not by the answers: `mount_github` is where both
        // `--dev` and a single service's "run from source" end up, and it is the field
        // the compose file's bind mounts were written from. Cloning anything else — or
        // less — is how a container gets handed an empty workspace.
        for id in config
            .enabled_services()
            .into_iter()
            .filter(|id| config.service(*id).mount_github)
        {
            let service = config.service(id);
            let branch = answers
                .service_options
                .get(&id)
                .and_then(|asked| asked.branch.as_deref())
                .map(str::trim)
                .filter(|b| !b.is_empty())
                .or(fallback);
            let into = git::checkout_dir(&dir, &service.host);
            std::fs::create_dir_all(&into)?;

            on(CreateEvent::Cloning {
                service: service.host.clone(),
                repo: service.github_repo.clone(),
                branch: branch.map(str::to_string),
            });

            let cloned = git::clone_service(&service.host, &service.github_repo, branch, &into)?;

            // The config is bind-mounted at `/workspace/config.yaml`, which is *inside*
            // the checkout. Docker creates a missing mount point itself, as root — so
            // the file is created here instead, owned by whoever owns the checkout.
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

/// The files a hub with these answers would be written to, without writing any of them.
///
/// Built from a throwaway config with a placeholder identity: the *names* do not depend on
/// the keys, and asking a coordination server for real ones is exactly what a preview must
/// not do.
pub fn preview_files(answers: &HubAnswers) -> Vec<String> {
    let config = crate::config::hub::build_hub_config(&crate::config::hub::HubConfigOptions {
        coord_server: answers.coord_server.clone(),
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
    crate::generate::generate_hub_files(&config, &crate::generate::IssuedIdentity::default())
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
        if let Some(branch) = asked.branch.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
            if branch.chars().any(|c| c.is_whitespace() || "~^:?*[\\".contains(c)) {
                return Err(CreateError::Answers(format!(
                    "`{branch}` is not a branch name git would accept (for {})",
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

/// Re-authorizing a hub that already exists on disk.
///
/// This is how a hub gains services, moves to a different network, or joins the mesh it
/// was created without. The profile is reused verbatim — its secrets and provenance key
/// are what the running services already trust — and only the manifest is sent again.
///
/// The service configs are then regenerated, because the JWKS URL the coordination server
/// returns is what they verify inbound tokens against, and it may have moved.
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
}

pub async fn reauthorize(
    answers: &ReauthorizeAnswers,
    cancel: &CancellationToken,
    on: &(dyn Fn(CreateEvent) + Sync),
) -> Result<Reauthorized, CreateError> {
    let profile = crate::profile::read_profile(&answers.dir)
        .map_err(|e| CreateError::Folder(e.to_string()))?;
    let mut config = profile.config;
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
    let files = generate_hub_files(&config, &credentials.issued_identity());
    for name in files.keys() {
        on(CreateEvent::Writing { file: name.clone() });
    }

    write_profile(&answers.dir, &hub_profile(config))
        .map_err(|e| CreateError::Write(std::io::Error::other(e.to_string())))?;
    write_credentials(&answers.dir, &credentials)?;
    write_generated_files(&answers.dir, &files)?;

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
    })
}
