//! Restoring a backup into a hub — the one this came from, or another.
//!
//! Two halves, and the first is the one that matters. [`plan`] reads the backup's
//! `manifest.json` and compares what was backed up with what the target hub runs: which
//! services, at which image tags, resolved to which builds; which Postgres; which storage
//! mode. A service in the backup that the target does not run is a database nobody will
//! serve, so it **blocks**; a service on a different tag or a different build of the same
//! tag is a **warning**, because a newer image migrates the schema forward on start and
//! usually that is exactly what is wanted — but it is not a thing to do to somebody
//! without telling them first. The front ends show the plan and ask.
//!
//! [`run`] then does it: stops the stack, replays the SQL dump (or copies `PGDATA` back
//! when asked, and only into the same Postgres major), copies the object storage back,
//! starts everything, and hands the result to [`crate::health`] — because the question
//! after a restore is not "did the files copy" but "do the services still work".

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::backup::{
    self, postgres_major, BackupEvent, BackupManifest, DataSource, DUMP_FILE, MANIFEST_FILE,
    MESH_DATA_DIR,
    MANIFEST_FORMAT, MINIO_DATA_DIR, POSTGRES_DATA_DIR,
};
use crate::catalog::ServiceId;
use crate::config::hub::{storage_mode_of, HubConfig, StorageMode, DB_COMPOSE_SERVICE};
use crate::health::{self, HealthEvent, ServiceHealth};
use crate::status::is_init_container;
use crate::{credentials, docker, profile};

#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    #[error("{0} is not a hub: no profile to read")]
    NotAHub(String),
    #[error("the profile could not be read: {0}")]
    Profile(String),
    #[error("{0} holds no {MANIFEST_FILE} — not a Konstruktor backup")]
    NoManifest(String),
    #[error("the manifest could not be read: {0}")]
    Manifest(String),
    #[error("this backup was written in format {0}; this version of Konstruktor reads format {MANIFEST_FORMAT}")]
    Format(u32),
    #[error("the restore cannot go ahead: {}", .0.join("; "))]
    Blocked(Vec<String>),
    #[error(transparent)]
    Backup(#[from] backup::BackupError),
    #[error("{0}")]
    Engine(String),
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
}

/// How the database is put back.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DbMethod {
    /// Replay `postgres/dump.sql` with `psql`. Works into any Postgres of the same or a
    /// newer major, and is the one to reach for.
    #[default]
    Dump,
    /// Copy `postgres/data` back over `PGDATA`. Byte-exact, and only valid into the same
    /// Postgres major — the plan refuses anything else.
    Raw,
}

#[derive(Debug, Clone)]
pub struct RestoreRequest {
    /// The hub to restore *into*.
    pub dir: PathBuf,
    /// The backup folder — the one holding `manifest.json`.
    pub backup: PathBuf,
    pub method: DbMethod,
    pub restore_postgres: bool,
    pub restore_minio: bool,
}

/// One line of narration. The same shape as a backup's, plus the health verdicts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum RestoreEvent {
    Step { step: String, title: String },
    Line { step: String, line: String, stderr: bool },
    Skipped { step: String, reason: String },
    Checked { service: String, healthy: bool, detail: String },
}

impl From<BackupEvent> for RestoreEvent {
    fn from(event: BackupEvent) -> Self {
        match event {
            BackupEvent::Step { step, title } => RestoreEvent::Step { step, title },
            BackupEvent::Line { step, line, stderr } => RestoreEvent::Line { step, line, stderr },
            BackupEvent::Skipped { step, reason } => RestoreEvent::Skipped { step, reason },
        }
    }
}

// --- the plan ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// Same tag, same build.
    Same,
    /// The target runs the service from a different tag.
    DifferentTag,
    /// Same tag, but it resolves to a different image on this machine.
    DifferentBuild,
    /// The target does not run this service at all. Blocks.
    MissingInTarget,
    /// The tags match and one side's build could not be resolved, so nothing more can
    /// be said.
    NotResolvable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceComparison {
    pub id: ServiceId,
    pub host: String,
    pub backup_image: String,
    pub backup_image_id: Option<String>,
    pub deployed_image: Option<String>,
    pub deployed_image_id: Option<String>,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageComparison {
    pub service: String,
    pub backup_image: String,
    pub deployed_image: String,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Available {
    pub dump: bool,
    pub postgres_raw: bool,
    pub minio: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorePlan {
    pub manifest: BackupManifest,
    /// The backup's hub identifier matches the target's. A restore into a different hub
    /// is allowed — it is how a hub is moved — but it should be known.
    pub same_hub: bool,
    pub target_identifier: Option<String>,
    pub target_storage: StorageMode,
    pub services: Vec<ServiceComparison>,
    /// Services the target runs that the backup has no data for.
    ///
    /// Harmless on a *fresh* target — they start empty, as they would on a new hub. On one
    /// that has been used they are the opposite of harmless: `pg_dumpall --clean` drops
    /// only the databases in the dump, so these keep their current data while everything
    /// else is replaced. [`judge`] warns for exactly that reason.
    pub extra_in_target: Vec<ServiceId>,
    pub db: ImageComparison,
    /// `postgres --version` of the target, when it could be asked.
    pub target_postgres_version: Option<String>,
    pub available: Available,
    /// Why this cannot go ahead as asked. Empty means it can.
    pub blocking: Vec<String>,
    /// What should be known before saying yes.
    pub warnings: Vec<String>,
}

pub fn read_manifest(backup: &Path) -> Result<BackupManifest, RestoreError> {
    let path = backup.join(MANIFEST_FILE);
    if !path.is_file() {
        return Err(RestoreError::NoManifest(backup.display().to_string()));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| RestoreError::Manifest(e.to_string()))?;
    let manifest: BackupManifest =
        serde_json::from_str(&text).map_err(|e| RestoreError::Manifest(e.to_string()))?;
    if manifest.format != MANIFEST_FORMAT {
        return Err(RestoreError::Format(manifest.format));
    }
    Ok(manifest)
}

/// What the folder holds, by looking rather than by trusting the manifest.
fn available_in(backup: &Path) -> Available {
    Available {
        dump: backup.join(DUMP_FILE).is_file(),
        postgres_raw: backup.join(POSTGRES_DATA_DIR).join("PG_VERSION").is_file(),
        minio: backup.join(MINIO_DATA_DIR).is_dir(),
    }
}

/// The comparison itself, pure so it can be tested without an engine.
///
/// `deployed_ids` maps a compose service (`rekuest`, `db`) to the image id its tag
/// resolves to on this machine right now; absent means it could not be resolved.
pub fn compare(
    manifest: &BackupManifest,
    target: &HubConfig,
    deployed_ids: &dyn Fn(&str) -> Option<String>,
) -> (Vec<ServiceComparison>, Vec<ServiceId>, ImageComparison) {
    let enabled = target.enabled_services();

    let services = manifest
        .services
        .iter()
        .map(|backed| {
            let deployed = enabled
                .contains(&backed.id)
                .then(|| target.service(backed.id))
                .and_then(|block| block.image.clone());
            let deployed_id = deployed.as_ref().and_then(|_| deployed_ids(&backed.host));
            let verdict = match &deployed {
                None => Verdict::MissingInTarget,
                Some(image) if image != &backed.image => Verdict::DifferentTag,
                Some(_) => match (&backed.image_id, &deployed_id) {
                    (Some(a), Some(b)) if a == b => Verdict::Same,
                    (Some(_), Some(_)) => Verdict::DifferentBuild,
                    _ => Verdict::NotResolvable,
                },
            };
            ServiceComparison {
                id: backed.id,
                host: backed.host.clone(),
                backup_image: backed.image.clone(),
                backup_image_id: backed.image_id.clone(),
                deployed_image: deployed,
                deployed_image_id: deployed_id,
                verdict,
            }
        })
        .collect();

    let extra = enabled
        .into_iter()
        .filter(|id| !manifest.services.iter().any(|s| s.id == *id))
        .collect();

    let backup_db = manifest
        .infrastructure
        .iter()
        .find(|i| i.service == DB_COMPOSE_SERVICE)
        .map(|i| i.image.clone())
        .unwrap_or_default();
    let db = ImageComparison {
        service: DB_COMPOSE_SERVICE.into(),
        backup_image: backup_db.clone(),
        deployed_image: target.db.image.clone(),
        verdict: if backup_db == target.db.image {
            Verdict::Same
        } else {
            Verdict::DifferentTag
        },
    };

    (services, extra, db)
}

/// The blocking reasons and warnings for a request, from a comparison that is already
/// made. Pure, for the same reason as [`compare`].
#[allow(clippy::too_many_arguments)]
/// The Postgres majors on the two sides of a restore, each resolved by whatever means was
/// available *without starting anything* — see [`plan`] for where each comes from.
///
/// `None` means "could not tell", which is never the same as "compatible".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostgresMajors {
    /// What wrote the data in the backup.
    pub backup: Option<u32>,
    /// What will serve it once it is restored.
    pub target: Option<u32>,
}

/// The two sides lined up against each other, as [`compare`] reports them.
pub struct Comparison {
    pub services: Vec<ServiceComparison>,
    /// Services the target runs that the backup has no data for.
    pub extra_in_target: Vec<ServiceId>,
    pub db: ImageComparison,
}

/// What the user asked for: which halves, and how the database should come back.
#[derive(Debug, Clone, Copy)]
pub struct Asked {
    pub method: DbMethod,
    pub postgres: bool,
    pub minio: bool,
}

pub fn judge(
    comparison: &Comparison,
    available: &Available,
    manifest: &BackupManifest,
    target: &HubConfig,
    majors: PostgresMajors,
    same_hub: bool,
    asked: Asked,
) -> (Vec<String>, Vec<String>) {
    let Comparison { services: plan_services, extra_in_target, db } = comparison;
    let Asked { method, postgres: restore_postgres, minio: restore_minio } = asked;
    let mut blocking = Vec::new();
    let mut warnings = Vec::new();
    let target_storage = storage_mode_of(target);

    // Every hub generates its own random Postgres role. A dump recreates the *backup's*
    // role and gives it ownership of everything it restores, and a raw copy replaces the
    // cluster wholesale — either way the services here go on connecting as this hub's
    // role, which then owns none of the restored databases. The manifest records the
    // other side's user precisely so this can be said out loud before it happens.
    if restore_postgres && manifest.postgres.user != target.db.postgres_user {
        warnings.push(format!(
            "the backup's database role ({}) is not this hub's ({}) — the restored \
             databases will be owned by a role these services do not connect as, and may \
             need their ownership reassigned afterwards",
            manifest.postgres.user, target.db.postgres_user
        ));
    }

    // The same skew one level down: a service whose database is named differently here
    // restores into a name nothing reads.
    let renamed: Vec<String> = manifest
        .services
        .iter()
        .filter(|backed| {
            target
                .enabled_services()
                .iter()
                .any(|id| *id == backed.id && target.service(*id).db_config.db != backed.db)
        })
        .map(|backed| {
            format!(
                "{} ({} in the backup, {} here)",
                backed.host,
                backed.db,
                target.service(backed.id).db_config.db
            )
        })
        .collect();
    if restore_postgres && !renamed.is_empty() {
        warnings.push(format!(
            "these services keep their data in a differently named database here: {} — the \
             backup's copy would be restored under the old name",
            renamed.join(", ")
        ));
    }

    // A service the target runs that the backup has no data for. `pg_dumpall --clean`
    // drops only the databases *in the dump*, so this one keeps whatever it has now while
    // everything around it is replaced — which on a hub that has been used is two eras of
    // data in one deployment, not the empty start a fresh target would get.
    if !extra_in_target.is_empty() {
        let names: Vec<&str> = extra_in_target.iter().map(|id| id.as_str()).collect();
        warnings.push(format!(
            "the backup holds no data for {} — {} will keep the data {} has now, while every \
             other service is replaced from the backup",
            names.join(", "),
            if names.len() == 1 { "it" } else { "they" },
            if names.len() == 1 { "it" } else { "they" },
        ));
    }

    for service in plan_services {
        match service.verdict {
            Verdict::MissingInTarget => blocking.push(format!(
                "the backup holds data for {}, which this hub does not run — its database \
                 would be restored with nothing to serve it",
                service.host
            )),
            Verdict::DifferentTag => warnings.push(format!(
                "{} runs {} here; the backup was taken from {}",
                service.host,
                service.deployed_image.as_deref().unwrap_or("?"),
                service.backup_image
            )),
            Verdict::DifferentBuild => warnings.push(format!(
                "{} is on the same tag ({}) but a different build than the backup was taken \
                 from — it may migrate the restored database forward on start",
                service.host, service.backup_image
            )),
            Verdict::Same | Verdict::NotResolvable => {}
        }
    }

    if !same_hub {
        warnings.push(
            "this backup was taken from a different hub; its data will replace this hub's"
                .into(),
        );
    }
    if manifest.storage != target_storage {
        warnings.push(format!(
            "the backup was taken from a hub using {}; this one uses {} — fine, the data is \
             copied into whichever it is",
            describe(manifest.storage),
            describe(target_storage)
        ));
    }

    if restore_postgres {
        match method {
            DbMethod::Dump if !available.dump => {
                blocking.push(format!("the backup holds no {DUMP_FILE} to replay"))
            }
            DbMethod::Raw if !available.postgres_raw => {
                blocking.push(format!("the backup holds no {POSTGRES_DATA_DIR} to copy back"))
            }
            DbMethod::Raw => {
                match backup::major_move(majors.backup, majors.target) {
                    backup::MajorMove::Across { data, server } => blocking.push(format!(
                        "a raw copy of PGDATA from Postgres {data} cannot be started by Postgres \
                         {server} — replay the dump instead"
                    )),
                    backup::MajorMove::Same(_) => {}
                    // Still possible — an image that sets no `PG_MAJOR`, a backup with no
                    // `PG_VERSION` — but no longer the ordinary case of a stopped hub.
                    backup::MajorMove::Unknown => warnings.push(
                        "the Postgres versions on the two sides could not both be read; a raw \
                         copy only works into the same major"
                            .into(),
                    ),
                }
                if db.verdict != Verdict::Same {
                    warnings.push(format!(
                        "the database image differs ({} here, {} in the backup)",
                        db.deployed_image, db.backup_image
                    ));
                }
            }
            DbMethod::Dump => {}
        }
    }
    if restore_minio && !available.minio {
        blocking.push(format!("the backup holds no {MINIO_DATA_DIR} to copy back"));
    }
    if !restore_postgres && !restore_minio {
        blocking.push("nothing was selected to restore".into());
    }

    // Half a restore leaves the two halves describing different moments: rows referring
    // to objects that were never put back, or objects nothing refers to. Every service
    // still answers, so the health check cannot notice — this is the only place it gets
    // said. Not blocking: doing one half deliberately is a legitimate thing to want.
    // The backup carries the hub's tailnet identity, and a restore deliberately does not
    // put it back: whether a node identity may reappear — here, or on another machine — is
    // a question about the tailnet, not about this folder. Say where it is, so it is a
    // decision rather than a surprise.
    if manifest.contents.mesh_copied {
        warnings.push(format!(
            "the backup holds this hub's mesh identity ({MESH_DATA_DIR}), which is not \
             restored — a hub that lost its tailnet state has to be authorized again for \
             a new key"
        ));
    }

    // Whatever the backup itself recorded as not having gone to plan. `backup.rs` writes
    // these carefully — "postgres/data was copied from a running server", say — and until
    // now nothing read them back.
    for note in &manifest.contents.warnings {
        warnings.push(format!("from the backup: {note}"));
    }

    match (restore_postgres, restore_minio) {
        (true, false) => warnings.push(
            "the object storage is being left as it is — the restored database may refer \
             to files this hub does not have"
                .into(),
        ),
        (false, true) => warnings.push(
            "the database is being left as it is — the restored files may be ones no row \
             in this hub refers to"
                .into(),
        ),
        _ => {}
    }

    (blocking, warnings)
}

fn describe(storage: StorageMode) -> &'static str {
    match storage {
        StorageMode::DockerVolumes => "Docker volumes",
        StorageMode::DeploymentFolder => "folders in the deployment",
    }
}

/// Whether a finished restore may be called healthy.
///
/// Two conditions, and the second is the one that is easy to forget: a dump replays under
/// `ON_ERROR_STOP=0`, so `psql` exits 0 even when statements failed — and `--clean` has
/// already dropped whatever they were meant to replace. Every service then answers
/// perfectly well over a half-restored database, so the health checks alone would report
/// success over data that is missing. The replay errors have to count against it.
pub fn restore_succeeded(health: &[crate::health::ServiceHealth], psql_errors: u32) -> bool {
    health.iter().all(|h| h.healthy) && psql_errors == 0
}

/// Reads both sides and compares them. Asks the engine for image ids and, when the
/// database is up, its version; neither failing stops the plan, it just knows less.
pub async fn plan(request: &RestoreRequest) -> Result<RestorePlan, RestoreError> {
    let dir = &request.dir;
    if !profile::holds_a_hub(dir) {
        return Err(RestoreError::NotAHub(dir.display().to_string()));
    }
    let config = profile::read_profile(dir)
        .map_err(|e| RestoreError::Profile(e.to_string()))?
        .config;
    let manifest = read_manifest(&request.backup)?;

    let (services, infra) = backup::images_of(&config);
    let mut asked: Vec<(String, String)> = services
        .iter()
        .map(|(_, host, image)| (host.clone(), image.clone()))
        .collect();
    asked.extend(infra);
    let states = docker::image_states(&asked).await.unwrap_or_default();
    let ids = |service: &str| {
        states
            .iter()
            .find(|s| s.service == service)
            .and_then(|s| s.image_id.clone())
    };

    let (services, extra_in_target, db) = compare(&manifest, &config, &ids);
    let target_identifier = credentials::read_credentials(dir).map(|c| c.identifier);
    let same_hub = match (&manifest.hub.identifier, &target_identifier) {
        (Some(a), Some(b)) => a == b,
        // Neither side authorized: the folder is the only identity there is.
        (None, None) => manifest.hub.path == dir.display().to_string(),
        _ => false,
    };
    let target_storage = storage_mode_of(&config);
    let available = available_in(&request.backup);

    // Asked of the running server only when it happens to be up — starting it just to ask
    // would be a side effect a *plan* must not have. It stays on the plan because the UI
    // shows it, but it is no longer what the raw-copy check depends on.
    let target_postgres_version =
        if backup::service_running(dir, DB_COMPOSE_SERVICE).await.unwrap_or(false) {
            backup::postgres_version(dir).await
        } else {
            None
        };

    // Both majors, resolved without starting anything. This is what makes the raw-copy
    // refusal fire for a stopped hub, which is the ordinary case and the one where
    // `copy_back`'s `rsync --delete` would otherwise destroy the target's data before
    // Postgres ever got the chance to refuse the directory.
    //
    // Backup side: the manifest if it recorded a version, else `PG_VERSION` — the file
    // `available_in` already stats to decide a raw copy is there at all.
    // Target side: the running server if there is one, else the `PG_MAJOR` the db image
    // declares. What matters is the server that will mount the directory.
    let majors = PostgresMajors {
        backup: manifest
            .postgres
            .server_version
            .as_deref()
            .and_then(postgres_major)
            .or_else(|| backup::backup_pgdata_major(&request.backup)),
        target: match target_postgres_version.as_deref().and_then(postgres_major) {
            Some(major) => Some(major),
            None => docker::image_pg_major(&config.db.image).await,
        },
    };

    let comparison = Comparison { services, extra_in_target, db };
    let (blocking, warnings) = judge(
        &comparison,
        &available,
        &manifest,
        &config,
        majors,
        same_hub,
        Asked {
            method: request.method,
            postgres: request.restore_postgres,
            minio: request.restore_minio,
        },
    );
    let Comparison { services, extra_in_target, db } = comparison;

    Ok(RestorePlan {
        manifest,
        same_hub,
        target_identifier,
        target_storage,
        services,
        extra_in_target,
        db,
        target_postgres_version,
        available,
        blocking,
        warnings,
    })
}

// --- the restore ------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreReport {
    pub path: String,
    pub backup: String,
    pub method: DbMethod,
    pub postgres_restored: bool,
    pub minio_restored: bool,
    /// `ERROR:` lines psql printed while replaying, minus the ones a `--clean` dump always
    /// produces. Zero is the ordinary outcome.
    pub psql_errors: u32,
    pub health: Vec<ServiceHealth>,
    pub all_healthy: bool,
    /// The stack was down before and has been left running for the check.
    pub left_running: bool,
    pub warnings: Vec<String>,
}

pub async fn run(
    request: &RestoreRequest,
    on_event: &(dyn Fn(RestoreEvent) + Send + Sync),
) -> Result<RestoreReport, RestoreError> {
    let dir = &request.dir;
    let forward = |event: BackupEvent| on_event(event.into());
    let step = |step: &str, title: &str| {
        on_event(RestoreEvent::Step {
            step: step.into(),
            title: title.into(),
        })
    };
    let line = |step: &str, line: &str| {
        on_event(RestoreEvent::Line {
            step: step.into(),
            line: line.into(),
            stderr: false,
        })
    };

    // --- 1. preflight ---------------------------------------------------------------
    step("preflight", "Checking the backup against this hub");
    let plan = plan(request).await?;
    if !plan.blocking.is_empty() {
        return Err(RestoreError::Blocked(plan.blocking));
    }
    for warning in &plan.warnings {
        line("preflight", warning);
    }
    let config = profile::read_profile(dir)
        .map_err(|e| RestoreError::Profile(e.to_string()))?
        .config;
    let mut warnings = plan.warnings.clone();

    // --- 2. stop --------------------------------------------------------------------
    step("stop", "Stopping the hub");
    let containers = docker::list_deployment_containers(&dir.to_string_lossy())
        .await
        .map_err(RestoreError::Engine)?;
    let was_running = containers
        .iter()
        .any(|c| !is_init_container(c) && c.state.as_deref() == Some("running"));
    backup::compose_streamed(dir, &["stop"], "stop", &forward).await?;

    // --- 3. the volumes have to exist before anything is copied into them ------------
    step("volumes", "Making sure the data volumes exist");
    backup::compose_streamed(
        dir,
        &["up", "--no-start", "--no-deps", DB_COMPOSE_SERVICE, &config.minio.host],
        "volumes",
        &forward,
    )
    .await?;

    let mut report = RestoreReport {
        path: dir.display().to_string(),
        backup: request.backup.display().to_string(),
        method: request.method,
        postgres_restored: false,
        minio_restored: false,
        psql_errors: 0,
        health: Vec::new(),
        all_healthy: false,
        left_running: !was_running,
        warnings: Vec::new(),
    };

    // --- 4. postgres ------------------------------------------------------------------
    if request.restore_postgres {
        match request.method {
            DbMethod::Dump => {
                step("postgres", "Replaying the database dump");
                backup::compose_streamed(
                    dir,
                    &["up", "-d", "--no-deps", DB_COMPOSE_SERVICE],
                    "postgres",
                    &forward,
                )
                .await?;
                backup::wait_for_database(dir, &config, "postgres", &forward).await?;
                report.psql_errors =
                    replay_dump(dir, &config, &request.backup.join(DUMP_FILE), on_event).await?;
                if report.psql_errors > 0 {
                    warnings.push(format!(
                        "psql reported {} error(s) while replaying the dump — see the log",
                        report.psql_errors
                    ));
                }
                backup::compose_streamed(dir, &["stop", DB_COMPOSE_SERVICE], "postgres", &forward)
                    .await?;
            }
            DbMethod::Raw => {
                step("postgres", "Copying the database files back");
                copy_back(
                    dir,
                    &request.backup.join(POSTGRES_DATA_DIR),
                    backup::source_of(dir, config.db.mount.as_deref(), &config.db.volume_name),
                    "postgres",
                    &forward,
                )
                .await?;
            }
        }
        report.postgres_restored = true;
    } else {
        on_event(RestoreEvent::Skipped {
            step: "postgres".into(),
            reason: "not selected".into(),
        });
    }

    // --- 5. minio ---------------------------------------------------------------------
    if request.restore_minio {
        step("minio", "Copying the object storage back");
        copy_back(
            dir,
            &request.backup.join(MINIO_DATA_DIR),
            backup::source_of(dir, config.minio.mount.as_deref(), &config.minio.volume_name),
            "minio",
            &forward,
        )
        .await?;
        report.minio_restored = true;
    } else {
        on_event(RestoreEvent::Skipped {
            step: "minio".into(),
            reason: "not selected".into(),
        });
    }

    // --- 6. start -----------------------------------------------------------------------
    step("start", "Starting the hub");
    backup::compose_streamed(dir, &["up", "-d"], "start", &forward).await?;

    // --- 7. and see whether it works --------------------------------------------------
    step("health", "Checking that the services answer");
    report.health = health::check(dir, &config, &|event| match event {
        HealthEvent::Line { line } => on_event(RestoreEvent::Line {
            step: "health".into(),
            line,
            stderr: false,
        }),
        HealthEvent::Checked { service, healthy, detail } => {
            on_event(RestoreEvent::Checked { service, healthy, detail })
        }
    })
    .await
    .map_err(RestoreError::Engine)?;
    let replay_failed = report.psql_errors > 0;
    report.all_healthy = restore_succeeded(&report.health, report.psql_errors);
    if replay_failed {
        warnings.push(format!(
            "{} statement(s) failed while replaying the dump — the database is only \
             partly restored, whatever the services report",
            report.psql_errors
        ));
    }
    let failing: Vec<&str> = report
        .health
        .iter()
        .filter(|h| !h.healthy)
        .map(|h| h.service.as_str())
        .collect();
    // Only when there is something to name — a restore that failed *only* its replay has
    // every service answering, and "not answering: " with nothing after it is noise.
    if !failing.is_empty() {
        warnings.push(format!("not answering: {}", failing.join(", ")));
    }
    if !was_running {
        warnings.push("The hub was stopped before; it has been left running.".into());
    }
    report.warnings = warnings;

    Ok(report)
}

/// The messages `psql` prints for a `pg_dumpall --clean` replay that are not problems:
/// the dump drops and recreates the very role it is connected as, which Postgres refuses,
/// and then finds it already there.
fn is_expected_psql_error(line: &str) -> bool {
    line.contains("current user cannot be dropped")
        || line.contains("cannot drop role")
        || line.contains("already exists")
}

/// Counts the `ERROR:` lines that are not expected. Exposed for the tests.
pub fn count_psql_errors<'a>(lines: impl IntoIterator<Item = &'a str>) -> u32 {
    lines
        .into_iter()
        .filter(|l| l.contains("ERROR:") && !is_expected_psql_error(l))
        .count() as u32
}

/// `psql` through `compose exec`, with the dump on stdin — the file can be gigabytes and
/// is streamed straight through.
async fn replay_dump(
    dir: &Path,
    config: &HubConfig,
    dump: &Path,
    on_event: &(dyn Fn(RestoreEvent) + Send + Sync),
) -> Result<u32, RestoreError> {
    let io = |path: &Path, source: std::io::Error| RestoreError::Io {
        path: path.display().to_string(),
        source,
    };

    let mut child = crate::engine_probe::engine()
        .async_command()
        .args([
            "compose",
            "exec",
            "-T",
            DB_COMPOSE_SERVICE,
            "psql",
            "-U",
            &config.db.postgres_user,
            "-d",
            "postgres",
            "-v",
            "ON_ERROR_STOP=0",
            "-q",
            "-f",
            "-",
        ])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| RestoreError::Engine(format!("docker compose exec: {e}")))?;

    let mut stdin = child.stdin.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let mut file = tokio::fs::File::open(dump).await.map_err(|e| io(dump, e))?;

    let feed = async move {
        let copied = tokio::io::copy(&mut file, &mut stdin).await;
        drop(stdin);
        copied
    };
    let errors = async {
        let mut count = 0u32;
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(raw)) = lines.next_line().await {
            if raw.trim().is_empty() {
                continue;
            }
            if raw.contains("ERROR:") && !is_expected_psql_error(&raw) {
                count += 1;
            }
            on_event(RestoreEvent::Line {
                step: "postgres".into(),
                line: raw,
                stderr: true,
            });
        }
        count
    };

    let (copied, count) = tokio::join!(feed, errors);
    let bytes = copied.map_err(|e| io(dump, e))?;
    let status = child
        .wait()
        .await
        .map_err(|e| RestoreError::Engine(format!("docker compose exec: {e}")))?;
    if !status.success() {
        return Err(RestoreError::Engine(format!(
            "psql exited with {status} after {bytes} bytes"
        )));
    }
    on_event(RestoreEvent::Line {
        step: "postgres".into(),
        line: format!("Replayed {bytes} bytes of SQL"),
        stderr: false,
    });
    Ok(count)
}

/// A backup directory back into a data mount, with the container stopped.
async fn copy_back(
    dir: &Path,
    from: &Path,
    into: DataSource,
    step: &str,
    on_event: &(dyn Fn(BackupEvent) + Send + Sync),
) -> Result<(), RestoreError> {
    let from = std::fs::canonicalize(from)
        .map_err(|source| RestoreError::Io {
            path: from.display().to_string(),
            source,
        })?
        .to_string_lossy()
        .to_string();

    let target = match &into {
        DataSource::Bind(path) => {
            std::fs::create_dir_all(path).map_err(|source| RestoreError::Io {
                path: path.display().to_string(),
                source,
            })?;
            std::fs::canonicalize(path)
                .unwrap_or_else(|_| path.clone())
                .to_string_lossy()
                .to_string()
        }
        DataSource::Volume(_) => backup::resolve_source(dir, &into)
            .await?
            .ok_or_else(|| RestoreError::Engine("the data volume does not exist".into()))?,
    };

    backup::rsync(
        &format!("{from}:/source:ro"),
        &format!("{target}:/target"),
        step,
        on_event,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::{BackupContents, ManifestHub, ManifestImage, ManifestPostgres, ManifestService};
    use crate::config::hub::{build_hub_config, HubConfigOptions};

    /// The three halves `compare` returns, as `judge` wants them.
    fn comparison(
        services: Vec<ServiceComparison>,
        extra: Vec<ServiceId>,
        db: ImageComparison,
    ) -> Comparison {
        Comparison { services, extra_in_target: extra, db }
    }

    fn asked(method: DbMethod, postgres: bool, minio: bool) -> Asked {
        Asked { method, postgres, minio }
    }

    fn hub(services: Vec<ServiceId>, storage: StorageMode) -> HubConfig {
        build_hub_config(&HubConfigOptions {
            device_id: "device".into(),
            coord_server: "go.arkitekt.live".into(),
            services: Some(services),
            storage,
            ..Default::default()
        })
    }

    fn manifest_of(config: &HubConfig, ids: &dyn Fn(&str) -> Option<String>) -> BackupManifest {
        let (services, infra) = backup::images_of(config);
        BackupManifest {
            format: MANIFEST_FORMAT,
            konstruktor_version: "test".into(),
            taken_at: 0,
            storage: storage_mode_of(config),
            hub: ManifestHub {
                identifier: Some("lab-hub".into()),
                coord_server: config.coord_server.clone(),
                project: "hub".into(),
                path: "/hubs/hub".into(),
            },
            services: services
                .iter()
                .map(|(id, host, image)| ManifestService {
                    id: *id,
                    host: host.clone(),
                    image: image.clone(),
                    image_id: ids(host),
                    repo_digests: vec![],
                    db: config.service(*id).db_config.db.clone(),
                })
                .collect(),
            infrastructure: infra
                .iter()
                .map(|(service, image)| ManifestImage {
                    service: service.clone(),
                    image: image.clone(),
                    image_id: ids(service),
                })
                .collect(),
            postgres: ManifestPostgres {
                user: config.db.postgres_user.clone(),
                server_version: Some("postgres (PostgreSQL) 16.2".into()),
            },
            contents: BackupContents::default(),
        }
    }

    #[test]
    fn a_service_the_target_does_not_run_blocks_and_an_extra_one_does_not() {
        let backed = hub(vec![ServiceId::Rekuest, ServiceId::Mikro], StorageMode::DockerVolumes);
        let target = hub(vec![ServiceId::Rekuest, ServiceId::Fluss], StorageMode::DockerVolumes);
        let ids = |_: &str| Some("sha256:abc".to_string());
        let manifest = manifest_of(&backed, &ids);

        let (services, extra, db) = compare(&manifest, &target, &ids);
        let mikro = services.iter().find(|s| s.id == ServiceId::Mikro).unwrap();
        assert_eq!(mikro.verdict, Verdict::MissingInTarget);
        let rekuest = services.iter().find(|s| s.id == ServiceId::Rekuest).unwrap();
        assert_eq!(rekuest.verdict, Verdict::Same);
        assert_eq!(extra, vec![ServiceId::Fluss]);
        assert_eq!(db.verdict, Verdict::Same);

        let available = Available { dump: true, postgres_raw: true, minio: true };
        let (blocking, warnings) = judge(
            &comparison(services.clone(), extra.clone(), db.clone()), &available, &manifest,
            &target, PostgresMajors::default(), true, asked(DbMethod::Dump, true, true),
        );
        assert_eq!(blocking.len(), 1, "{blocking:?}");
        assert!(blocking[0].contains("mikro"));

        // Two separately generated hubs have two different random Postgres roles, so this
        // is also the cross-machine case: the restored databases would end up owned by a
        // role these services do not connect as.
        assert!(
            warnings.iter().any(|w| w.contains("database role")),
            "{warnings:?}"
        );

        // The extra service does not block — a restore into a hub that runs more than the
        // backup is legitimate — but it is no longer silent: fluss keeps its own data
        // while everything else is replaced, and that is worth being told.
        assert!(
            warnings.iter().any(|w| w.contains("fluss") && w.contains("keep the data")),
            "{warnings:?}"
        );
    }

    /// A fresh target has nothing to keep, so the warning is only as loud as it needs to
    /// be: it fires on the list, not on the hub's history, which the core cannot see.
    #[test]
    fn no_extra_services_means_no_warning_about_them() {
        let config = hub(vec![ServiceId::Rekuest], StorageMode::DockerVolumes);
        let manifest = manifest_of(&config, &|_| Some("sha256:abc".into()));
        let (services, extra, db) = compare(&manifest, &config, &|_| Some("sha256:abc".into()));
        assert!(extra.is_empty());

        let available = Available { dump: true, postgres_raw: true, minio: true };
        let (_, warnings) = judge(
            &comparison(services.clone(), extra.clone(), db.clone()), &available, &manifest,
            &config, PostgresMajors::default(), true, asked(DbMethod::Dump, true, true),
        );
        assert!(!warnings.iter().any(|w| w.contains("keep the data")), "{warnings:?}");
    }

    #[test]
    fn a_different_build_of_the_same_tag_is_a_warning_not_a_block() {
        let config = hub(vec![ServiceId::Rekuest], StorageMode::DockerVolumes);
        let manifest = manifest_of(&config, &|_| Some("sha256:old".into()));
        let (services, _, db) =
            compare(&manifest, &config, &|_| Some("sha256:new".to_string()));
        assert_eq!(services[0].verdict, Verdict::DifferentBuild);

        let available = Available { dump: true, postgres_raw: true, minio: true };
        let (blocking, warnings) = judge(
            &comparison(services.clone(), vec![], db.clone()), &available, &manifest,
            &config, PostgresMajors::default(), true, asked(DbMethod::Dump, true, true),
        );
        assert!(blocking.is_empty(), "{blocking:?}");
        assert!(warnings.iter().any(|w| w.contains("different build")));
    }

    #[test]
    fn a_raw_copy_across_postgres_majors_is_refused_and_a_missing_dump_too() {
        let config = hub(vec![ServiceId::Rekuest], StorageMode::DockerVolumes);
        let manifest = manifest_of(&config, &|_| None);
        let (services, _, db) = compare(&manifest, &config, &|_| None);
        assert_eq!(services[0].verdict, Verdict::NotResolvable);

        let available = Available { dump: false, postgres_raw: true, minio: false };
        let (blocking, _) = judge(
            &comparison(services.clone(), vec![], db.clone()), &available, &manifest, &config,
            // The backup was written by 16 (the manifest's own version) and the target
            // would serve it with 15 — the case that has to be refused.
            PostgresMajors { backup: Some(16), target: Some(15) },
            false, asked(DbMethod::Raw, true, false),
        );
        assert!(blocking.iter().any(|b| b.contains("Postgres 16")), "{blocking:?}");

        // The backup was taken from a volumes hub; this target keeps its data in the
        // folder, which is the storage-mode skew worth mentioning.
        let folder_target = hub(vec![ServiceId::Rekuest], StorageMode::DeploymentFolder);
        let (blocking, warnings) = judge(
            &comparison(services.clone(), vec![], db.clone()), &available, &manifest,
            &folder_target, PostgresMajors::default(), false, asked(DbMethod::Dump, true, true),
        );
        assert!(blocking.iter().any(|b| b.contains("dump.sql")));
        assert!(blocking.iter().any(|b| b.contains("minio")));
        assert!(warnings.iter().any(|w| w.contains("different hub")));
        assert!(warnings.iter().any(|w| w.contains("Docker volumes")));
    }

    /// The case that used to slip through: a stopped hub gave no running server, the
    /// majors matched on `_`, and the refusal became a warning while `copy_back` went on
    /// to rsync `--delete` over PGDATA. Resolving the target from the image closes it.
    #[test]
    fn a_raw_copy_is_refused_even_when_the_hub_is_stopped() {
        let config = hub(vec![ServiceId::Rekuest], StorageMode::DeploymentFolder);
        let manifest = manifest_of(&config, &|_| None);
        let (services, _, db) = compare(&manifest, &config, &|_| None);
        let available = Available { dump: false, postgres_raw: true, minio: false };

        // No running server to ask — the target major came from the image instead.
        let (blocking, _) = judge(
            &comparison(services.clone(), vec![], db.clone()), &available, &manifest, &config,
            PostgresMajors { backup: Some(15), target: Some(16) },
            true, asked(DbMethod::Raw, true, false),
        );
        assert!(
            blocking.iter().any(|b| b.contains("Postgres 15") && b.contains("Postgres 16")),
            "{blocking:?}"
        );

        // Same majors is fine, and must not warn about being unable to read them.
        let (blocking, warnings) = judge(
            &comparison(services.clone(), vec![], db.clone()), &available, &manifest, &config,
            PostgresMajors { backup: Some(16), target: Some(16) },
            true, asked(DbMethod::Raw, true, false),
        );
        assert!(blocking.is_empty(), "{blocking:?}");
        assert!(!warnings.iter().any(|w| w.contains("could not both be read")), "{warnings:?}");
    }

    /// An image that declares no major must not read as agreement.
    #[test]
    fn an_unreadable_major_warns_rather_than_passing() {
        let config = hub(vec![ServiceId::Rekuest], StorageMode::DeploymentFolder);
        let manifest = manifest_of(&config, &|_| None);
        let (services, _, db) = compare(&manifest, &config, &|_| None);
        let available = Available { dump: false, postgres_raw: true, minio: false };

        let (blocking, warnings) = judge(
            &comparison(services.clone(), vec![], db.clone()), &available, &manifest, &config,
            PostgresMajors { backup: Some(16), target: None },
            true, asked(DbMethod::Raw, true, false),
        );
        assert!(blocking.is_empty(), "{blocking:?}");
        assert!(warnings.iter().any(|w| w.contains("could not both be read")), "{warnings:?}");
    }

    /// Half a restore is allowed, but it must not be silent: the two halves then describe
    /// different moments and nothing downstream can tell.
    #[test]
    fn restoring_only_one_half_warns_about_the_other() {
        let config = hub(vec![ServiceId::Rekuest], StorageMode::DockerVolumes);
        let manifest = manifest_of(&config, &|_| Some("sha256:abc".into()));
        let (services, extra, db) = compare(&manifest, &config, &|_| Some("sha256:abc".into()));
        let available = Available { dump: true, postgres_raw: true, minio: true };

        let judge_with = |postgres, minio| {
            judge(
                &comparison(services.clone(), extra.clone(), db.clone()), &available, &manifest,
                &config, PostgresMajors::default(), true, asked(DbMethod::Dump, postgres, minio),
            )
        };

        let (blocking, warnings) = judge_with(true, false);
        assert!(blocking.is_empty(), "{blocking:?}");
        assert!(warnings.iter().any(|w| w.contains("object storage is being left")), "{warnings:?}");

        let (blocking, warnings) = judge_with(false, true);
        assert!(blocking.is_empty(), "{blocking:?}");
        assert!(warnings.iter().any(|w| w.contains("database is being left")), "{warnings:?}");

        // Both halves is the whole thing, and says nothing about either.
        let (_, warnings) = judge_with(true, true);
        assert!(!warnings.iter().any(|w| w.contains("being left as it is")), "{warnings:?}");

        // Neither is still refused outright.
        let (blocking, _) = judge_with(false, false);
        assert!(blocking.iter().any(|b| b.contains("nothing was selected")), "{blocking:?}");
    }

    /// The health checks alone are not enough: they pass over a half-restored database.
    #[test]
    fn a_restore_with_replay_errors_is_not_healthy() {
        let ok = |service: &str| crate::health::ServiceHealth {
            service: service.into(),
            container_state: Some("running".into()),
            restarts_seen: false,
            http_status: Some(200),
            url: None,
            detail: "answers".into(),
            healthy: true,
        };
        let bad = crate::health::ServiceHealth { healthy: false, ..ok("mikro") };

        assert!(restore_succeeded(&[ok("rekuest"), ok("mikro")], 0));
        // Every service answers, but statements failed on the way in.
        assert!(!restore_succeeded(&[ok("rekuest"), ok("mikro")], 1));
        // And the older condition still holds on its own.
        assert!(!restore_succeeded(&[ok("rekuest"), bad], 0));
        assert!(!restore_succeeded(&[], 3));
        assert!(restore_succeeded(&[], 0));
    }

    #[test]
    fn expected_psql_noise_is_not_counted() {
        let lines = [
            "ERROR:  current user cannot be dropped",
            "ERROR:  role \"someone\" already exists",
            "ERROR:  relation \"x\" does not exist",
            "NOTICE:  database \"rekuest\" does not exist, skipping",
        ];
        assert_eq!(count_psql_errors(lines), 1);
    }

    #[test]
    fn a_manifest_round_trips_and_an_unknown_format_is_refused() {
        let dir = std::env::temp_dir().join(format!(
            "konstruktor-restore-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(matches!(read_manifest(&dir), Err(RestoreError::NoManifest(_))));

        let config = hub(vec![ServiceId::Rekuest], StorageMode::DockerVolumes);
        let mut manifest = manifest_of(&config, &|_| None);
        std::fs::write(dir.join(MANIFEST_FILE), serde_json::to_string(&manifest).unwrap()).unwrap();
        let back = read_manifest(&dir).unwrap();
        assert_eq!(back.services[0].id, ServiceId::Rekuest);
        assert_eq!(back.hub.identifier.as_deref(), Some("lab-hub"));

        manifest.format = 99;
        std::fs::write(dir.join(MANIFEST_FILE), serde_json::to_string(&manifest).unwrap()).unwrap();
        assert!(matches!(read_manifest(&dir), Err(RestoreError::Format(99))));
        std::fs::remove_dir_all(&dir).ok();
    }
}
