//! Backing a hub's data up into a folder of the user's choosing.
//!
//! A hub's state is in three places, and a backup that misses one is not a backup:
//!
//! * **The database.** Taken twice, on purpose. `pg_dumpall` produces the copy that can
//!   be restored into any Postgres of the same or a newer major — it is the one to reach
//!   for. The raw `PGDATA` directory is copied as well, because a dump is only as good
//!   as the `pg_dumpall` that made it, and a byte copy is the fallback when it is not.
//!   The raw copy is taken from a live server when the hub is up, so it is a crash-
//!   consistent snapshot at best; the dump is the consistent one.
//! * **The object storage** — every image, file and artifact a service stored. Copied
//!   as it is.
//! * **The deployment itself**: the profile, the credentials, the generated configs and
//!   the compose file. Small, and without them the data cannot be told what it belongs to.
//!
//! The copying runs **inside a container**, not on the host. That is not a preference:
//! with the default storage the data is in a named volume that the host cannot see at
//! all, and even a bind mount is owned by root on Linux. A throwaway container that
//! mounts the source read-only and the backup folder read-write, and runs `rsync` between
//! them, is the one method that works for both storage modes on every engine.
//!
//! Every backup carries a `manifest.json` saying what it is a backup *of*: which services
//! the hub ran, at which image tags and resolved image ids, which Postgres, which storage
//! mode. Restoring (`crate::restore`) reads it back and compares it with the hub it is
//! asked to restore into, so a dump taken from one set of services is never replayed
//! silently into another.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::catalog::ServiceId;
use crate::config::hub::{storage_mode_of, HubConfig, StorageMode, DB_COMPOSE_SERVICE};
use crate::credentials::{self, CREDENTIALS_FILENAME};
use crate::{docker, engine_probe};
use crate::profile::{self, HUB_CONFIG_FILENAME};

/// Alpine with `rsync` on it, and nothing else — small enough that pulling it during a
/// backup is not the slow part. Pinned by tag; a backup tool that changes under people is
/// worse than one that is a version behind.
pub const RSYNC_IMAGE: &str = "instrumentisto/rsync-ssh:alpine";

/// How long to wait for a database that was just started to accept connections.
const DB_READY_TIMEOUT: Duration = Duration::from_secs(60);

/// Where each part lands, relative to the backup folder.
pub const DUMP_FILE: &str = "postgres/dump.sql";
pub const POSTGRES_DATA_DIR: &str = "postgres/data";
pub const MINIO_DATA_DIR: &str = "minio/data";
pub const DEPLOYMENT_DIR: &str = "deployment";
pub const MANIFEST_FILE: &str = "manifest.json";
/// Bumped when the manifest's shape changes in a way an older restore could misread.
pub const MANIFEST_FORMAT: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("{0} is not a hub: no profile to read")]
    NotAHub(String),
    #[error("the profile could not be read: {0}")]
    Profile(String),
    #[error("the backup folder could not be created at {path}: {source}")]
    Target {
        path: String,
        source: std::io::Error,
    },
    #[error("the backup folder must not be inside the deployment's own data")]
    TargetInsideData,
    #[error("{engine} could not be run: {source}")]
    Engine {
        engine: &'static str,
        source: std::io::Error,
    },
    #[error("{step} failed: {detail}")]
    Step { step: &'static str, detail: String },
    #[error("the database did not become ready within {0} seconds")]
    DatabaseNotReady(u64),
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
}

/// One line of narration, as the backup progresses. The `step` names which of the parts
/// it belongs to, so a front end can show progress per part rather than a wall of text.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum BackupEvent {
    /// A part is starting.
    Step { step: String, title: String },
    /// A line of output from whatever the step is running.
    Line { step: String, line: String, stderr: bool },
    /// A part was skipped, and why. Not a failure: a hub that never started has no
    /// volume to copy, and saying so beats copying nothing in silence.
    Skipped { step: String, reason: String },
}

/// `manifest.json`: what this is a backup of. Read back by `restore::plan`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub format: u32,
    pub konstruktor_version: String,
    pub taken_at: u64,
    pub storage: StorageMode,
    pub hub: ManifestHub,
    /// The enabled services, with the image each ran from.
    pub services: Vec<ManifestService>,
    /// The database, object storage, redis, gateway and the rest.
    pub infrastructure: Vec<ManifestImage>,
    pub postgres: ManifestPostgres,
    pub contents: BackupContents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestHub {
    /// The identifier on the coordination server, when the hub was authorized.
    pub identifier: Option<String>,
    pub coord_server: String,
    /// The compose project name — what the volumes are prefixed with.
    pub project: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestService {
    pub id: ServiceId,
    pub host: String,
    /// The tag, as the compose file names it.
    pub image: String,
    /// What that tag resolved to on this machine when the backup was taken. Two hubs on
    /// the same tag can run different builds; this is what tells them apart.
    pub image_id: Option<String>,
    #[serde(default)]
    pub repo_digests: Vec<String>,
    /// The database this service owns inside the dump.
    pub db: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestImage {
    pub service: String,
    pub image: String,
    pub image_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestPostgres {
    pub user: String,
    /// `postgres --version` inside the container, e.g. `postgres (PostgreSQL) 16.2`. A raw
    /// copy of `PGDATA` is only valid into the same major.
    pub server_version: Option<String>,
}

/// Which parts the folder actually holds.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackupContents {
    pub dumped: bool,
    pub postgres_copied: bool,
    pub minio_copied: bool,
    pub deployment_files: Vec<String>,
    pub warnings: Vec<String>,
}

/// The major of a `postgres --version` line: `16` from `postgres (PostgreSQL) 16.2`.
pub fn postgres_major(version: &str) -> Option<u32> {
    version
        .split_whitespace()
        .filter_map(|word| word.split('.').next())
        .find_map(|word| word.trim_start_matches('(').parse::<u32>().ok())
}

/// What the backup folder holds when it is done.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupReport {
    /// The folder everything was written into.
    pub path: String,
    /// The manifest inside it.
    pub manifest: String,
    /// When it was taken, as seconds since the epoch — no clock library in the core.
    pub taken_at: u64,
    /// The hub's storage mode at the time, so a restore knows what it is looking at.
    pub storage: StorageMode,
    /// `postgres/dump.sql` was written.
    pub dumped: bool,
    /// `postgres/data` holds a copy of `PGDATA`.
    pub postgres_copied: bool,
    /// `minio/data` holds a copy of the buckets.
    pub minio_copied: bool,
    /// The files copied into `deployment/`.
    pub deployment_files: Vec<String>,
    /// Anything that did not happen and should be known about.
    pub warnings: Vec<String>,
}

/// Where the backup goes and what it is of.
#[derive(Debug, Clone)]
pub struct BackupRequest {
    /// The deployment folder.
    pub dir: PathBuf,
    /// The folder to back up *into*. A timestamped subfolder is created inside it, so
    /// pointing several backups at the same place keeps every one.
    pub target: PathBuf,
}

/// The folder a backup taken now would be written to, for a front end to show before
/// starting: `<target>/<hub>-backup-<UTC timestamp>`.
pub fn backup_folder(request: &BackupRequest, now: u64) -> PathBuf {
    let hub = crate::compose::project_name(&request.dir.to_string_lossy());
    let hub = if hub.is_empty() { "hub".to_string() } else { hub };
    request
        .target
        .join(format!("{hub}-backup-{}", timestamp(now)))
}

pub async fn run(
    request: &BackupRequest,
    on_event: &(dyn Fn(BackupEvent) + Send + Sync),
) -> Result<BackupReport, BackupError> {
    let dir = &request.dir;
    if !profile::holds_a_hub(dir) {
        return Err(BackupError::NotAHub(dir.display().to_string()));
    }
    let config = profile::read_profile(dir)
        .map_err(|e| BackupError::Profile(e.to_string()))?
        .config;
    let storage = storage_mode_of(&config);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let out = backup_folder(request, now);

    // A backup written into `./minio_data` would be copied into itself, forever.
    for data in data_sources(dir, &config) {
        if let DataSource::Bind(path) = &data.source {
            if out.starts_with(path) {
                return Err(BackupError::TargetInsideData);
            }
        }
    }

    std::fs::create_dir_all(&out).map_err(|source| BackupError::Target {
        path: out.display().to_string(),
        source,
    })?;

    let mut report = BackupReport {
        path: out.display().to_string(),
        manifest: out.join(MANIFEST_FILE).display().to_string(),
        taken_at: now,
        storage,
        dumped: false,
        postgres_copied: false,
        minio_copied: false,
        deployment_files: Vec::new(),
        warnings: Vec::new(),
    };

    // --- the deployment's own files -----------------------------------------
    step(on_event, "deployment", "Copying the deployment's configuration");
    report.deployment_files = copy_deployment_files(dir, &out.join(DEPLOYMENT_DIR))?;
    for file in &report.deployment_files {
        line(on_event, "deployment", file);
    }

    // --- the dump -----------------------------------------------------------
    step(on_event, "dump", "Dumping the database");
    let db_was_running = service_running(dir, DB_COMPOSE_SERVICE).await?;
    if !db_was_running {
        line(on_event, "dump", "The database is not running; starting it for the dump");
        compose_streamed(dir, &["up", "-d", "--no-deps", DB_COMPOSE_SERVICE], "dump", on_event)
            .await?;
    }
    wait_for_database(dir, &config, "dump", on_event).await?;
    let server_version = postgres_version(dir).await;
    if let Some(version) = &server_version {
        line(on_event, "dump", version);
    }
    dump_database(dir, &config, &out.join(DUMP_FILE), on_event).await?;
    report.dumped = true;
    if !db_was_running {
        line(on_event, "dump", "Stopping the database again");
        compose_streamed(dir, &["stop", DB_COMPOSE_SERVICE], "dump", on_event).await?;
    }

    // --- the raw copies -------------------------------------------------------
    for data in data_sources(dir, &config) {
        step(on_event, data.step, data.title);
        let destination = out.join(data.destination);
        match resolve_source(dir, &data.source).await? {
            None => {
                let reason = match &data.source {
                    DataSource::Bind(path) => {
                        format!("{} does not exist yet — nothing has been stored", path.display())
                    }
                    DataSource::Volume(name) => format!(
                        "the volume `{name}` does not exist yet — the hub has never been started"
                    ),
                };
                on_event(BackupEvent::Skipped {
                    step: data.step.into(),
                    reason: reason.clone(),
                });
                report.warnings.push(reason);
            }
            Some(mount) => {
                std::fs::create_dir_all(&destination).map_err(|source| BackupError::Io {
                    path: destination.display().to_string(),
                    source,
                })?;
                let destination = std::fs::canonicalize(&destination)
                    .unwrap_or(destination)
                    .to_string_lossy()
                    .to_string();
                rsync(
                    &format!("{mount}:/source:ro"),
                    &format!("{destination}:/target"),
                    data.step,
                    on_event,
                )
                .await?;
                match data.step {
                    "postgres" => report.postgres_copied = true,
                    "minio" => report.minio_copied = true,
                    _ => {}
                }
            }
        }
    }

    if !db_was_running {
        report.warnings.push(
            "The database was started for the dump and stopped again afterwards.".into(),
        );
    } else {
        report.warnings.push(
            "postgres/data was copied from a running server; use postgres/dump.sql to restore."
                .into(),
        );
    }

    // --- the manifest -------------------------------------------------------
    step(on_event, "manifest", "Writing the manifest");
    let manifest = build_manifest(dir, &config, now, server_version, &report).await;
    let manifest_path = out.join(MANIFEST_FILE);
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).expect("the manifest serializes"),
    )
    .map_err(|source| BackupError::Io {
        path: manifest_path.display().to_string(),
        source,
    })?;
    line(
        on_event,
        "manifest",
        &format!(
            "{} services, {} infrastructure images",
            manifest.services.len(),
            manifest.infrastructure.len()
        ),
    );

    Ok(report)
}

/// One arkitekt service's image: `(id, compose host, image)`.
pub(crate) type ServiceImage = (ServiceId, String, String);
/// One infrastructure image: `(compose service, image)`.
pub(crate) type InfraImage = (String, String);

/// Every image the compose file names, services first.
pub(crate) fn images_of(config: &HubConfig) -> (Vec<ServiceImage>, Vec<InfraImage>) {
    let services = config
        .enabled_services()
        .into_iter()
        .filter_map(|id| {
            let block = config.service(id);
            block
                .image
                .clone()
                .map(|image| (id, block.host.clone(), image))
        })
        .collect();

    let mut infra = vec![
        (DB_COMPOSE_SERVICE.to_string(), config.db.image.clone()),
        (config.local_redis.host.clone(), config.local_redis.image.clone()),
        (config.minio.host.clone(), config.minio.image.clone()),
        (
            config.minio.init_container_host.clone(),
            config.minio.init_container_image.clone(),
        ),
        (config.gateway.host.clone(), config.gateway.image.clone()),
    ];
    if let Some(mesh) = config.mesh.as_ref().filter(|m| m.enabled) {
        infra.push((mesh.host.clone(), mesh.image.clone()));
    }
    if let Some(ollama) = config.local_ollama.as_ref().filter(|o| o.enabled) {
        infra.push((ollama.host.clone(), ollama.image.clone()));
    }
    (services, infra)
}

async fn build_manifest(
    dir: &Path,
    config: &HubConfig,
    now: u64,
    server_version: Option<String>,
    report: &BackupReport,
) -> BackupManifest {
    let (services, infra) = images_of(config);

    // Resolved on a best-effort basis: a machine that has not pulled an image yet has no
    // id for it, and that is worth recording as "unknown" rather than refusing to back up.
    let mut asked: Vec<(String, String)> = services
        .iter()
        .map(|(_, host, image)| (host.clone(), image.clone()))
        .collect();
    asked.extend(infra.iter().cloned());
    let states = docker::image_states(&asked).await.unwrap_or_default();
    let state_of = |service: &str| states.iter().find(|s| s.service == service);

    BackupManifest {
        format: MANIFEST_FORMAT,
        konstruktor_version: env!("CARGO_PKG_VERSION").to_string(),
        taken_at: now,
        storage: storage_mode_of(config),
        hub: ManifestHub {
            identifier: credentials::read_credentials(dir).map(|c| c.identifier),
            coord_server: config.coord_server.clone(),
            project: crate::compose::project_name(&dir.to_string_lossy()),
            path: dir.display().to_string(),
        },
        services: services
            .iter()
            .map(|(id, host, image)| ManifestService {
                id: *id,
                host: host.clone(),
                image: image.clone(),
                image_id: state_of(host).and_then(|s| s.image_id.clone()),
                repo_digests: state_of(host).map(|s| s.repo_digests.clone()).unwrap_or_default(),
                db: config.service(*id).db_config.db.clone(),
            })
            .collect(),
        infrastructure: infra
            .iter()
            .map(|(service, image)| ManifestImage {
                service: service.clone(),
                image: image.clone(),
                image_id: state_of(service).and_then(|s| s.image_id.clone()),
            })
            .collect(),
        postgres: ManifestPostgres {
            user: config.db.postgres_user.clone(),
            server_version,
        },
        contents: BackupContents {
            dumped: report.dumped,
            postgres_copied: report.postgres_copied,
            minio_copied: report.minio_copied,
            deployment_files: report.deployment_files.clone(),
            warnings: report.warnings.clone(),
        },
    }
}

/// `postgres --version` inside the running database container. Best effort.
pub(crate) async fn postgres_version(dir: &Path) -> Option<String> {
    let output = engine()
        .args(["compose", "exec", "-T", DB_COMPOSE_SERVICE, "postgres", "--version"])
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

// --- the parts --------------------------------------------------------------

pub(crate) fn step(on_event: &(dyn Fn(BackupEvent) + Send + Sync), step: &str, title: &str) {
    on_event(BackupEvent::Step {
        step: step.into(),
        title: title.into(),
    });
}

pub(crate) fn line(on_event: &(dyn Fn(BackupEvent) + Send + Sync), step: &str, line: &str) {
    on_event(BackupEvent::Line {
        step: step.into(),
        line: line.into(),
        stderr: false,
    });
}

/// The files that describe the hub, without any of its data or its source checkouts.
fn copy_deployment_files(dir: &Path, into: &Path) -> Result<Vec<String>, BackupError> {
    let io = |path: &Path, source: std::io::Error| BackupError::Io {
        path: path.display().to_string(),
        source,
    };

    std::fs::create_dir_all(into).map_err(|e| io(into, e))?;
    let mut copied = Vec::new();

    for name in [
        HUB_CONFIG_FILENAME,
        CREDENTIALS_FILENAME,
        crate::compose_file::COMPOSE_FILENAME,
        crate::compose_file::COMPOSE_BACKUP_FILENAME,
    ] {
        let from = dir.join(name);
        if from.is_file() {
            std::fs::copy(&from, into.join(name)).map_err(|e| io(&from, e))?;
            copied.push(name.to_string());
        }
    }

    let configs = dir.join("configs");
    if configs.is_dir() {
        let target = into.join("configs");
        std::fs::create_dir_all(&target).map_err(|e| io(&target, e))?;
        for entry in std::fs::read_dir(&configs).map_err(|e| io(&configs, e))? {
            let entry = entry.map_err(|e| io(&configs, e))?;
            let path = entry.path();
            if path.is_file() {
                let name = entry.file_name();
                std::fs::copy(&path, target.join(&name)).map_err(|e| io(&path, e))?;
                copied.push(format!("configs/{}", name.to_string_lossy()));
            }
        }
    }

    copied.sort();
    Ok(copied)
}

/// What a raw copy is taken from: a directory on the host, or a compose volume.
#[derive(Debug, Clone)]
pub(crate) enum DataSource {
    Bind(PathBuf),
    /// The volume's *compose* name — `db_data` — not the engine's `<project>_db_data`.
    Volume(String),
}

pub(crate) struct DataPart {
    pub(crate) step: &'static str,
    pub(crate) title: &'static str,
    pub(crate) destination: &'static str,
    pub(crate) source: DataSource,
}

pub(crate) fn source_of(dir: &Path, mount: Option<&str>, volume_name: &str) -> DataSource {
    match mount.filter(|m| !m.is_empty()) {
        Some(mount) => DataSource::Bind(dir.join(mount.trim_start_matches("./"))),
        None => DataSource::Volume(volume_name.to_string()),
    }
}

pub(crate) fn data_sources(dir: &Path, config: &HubConfig) -> Vec<DataPart> {
    vec![
        DataPart {
            step: "postgres",
            title: "Copying the database files",
            destination: POSTGRES_DATA_DIR,
            source: source_of(dir, config.db.mount.as_deref(), &config.db.volume_name),
        },
        DataPart {
            step: "minio",
            title: "Copying the object storage",
            destination: MINIO_DATA_DIR,
            source: source_of(dir, config.minio.mount.as_deref(), &config.minio.volume_name),
        },
    ]
}

/// The `-v` source for the rsync container: an absolute host path, or the engine's name
/// for the volume. `None` when there is nothing there yet.
pub(crate) async fn resolve_source(dir: &Path, source: &DataSource) -> Result<Option<String>, BackupError> {
    match source {
        DataSource::Bind(path) => {
            if !path.is_dir() {
                return Ok(None);
            }
            let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            Ok(Some(absolute.to_string_lossy().to_string()))
        }
        DataSource::Volume(name) => {
            let engine_name = volume_engine_name(dir, name).await?;
            let exists = engine()
                .args(["volume", "inspect", &engine_name])
                .current_dir(dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .map_err(|source| BackupError::Engine {
                    engine: "docker volume inspect",
                    source,
                })?
                .success();
            Ok(exists.then_some(engine_name))
        }
    }
}

/// What the engine calls a compose volume.
///
/// Asked of `compose config`, which resolves the project name the same way `up` will —
/// including a `name:` somebody put into the file by hand — and falls back to compose's
/// own `<project>_<volume>` convention if that cannot be read.
pub(crate) async fn volume_engine_name(dir: &Path, volume: &str) -> Result<String, BackupError> {
    let fallback = format!(
        "{}_{volume}",
        crate::compose::project_name(&dir.to_string_lossy())
    );
    let output = engine()
        .args(["compose", "config", "--format", "json"])
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|source| BackupError::Engine {
            engine: "docker compose config",
            source,
        })?;
    if !output.status.success() {
        return Ok(fallback);
    }
    let parsed: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(_) => return Ok(fallback),
    };
    Ok(parsed["volumes"][volume]["name"]
        .as_str()
        .map(str::to_string)
        .unwrap_or(fallback))
}

pub(crate) async fn service_running(dir: &Path, service: &str) -> Result<bool, BackupError> {
    let output = engine()
        .args(["compose", "ps", "--status", "running", "-q", service])
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|source| BackupError::Engine {
            engine: "docker compose ps",
            source,
        })?;
    Ok(output.status.success() && !String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

pub(crate) async fn wait_for_database(
    dir: &Path,
    config: &HubConfig,
    step: &str,
    on_event: &(dyn Fn(BackupEvent) + Send + Sync),
) -> Result<(), BackupError> {
    let started = Instant::now();
    let mut reported = false;
    loop {
        let ready = engine()
            .args([
                "compose",
                "exec",
                "-T",
                DB_COMPOSE_SERVICE,
                "pg_isready",
                "-U",
                &config.db.postgres_user,
            ])
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map_err(|source| BackupError::Engine {
                engine: "docker compose exec",
                source,
            })?
            .success();
        if ready {
            return Ok(());
        }
        if started.elapsed() > DB_READY_TIMEOUT {
            return Err(BackupError::DatabaseNotReady(DB_READY_TIMEOUT.as_secs()));
        }
        if !reported {
            line(on_event, step, "Waiting for the database to accept connections…");
            reported = true;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// `pg_dumpall` through `compose exec`, streamed straight into the file — a hub's
/// database can be gigabytes, and none of it needs to pass through memory.
async fn dump_database(
    dir: &Path,
    config: &HubConfig,
    into: &Path,
    on_event: &(dyn Fn(BackupEvent) + Send + Sync),
) -> Result<(), BackupError> {
    let io = |path: &Path, source: std::io::Error| BackupError::Io {
        path: path.display().to_string(),
        source,
    };
    if let Some(parent) = into.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
    }

    let mut child = engine()
        .args([
            "compose",
            "exec",
            "-T",
            DB_COMPOSE_SERVICE,
            "pg_dumpall",
            "-U",
            &config.db.postgres_user,
            "--clean",
            "--if-exists",
        ])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|source| BackupError::Engine {
            engine: "docker compose exec",
            source,
        })?;

    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");

    let mut file = tokio::fs::File::create(into).await.map_err(|e| io(into, e))?;
    let copy = async {
        let mut reader = BufReader::new(stdout);
        let mut written: u64 = 0;
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let n = tokio::io::AsyncReadExt::read(&mut reader, &mut buffer).await?;
            if n == 0 {
                break;
            }
            file.write_all(&buffer[..n]).await?;
            written += n as u64;
        }
        file.flush().await?;
        Ok::<u64, std::io::Error>(written)
    };
    let errors = async {
        let mut collected = String::new();
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(raw)) = lines.next_line().await {
            if raw.trim().is_empty() {
                continue;
            }
            collected.push_str(&raw);
            collected.push('\n');
            on_event(BackupEvent::Line {
                step: "dump".into(),
                line: raw,
                stderr: true,
            });
        }
        collected
    };

    let (written, stderr_text) = tokio::join!(copy, errors);
    let written = written.map_err(|e| io(into, e))?;
    let status = child.wait().await.map_err(|source| BackupError::Engine {
        engine: "docker compose exec",
        source,
    })?;

    if !status.success() {
        return Err(BackupError::Step {
            step: "pg_dumpall",
            detail: if stderr_text.trim().is_empty() {
                format!("exit status {status}")
            } else {
                stderr_text.trim().to_string()
            },
        });
    }
    line(
        on_event,
        "dump",
        &format!("Wrote {} ({} bytes)", DUMP_FILE, written),
    );
    Ok(())
}

/// The copy itself: a throwaway container with the source read-only and the backup
/// folder read-write, running `rsync -a` between them.
///
/// `--delete` so that a second backup into the same folder is a mirror rather than a
/// union; `--info=progress2` for one progress line to stream rather than one per file.
///
/// Both sides are complete `-v` specs — `<host path or volume>:/source:ro` and
/// `<host path or volume>:/target` — so the same call copies a volume out for a backup
/// and a backup folder back into a volume for a restore.
pub(crate) async fn rsync(
    source_mount: &str,
    target_mount: &str,
    step: &str,
    on_event: &(dyn Fn(BackupEvent) + Send + Sync),
) -> Result<(), BackupError> {
    let args = [
        "run".to_string(),
        "--rm".into(),
        "-v".into(),
        source_mount.to_string(),
        "-v".into(),
        target_mount.to_string(),
        "--entrypoint".into(),
        "rsync".into(),
        RSYNC_IMAGE.into(),
        "-a".into(),
        "--delete".into(),
        "--info=progress2".into(),
        "--no-inc-recursive".into(),
        "/source/".into(),
        "/target/".into(),
    ];

    let mut command = engine();
    command.args(&args);
    let (status, stdout, stderr) = stream(command, step, on_event).await?;
    if !status.success() {
        return Err(BackupError::Step {
            step: "rsync",
            detail: format!("{stdout}{stderr}").trim().to_string(),
        });
    }
    Ok(())
}

pub(crate) async fn compose_streamed(
    dir: &Path,
    args: &[&str],
    step: &str,
    on_event: &(dyn Fn(BackupEvent) + Send + Sync),
) -> Result<(), BackupError> {
    let mut command = engine();
    command
        .arg("compose")
        .args(["--ansi", "never"])
        .args(args)
        .current_dir(dir);
    let (status, stdout, stderr) = stream(command, step, on_event).await?;
    if !status.success() {
        return Err(BackupError::Step {
            step: "docker compose",
            detail: format!("{stdout}{stderr}").trim().to_string(),
        });
    }
    Ok(())
}

pub(crate) fn engine() -> Command {
    engine_probe::engine().async_command()
}

/// Runs a command, handing every line of either stream to `on_event` as it appears, and
/// hands back what was collected for the error message when it fails.
pub(crate) async fn stream(
    mut command: Command,
    step: &str,
    on_event: &(dyn Fn(BackupEvent) + Send + Sync),
) -> Result<(std::process::ExitStatus, String, String), BackupError> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|source| BackupError::Engine {
            engine: "docker",
            source,
        })?;

    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");

    let collect = |reader: tokio::process::ChildStdout| async move {
        let mut collected = String::new();
        let mut lines = BufReader::new(reader).lines();
        let mut out = Vec::new();
        while let Ok(Some(raw)) = lines.next_line().await {
            // rsync's progress line ends in `\r`, not `\n`; a whole transfer can be one
            // "line" to the reader. Split it so the last state is what gets shown.
            for piece in raw.split('\r') {
                let piece = piece.trim_end();
                if piece.trim().is_empty() {
                    continue;
                }
                collected.push_str(piece);
                collected.push('\n');
                out.push(piece.to_string());
            }
        }
        (collected, out)
    };

    let err_task = async move {
        let mut collected = String::new();
        let mut lines = BufReader::new(stderr).lines();
        let mut out = Vec::new();
        while let Ok(Some(raw)) = lines.next_line().await {
            if raw.trim().is_empty() {
                continue;
            }
            collected.push_str(&raw);
            collected.push('\n');
            out.push(raw);
        }
        (collected, out)
    };

    // Both streams are drained concurrently; lines are handed on afterwards in order per
    // stream. The output of a copy is short and the wait is the copy itself, so nothing
    // is lost by not interleaving them live — and it keeps `on_event` off the reader.
    let ((stdout_text, stdout_lines), (stderr_text, stderr_lines)) =
        tokio::join!(collect(stdout), err_task);
    for l in stdout_lines {
        on_event(BackupEvent::Line {
            step: step.into(),
            line: l,
            stderr: false,
        });
    }
    for l in stderr_lines {
        on_event(BackupEvent::Line {
            step: step.into(),
            line: l,
            stderr: true,
        });
    }

    let status = child.wait().await.map_err(|source| BackupError::Engine {
        engine: "docker",
        source,
    })?;
    Ok((status, stdout_text, stderr_text))
}

/// `YYYYMMDD-HHMMSS` in UTC, from seconds since the epoch. The core has no date library
/// and one folder name is not a reason to take one on.
pub fn timestamp(secs: u64) -> String {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    // Howard Hinnant's civil-from-days.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };

    format!("{y:04}{mo:02}{d:02}-{h:02}{m:02}{s:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_utc_and_sortable() {
        assert_eq!(timestamp(0), "19700101-000000");
        // 2024-02-29T12:34:56Z — a leap day, the case the arithmetic most often gets wrong.
        assert_eq!(timestamp(1_709_210_096), "20240229-123456");
    }

    #[test]
    fn a_bind_mount_is_a_host_directory_and_a_blank_one_is_a_volume() {
        let dir = Path::new("/hubs/mine");
        assert!(matches!(
            source_of(dir, Some("./db_data"), "db_data"),
            DataSource::Bind(p) if p == Path::new("/hubs/mine/db_data")
        ));
        assert!(matches!(
            source_of(dir, None, "db_data"),
            DataSource::Volume(v) if v == "db_data"
        ));
        assert!(matches!(
            source_of(dir, Some(""), "minio_data"),
            DataSource::Volume(v) if v == "minio_data"
        ));
    }

    #[test]
    fn reads_the_major_out_of_a_postgres_version_line() {
        assert_eq!(postgres_major("postgres (PostgreSQL) 16.2"), Some(16));
        assert_eq!(
            postgres_major("postgres (PostgreSQL) 15.6 (Debian 15.6-1.pgdg120+2)"),
            Some(15)
        );
        assert_eq!(postgres_major(""), None);
    }

    #[test]
    fn the_backup_folder_is_named_after_the_hub_and_the_moment() {
        let request = BackupRequest {
            dir: PathBuf::from("/hubs/My Hub"),
            target: PathBuf::from("/backups"),
        };
        assert_eq!(
            backup_folder(&request, 0),
            PathBuf::from("/backups/myhub-backup-19700101-000000")
        );
    }
}
