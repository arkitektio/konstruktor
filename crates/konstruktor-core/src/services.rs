//! Adding services to a hub that already exists, and taking them out again.
//!
//! A hub's services used to be chosen once, in the wizard. Changing them afterwards is
//! the re-authorize flow with one more answer: the profile's blocks are switched on or
//! off *in memory*, the manifest goes to the coordination server again — a new instance
//! needs its grant, and its public key has to be vouched for — and only once that is
//! accepted is anything written. A decline leaves the hub exactly as it was, as with any
//! other re-authorization.
//!
//! Applying is its own step ([`apply_services`]): databases the new services need are
//! created in the running cluster (Postgres only runs its init scripts on an empty data
//! directory, so the init list alone creates nothing on an existing hub), the bucket
//! manifest is replayed, and `compose up -d --remove-orphans` starts the new containers
//! and removes the old ones.
//!
//! **Nothing is deleted.** A service taken out keeps its database and buckets — they stay
//! in the init list and the bucket manifest ([`ServiceBlock::retained`]) — and its block
//! stays in the profile with its keys, so adding it back picks everything up again.
//!
//! [`ServiceBlock::retained`]: crate::config::hub::ServiceBlock::retained

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::catalog::{in_generation_order, ServiceId};
use crate::compose::ComposeLine;
use crate::config::hub::{HubConfig, DB_COMPOSE_SERVICE};
use crate::contract::Said;
use crate::create::{
    reauthorize, CreateError, CreateEvent, MeshKeyRequest, ReauthorizeAnswers, Reauthorized,
};
use crate::start::{StartError, StartReport};

/// What somebody asked for: services to add, services to take out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceChange {
    #[serde(default)]
    pub add: Vec<ServiceId>,
    #[serde(default)]
    pub remove: Vec<ServiceId>,
    /// The image each added service runs, for the ones added *by* their image
    /// (`hub services add --image`). A service of the catalogue needs none: it starts on
    /// the image its block names. One outside it has no other.
    #[serde(default)]
    pub images: BTreeMap<ServiceId, String>,
}

/// What a [`ServiceChange`] actually does to one hub.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicePlan {
    /// Switched on, in generation order.
    pub added: Vec<ServiceId>,
    /// Switched off, their data kept, in generation order.
    pub removed: Vec<ServiceId>,
    /// Asked for, but already so: a service added that runs, one removed that does not.
    pub unchanged: Vec<ServiceId>,
    /// What the hub runs afterwards, in generation order.
    pub services: Vec<ServiceId>,
    /// Worth saying before anybody is sent to a browser.
    pub notes: Vec<String>,
    /// The image each of `added` was named by, for the ones that were. See
    /// [`ServiceChange::images`].
    #[serde(default)]
    pub images: BTreeMap<ServiceId, String>,
}

/// What a service is called in a sentence: the catalogue's word for it, or its own name.
fn name_of(id: ServiceId) -> String {
    id.known()
        .map(|known| known.name.to_string())
        .unwrap_or_else(|| id.as_str().to_string())
}

/// Of `services`, the ones Rekuest runs the periodic work of and receives the signals of,
/// by what their images said (`said`, by compose service). A service that was not asked is
/// not known to be one.
pub fn hooked_by_rekuest(
    config: &HubConfig,
    said: &Said,
    services: &[ServiceId],
) -> Vec<ServiceId> {
    services
        .iter()
        .copied()
        .filter(|id| *id != ServiceId::Rekuest)
        .filter(|id| {
            config
                .get(*id)
                .and_then(|block| said.get(&block.host))
                .is_some_and(|description| description.hooked_by_rekuest())
        })
        .collect()
}

fn names(ids: &[ServiceId]) -> String {
    ids.iter()
        .map(|id| name_of(*id))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Works out what `change` does to `config`, or why it cannot be done. Pure: the front ends
/// show the result before asking anybody to confirm, and [`reauthorize`] asks it again
/// before a request leaves this machine.
///
/// `said` is what the hub's images have said of themselves, by compose service
/// ([`crate::contract::known`]): it is what tells which services need Rekuest kept.
pub fn plan(
    config: &HubConfig,
    said: &Said,
    change: &ServiceChange,
) -> Result<ServicePlan, CreateError> {
    let refuse = |message: String| Err(CreateError::Answers(message));

    if let Some(both) = change.add.iter().find(|id| change.remove.contains(id)) {
        return refuse(format!(
            "{} is both added and removed — ask for one or the other",
            name_of(*both)
        ));
    }

    // Something has to say which image an added service runs: the change, the block it
    // already has, or the catalogue. A name on its own, outside the catalogue, is not a
    // service anybody can start.
    for id in &change.add {
        let has_image = change.images.contains_key(id)
            || config.get(*id).is_some_and(|block| block.image.is_some())
            || id.default_image().is_some();
        if !has_image {
            return refuse(format!(
                "{} is not a service this konstruktor knows an image for — add it by its \
                 image, with `--image`",
                name_of(*id)
            ));
        }
    }

    let running = config.enabled_services();
    let mut plan = ServicePlan::default();
    // Every service the hub has or is asked about, the catalogue's first, in the order
    // they are generated in.
    let concerned = in_generation_order(
        config
            .service_ids()
            .into_iter()
            .chain(change.add.iter().copied())
            .chain(change.remove.iter().copied()),
    );
    for id in concerned {
        let on = running.contains(&id);
        if change.add.contains(&id) {
            if on {
                plan.unchanged.push(id);
            } else {
                plan.added.push(id);
                if let Some(image) = change.images.get(&id) {
                    plan.images.insert(id, image.clone());
                }
            }
        }
        if change.remove.contains(&id) {
            if on {
                plan.removed.push(id);
            } else {
                plan.unchanged.push(id);
            }
        }
        let after = (on || plan.added.contains(&id)) && !plan.removed.contains(&id);
        if after {
            plan.services.push(id);
        }
    }

    if plan.added.is_empty() && plan.removed.is_empty() {
        let mut why = Vec::new();
        let already: Vec<ServiceId> = plan
            .unchanged
            .iter()
            .copied()
            .filter(|id| change.add.contains(id))
            .collect();
        let absent: Vec<ServiceId> = plan
            .unchanged
            .iter()
            .copied()
            .filter(|id| change.remove.contains(id))
            .collect();
        if !already.is_empty() {
            why.push(format!("{} already running", names(&already)));
        }
        if !absent.is_empty() {
            why.push(format!("{} not running", names(&absent)));
        }
        return refuse(if why.is_empty() {
            "nothing to change — name a service to add or remove".to_string()
        } else {
            format!("nothing to change: {}", why.join("; "))
        });
    }

    if plan.services.is_empty() {
        return refuse("a hub has to keep at least one service".to_string());
    }

    if plan.removed.contains(&ServiceId::Rekuest) {
        let hooked = hooked_by_rekuest(config, said, &plan.services);
        if !hooked.is_empty() {
            return refuse(format!(
                "Rekuest runs the periodic work and receives the signals of {} — remove \
                 those too, or keep Rekuest",
                names(&hooked)
            ));
        }
    }

    if plan.added.contains(&ServiceId::Rekuest) {
        let remote = config.rekuest_server.trim();
        if !matches!(remote, "local" | "none" | "") {
            plan.notes.push(format!(
                "The services here trust the Rekuest at {remote} now; they will trust this \
                 hub's own instead."
            ));
        }
    }
    if plan.added.contains(&ServiceId::Alpaka) && config.local_ollama.is_none() {
        plan.notes.push(
            "Alpaka is added without a model provider: it starts, but answers nothing until \
             one is configured."
                .to_string(),
        );
    }
    if plan.added.contains(&ServiceId::Lovekit) {
        let mesh_only = config
            .mesh
            .as_ref()
            .is_some_and(|m| m.enabled && m.mesh_only);
        plan.notes.push(if mesh_only {
            "Lovekit brings a LiveKit media server with it. This hub is mesh-only and \
             publishes no port, so only clients that relay their media over the mesh can \
             hold a room."
                .to_string()
        } else {
            "Lovekit brings a LiveKit media server with it, which opens ports 2756/tcp, \
             2757/tcp and 2758/udp on this machine. Media flows on them directly, so it \
             works on this machine's network, not across the internet."
                .to_string()
        });
    }
    if !plan.removed.is_empty() {
        plan.notes.push(format!(
            "The data of {} is kept: its database and buckets stay, and adding it back \
             picks them up again.",
            names(&plan.removed)
        ));
    }
    Ok(plan)
}

/// Folds a plan into a profile. Only ever on a copy in memory until the coordination
/// server has accepted it — see [`reauthorize`].
pub fn apply_plan(config: &mut HubConfig, plan: &ServicePlan) {
    for id in &plan.added {
        match plan.images.get(id) {
            Some(image) => config.add_service_on(*id, image),
            None => config.add_service(*id),
        }
    }
    for id in &plan.removed {
        config.remove_service(*id);
    }
}

/// The re-authorization a service change sends, from what is on disk: the identifier,
/// server and addresses the hub is authorized with now, so the only thing that differs is
/// the services.
///
/// A mesh key is asked for only by a hub already on a mesh (see [`MeshKeyRequest::Auto`]):
/// changing services must not make a hub join one.
pub fn answers_from_disk(
    dir: &Path,
    change: ServiceChange,
) -> Result<ReauthorizeAnswers, CreateError> {
    let profile =
        crate::profile::read_profile(dir).map_err(|e| CreateError::Folder(e.to_string()))?;
    let config = &profile.config;
    if config.running_lok().is_some() {
        return Err(CreateError::Answers(
            "this hub runs its own coordination server, and its services are part of what \
             that server was set up with — create the hub again with the services you want"
                .into(),
        ));
    }
    let Some(credentials) = crate::credentials::read_credentials(dir) else {
        return Err(CreateError::Answers(
            "this hub has never been authorized — authorize it first, then change its \
             services"
                .into(),
        ));
    };
    let on_mesh = config.mesh.as_ref().is_some_and(|m| m.enabled);
    let mesh_only = config
        .mesh
        .as_ref()
        .is_some_and(|m| m.enabled && m.mesh_only);
    // Sent as it is, an empty list would replace what the coordination server has with
    // nothing. Only a mesh-only hub advertises nothing on purpose; any other was
    // authorized before its addresses were recorded, and has to say them once.
    if credentials.advertised_hosts.is_empty() && !mesh_only {
        return Err(CreateError::Answers(
            "this hub does not record the addresses it advertises, and changing its services \
             would send none — authorize it once with `konstruktor authorize --host/--reach` \
             (or the Authorize screen) so it records them"
                .into(),
        ));
    }
    Ok(ReauthorizeAnswers {
        dir: dir.to_path_buf(),
        coord_server: if credentials.server.trim().is_empty() {
            config.coord_server.clone()
        } else {
            credentials.server.clone()
        },
        identifier: credentials.identifier.clone(),
        description: None,
        // Empty for a mesh-only hub, which is what it should keep advertising.
        hosts: credentials.advertised_hosts.clone(),
        // Not recorded: which aliases a probe reached is only known at the time.
        reachable_hosts: Vec::new(),
        mesh_key: if on_mesh {
            MeshKeyRequest::Auto
        } else {
            MeshKeyRequest::Never
        },
        services: Some(change),
        described: BTreeMap::new(),
    })
}

/// What changing the services did.
#[derive(Debug, Clone)]
pub struct ServicesChanged {
    pub reauthorized: Reauthorized,
    pub plan: ServicePlan,
    /// The stack was brought to the new set of services.
    pub applied: bool,
}

/// Change a hub's services end to end: re-authorize with the new set, check out the source
/// of any added service that runs from a checkout, and — when `apply` — bring the stack to
/// it with [`apply_services`].
///
/// `answers.services` has to be set; [`answers_from_disk`] builds answers that do.
pub async fn change_services(
    answers: &ReauthorizeAnswers,
    apply: bool,
    cancel: &CancellationToken,
    on: &(dyn Fn(CreateEvent) + Sync),
) -> Result<ServicesChanged, CreateError> {
    if answers.services.is_none() {
        return Err(CreateError::Answers(
            "no service change was asked for".into(),
        ));
    }
    let before = snapshot_configs(&answers.dir);
    let reauthorized = reauthorize(answers, cancel, on).await?;
    let plan = reauthorized
        .services
        .clone()
        .expect("a change was asked for, so a plan was made");
    let changed = changed_configs(&before, &snapshot_configs(&answers.dir));

    // A dev hub mounts every service's source, the added ones included, and compose
    // would hand them an empty workspace without a checkout.
    let config = crate::profile::read_profile(&answers.dir)
        .map_err(|e| CreateError::Folder(e.to_string()))?
        .config;
    crate::create::check_sources_out(&answers.dir, &config, &plan.added, &|_| None, on)?;

    if apply {
        on(CreateEvent::Starting);
        let log = |line: ComposeLine| on(CreateEvent::Log { line: line.line });
        let restart = services_to_restart(&config, &changed, &plan);
        if let Err(error) = apply_services(&answers.dir, &restart, &log).await {
            on(CreateEvent::Log {
                line: error.to_string(),
            });
            return Err(CreateError::ApplyFailed(error.to_string()));
        }
    }

    Ok(ServicesChanged {
        reauthorized,
        plan,
        applied: apply,
    })
}

/// Every generated file under `configs/`, by name. What the running containers have
/// bind-mounted — compose does not look into them, so a change to one recreates nothing.
pub fn snapshot_configs(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let Ok(entries) = std::fs::read_dir(dir.join("configs")) else {
        return BTreeMap::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            std::fs::read(entry.path()).ok().map(|bytes| (name, bytes))
        })
        .collect()
}

/// The names of the files that differ between two snapshots, appeared or went.
pub fn changed_configs(
    before: &BTreeMap<String, Vec<u8>>,
    after: &BTreeMap<String, Vec<u8>>,
) -> Vec<String> {
    before
        .keys()
        .chain(after.keys())
        .filter(|name| before.get(*name) != after.get(*name))
        .cloned()
        .collect::<std::collections::BTreeSet<String>>()
        .into_iter()
        .collect()
}

/// The running containers that have to be restarted to read a config that changed under
/// them: the gateway for the Caddyfile (routes came and went), a service — and whatever
/// reads it besides it, Rekuest's takt — for its own config (the services Rekuest
/// hooks into, the inline trust bundle). A service just added or removed is not among them:
/// `up` starts the one and removes the other.
pub fn services_to_restart(
    config: &HubConfig,
    changed: &[String],
    plan: &ServicePlan,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |name: String| {
        if !out.contains(&name) {
            out.push(name);
        }
    };
    for file in changed {
        if file == "Caddyfile" {
            push(config.gateway.host.clone());
            continue;
        }
        let Some(host) = file.strip_suffix(".yaml") else {
            continue;
        };
        let Some(id) = config.service_at(host).filter(|id| config.runs(*id)) else {
            continue;
        };
        if plan.added.contains(&id) {
            continue;
        }
        push(host.to_string());
        for companion in crate::generate::compose::companions(config, host) {
            push(companion);
        }
    }
    out
}

/// Everything a standalone [`apply_services`] restarts: it cannot know which configs
/// changed since the stack last read them, so every service that reads one does.
pub fn every_config_reader(config: &HubConfig) -> Vec<String> {
    let mut out = vec![config.gateway.host.clone()];
    for id in config.enabled_services() {
        let host = &config.service(id).host;
        out.push(host.clone());
        out.extend(crate::generate::compose::companions(config, host));
    }
    out
}

/// Brings a running (or stopped) hub to the services its profile names.
///
/// 1. the database guard, then the database up on its own;
/// 2. every enabled service's database created if the cluster lacks it — the init list
///    only takes effect on an empty data directory;
/// 3. `compose up -d --remove-orphans`: new containers start, removed ones go (their
///    volumes stay);
/// 4. the bucket manifest replayed, so a new service finds its buckets;
/// 5. `restart` restarted — the containers whose bind-mounted config changed, which
///    `up` does not notice ([`services_to_restart`]).
///
/// Idempotent, so it is also how a `--no-apply` change is applied later.
pub async fn apply_services(
    dir: &Path,
    restart: &[String],
    on_line: &(dyn Fn(ComposeLine) + Send + Sync),
) -> Result<StartReport, StartError> {
    let config = crate::profile::read_profile(dir)
        .map_err(|e| StartError::Refused(e.to_string()))?
        .config;
    let say = |line: &str| {
        on_line(ComposeLine {
            line: line.to_string(),
            stderr: true,
        })
    };

    let databases: Vec<String> = config
        .enabled_services()
        .into_iter()
        .filter_map(|id| config.service(id).database().map(str::to_string))
        .collect();
    if !databases.is_empty() {
        // The same guard `start` runs, before anything pulls or recreates the database.
        let database = crate::updates::pinned_database(dir, &config);
        match crate::updates::guard_on(dir, &config, DB_COMPOSE_SERVICE, &database).await {
            crate::updates::Guard::Refuse(reason) => return Err(StartError::Refused(reason)),
            // Said once, by `start` below, which asks the same guard again.
            crate::updates::Guard::Warn(_) | crate::updates::Guard::Clear => {}
        }
        // `--remove-orphans` already: a service just taken out is removed by the `up`
        // below either way, and compose would otherwise warn about it here.
        let up_db = vec![
            "compose".to_string(),
            "up".to_string(),
            "-d".to_string(),
            "--remove-orphans".to_string(),
            DB_COMPOSE_SERVICE.to_string(),
        ];
        crate::compose::run_streamed(dir, up_db, on_line)
            .await
            .map_err(StartError::Compose)?;
        crate::backup::wait_for_database(dir, &config, "database", &|event| {
            if let crate::backup::BackupEvent::Line { line, stderr, .. } = event {
                on_line(ComposeLine { line, stderr });
            }
        })
        .await
        .map_err(|e| StartError::Compose(e.to_string()))?;
        for database in &databases {
            ensure_database(dir, &config, database, &say).await?;
        }
    }

    let report = crate::start::start_removing_orphans(dir, on_line).await?;

    // The init container is run-once: replayed so the manifest's new buckets exist.
    let has_buckets = config
        .provisioned_services()
        .iter()
        .any(|id| config.service(*id).uses_datalayer());
    if has_buckets {
        let replay = vec![
            "compose".to_string(),
            "up".to_string(),
            "-d".to_string(),
            "--force-recreate".to_string(),
            config.minio.init_container_host.clone(),
        ];
        crate::compose::run_streamed(dir, replay, on_line)
            .await
            .map_err(StartError::Compose)?;
    }

    // Only what is declared: a service somebody took out of the file by hand would fail
    // the whole restart.
    let restart: Vec<String> = restart
        .iter()
        .filter(|name| crate::compose_file::declares_service(dir, name))
        .cloned()
        .collect();
    if !restart.is_empty() {
        say(&format!(
            "Restarting {} to read the rewritten configuration…",
            restart.join(", ")
        ));
        let mut args = vec!["compose".to_string(), "restart".to_string()];
        args.extend(restart);
        crate::compose::run_streamed(dir, args, on_line)
            .await
            .map_err(StartError::Compose)?;
    }
    Ok(report)
}

/// A database name the init script would have created: nothing to quote, nothing to
/// inject. The names come from the profile, which a person can edit.
fn plain_identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
}

async fn psql(dir: &Path, config: &HubConfig, database: &str, sql: &str) -> Result<String, String> {
    let output = crate::engine_probe::engine()
        .async_command()
        .args([
            "compose",
            "exec",
            "-T",
            DB_COMPOSE_SERVICE,
            "psql",
            "-v",
            "ON_ERROR_STOP=1",
            "-U",
            &config.db.postgres_user,
            "-d",
            database,
            "-tAc",
            sql,
        ])
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// What the image's init script does for each database on a new cluster, for one database
/// on an existing one: the role, the database, the grant, then the extensions.
async fn ensure_database(
    dir: &Path,
    config: &HubConfig,
    database: &str,
    say: &(dyn Fn(&str) + Sync),
) -> Result<(), StartError> {
    if !plain_identifier(database) {
        return Err(StartError::Refused(format!(
            "the database name `{database}` is not a plain identifier; create it by hand"
        )));
    }
    let exists = psql(
        dir,
        config,
        "postgres",
        &format!("SELECT 1 FROM pg_database WHERE datname = '{database}'"),
    )
    .await
    .map_err(StartError::Compose)?;
    if exists == "1" {
        return Ok(());
    }

    say(&format!("Creating the database {database}…"));
    let steps = [
        format!(
            "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '{database}') \
             THEN CREATE ROLE {database} LOGIN; END IF; END $$"
        ),
        format!("CREATE DATABASE {database}"),
        format!("GRANT ALL PRIVILEGES ON DATABASE {database} TO {database}"),
    ];
    for sql in &steps {
        psql(dir, config, "postgres", sql)
            .await
            .map_err(|e| StartError::Compose(format!("creating the database {database}: {e}")))?;
    }
    // As the init script does, and as the superuser: none of them is `trusted`. A missing
    // one is the service's migration's to report, not a reason to stop here.
    for extension in ["cube", "vector", "postgis"] {
        if let Err(error) = psql(
            dir,
            config,
            database,
            &format!("CREATE EXTENSION IF NOT EXISTS {extension}"),
        )
        .await
        {
            say(&format!(
                "{database}: could not create the {extension} extension: {error}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::hub::HubConfigOptions;
    use crate::support;

    /// A hub with the services the wizard pre-ticks, each provided what it asked for.
    fn hub() -> HubConfig {
        support::hub(&HubConfigOptions {
            services: Some(crate::catalog::default_services()),
            ..Default::default()
        })
    }

    fn change(add: &[ServiceId], remove: &[ServiceId]) -> ServiceChange {
        ServiceChange {
            add: add.to_vec(),
            remove: remove.to_vec(),
            images: BTreeMap::new(),
        }
    }

    /// [`super::plan`], with what the catalogue's services say of themselves.
    fn plan(config: &HubConfig, change: &ServiceChange) -> Result<ServicePlan, CreateError> {
        super::plan(config, &support::said(), change)
    }

    #[test]
    fn adding_and_removing_is_planned_in_generation_order() {
        let plan = plan(
            &hub(),
            &change(&[ServiceId::Kuvert, ServiceId::Bank], &[ServiceId::Kraph]),
        )
        .expect("a valid change");
        assert_eq!(plan.added, [ServiceId::Bank, ServiceId::Kuvert]);
        assert_eq!(plan.removed, [ServiceId::Kraph]);
        assert!(plan.services.contains(&ServiceId::Bank));
        assert!(!plan.services.contains(&ServiceId::Kraph));
        assert!(
            plan.notes.iter().any(|n| n.contains("Kraph is kept")),
            "{:?}",
            plan.notes
        );
    }

    /// It has an image now. What it brings with it is said before anybody accepts it.
    #[test]
    fn adding_lovekit_says_what_it_opens() {
        let mut config = hub();
        let plan = plan(&config, &change(&[ServiceId::Lovekit], &[])).unwrap();
        assert_eq!(plan.added, [ServiceId::Lovekit]);
        assert!(
            plan.notes.iter().any(|n| n.contains("2758/udp")),
            "{:?}",
            plan.notes
        );

        // Applied, the media server exists, and only while Lovekit runs.
        apply_plan(&mut config, &plan);
        config.provide(&support::said());
        assert!(config.running_livekit().is_some());
    }

    #[test]
    fn a_service_in_both_lists_is_refused() {
        let error = plan(&hub(), &change(&[ServiceId::Bank], &[ServiceId::Bank])).unwrap_err();
        assert!(
            error.to_string().contains("both added and removed"),
            "{error}"
        );
    }

    #[test]
    fn a_change_that_changes_nothing_says_why() {
        let error = plan(&hub(), &change(&[ServiceId::Mikro], &[ServiceId::Bank])).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("Mikro already running"), "{message}");
        assert!(message.contains("Bank not running"), "{message}");
    }

    #[test]
    fn rekuest_stays_while_a_hooked_service_runs() {
        let error = plan(&hub(), &change(&[], &[ServiceId::Rekuest])).unwrap_err();
        assert!(error.to_string().contains("keep Rekuest"), "{error}");

        // Without any hooked service left, it may go — Kraph is not hooked.
        let everything_hooked: Vec<ServiceId> = hub()
            .enabled_services()
            .into_iter()
            .filter(|id| support::HOOKED.contains(id))
            .chain([ServiceId::Rekuest])
            .collect();
        let plan = plan(&hub(), &change(&[], &everything_hooked)).expect("allowed");
        assert_eq!(plan.services, [ServiceId::Kraph]);
    }

    /// Which services keep Rekuest in a hub is what their images say, not a list here: a
    /// service nobody has heard of that names Rekuest among its peers, or offers it a
    /// hook, holds it as firmly as Mikro does — and one that says neither does not.
    #[test]
    fn hooked_by_rekuest_follows_the_description() {
        use crate::contract::{Description, Needs, Offers};
        let example = ServiceId::named("example");
        let hub_with = |description: Description| {
            let mut config = support::hub(&HubConfigOptions {
                services: Some(vec![ServiceId::Kraph, example]),
                ..Default::default()
            });
            config.set_service_image("example", "example:1");
            let mut said = support::said();
            said.insert("example".to_string(), description);
            config.provide(&said);
            (config, said)
        };
        let remove_rekuest = change(&[], &[ServiceId::Rekuest]);

        // It says nothing of Rekuest: Rekuest may go (Kraph is not hooked either).
        let (config, said) = hub_with(support::example());
        assert!(!support::example().hooked_by_rekuest());
        let gone = super::plan(&config, &said, &remove_rekuest).expect("allowed");
        assert_eq!(gone.services, [ServiceId::Kraph, example]);

        // It names Rekuest as a peer.
        let peer = Description {
            needs: Needs {
                peers: vec!["rekuest".into()],
                ..support::example().needs
            },
            ..support::example()
        };
        assert!(peer.hooked_by_rekuest());
        let (config, said) = hub_with(peer);
        let refused = super::plan(&config, &said, &remove_rekuest).unwrap_err();
        assert!(refused.to_string().contains("example"), "{refused}");

        // Or it offers the endpoint Rekuest calls.
        let hook = Description {
            offers: Offers {
                endpoints: BTreeMap::from([("rekuest_hook".to_string(), "_hook".to_string())]),
                ..support::example().offers
            },
            ..support::example()
        };
        assert!(hook.hooked_by_rekuest());
        let (config, said) = hub_with(hook);
        assert!(super::plan(&config, &said, &remove_rekuest).is_err());

        // Of a hub nothing was said about, nothing is known to be hooked.
        let (config, _) = hub_with(support::example());
        assert!(super::plan(&config, &Said::new(), &remove_rekuest).is_ok());
    }

    /// A service outside the catalogue is added by its image, and by nothing less: a name
    /// alone names nothing that could be started.
    #[test]
    fn an_unknown_service_is_added_by_its_image() {
        let example = ServiceId::named("example");
        let mut config = hub();
        let refused = plan(&config, &change(&[example], &[])).unwrap_err();
        assert!(refused.to_string().contains("--image"), "{refused}");

        let by_image = ServiceChange {
            add: vec![example],
            remove: Vec::new(),
            images: BTreeMap::from([(example, "example:1".to_string())]),
        };
        let added = plan(&config, &by_image).expect("it has an image now");
        // After the catalogue's services, where generation puts it.
        assert_eq!(added.added, [example]);
        assert_eq!(added.services.last(), Some(&example));
        apply_plan(&mut config, &added);
        let block = config.service(example);
        assert!(block.runs());
        assert_eq!(block.image.as_deref(), Some("example:1"));
        assert_eq!(block.github_repo, None);

        // Taken out and added back by name: the block remembers its image.
        let out = plan(&config, &change(&[], &[example])).unwrap();
        apply_plan(&mut config, &out);
        assert!(config.service(example).retained && !config.runs(example));
        let back = plan(&config, &change(&[example], &[])).expect("its block has an image");
        apply_plan(&mut config, &back);
        assert!(config.runs(example));
    }

    #[test]
    fn the_last_service_cannot_go() {
        let all = hub().enabled_services();
        let error = plan(&hub(), &change(&[], &all)).unwrap_err();
        assert!(
            error.to_string().contains("at least one service"),
            "{error}"
        );
    }

    /// Ollama is Alpaka's provider: it goes with Alpaka, and comes back with it, while the
    /// block (and the models volume it names) stays in the profile.
    #[test]
    fn ollama_follows_alpaka_out_and_back() {
        use crate::config::hub::OllamaBlock;
        use crate::generate::compose::build_compose;

        let has_ollama = |config: &HubConfig| {
            build_compose(config, &config.enabled_services(), &Default::default())["services"]
                .get("ollama")
                .is_some()
        };
        let mut config = hub();
        config.local_ollama = Some(OllamaBlock::local());
        assert!(has_ollama(&config));

        let out = plan(&config, &change(&[], &[ServiceId::Alpaka])).unwrap();
        apply_plan(&mut config, &out);
        assert!(!has_ollama(&config));
        assert!(config.local_ollama.is_some());

        let back = plan(&config, &change(&[ServiceId::Alpaka], &[])).unwrap();
        apply_plan(&mut config, &back);
        assert!(has_ollama(&config));
    }

    /// Adding Bank rewrites the Caddyfile (its route), Rekuest's config (its HookAgent) and
    /// — with an inline trust bundle — every other service's: those containers are
    /// restarted, since compose does not notice a bind-mounted file changing. Bank itself
    /// is started by `up`, and the bucket manifest is replayed on its own.
    #[test]
    fn a_changed_config_restarts_what_reads_it() {
        let mut config = hub();
        let added = plan(&config, &change(&[ServiceId::Bank], &[])).unwrap();
        apply_plan(&mut config, &added);

        let changed: Vec<String> = [
            "Caddyfile",
            "bank.yaml",
            "rekuest.yaml",
            "mikro.yaml",
            "rustfs_init.yaml",
            "kraph.yaml.bak",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            services_to_restart(&config, &changed, &added),
            ["gateway", "rekuest", "rekuest-takt", "mikro"]
        );

        // A service taken out is not restarted: `up --remove-orphans` removes it.
        let mut config = hub();
        let removed = plan(&config, &change(&[], &[ServiceId::Kraph])).unwrap();
        apply_plan(&mut config, &removed);
        let changed: Vec<String> = ["Caddyfile", "kraph.yaml"].map(String::from).to_vec();
        assert_eq!(
            services_to_restart(&config, &changed, &removed),
            ["gateway"]
        );
    }

    #[test]
    fn a_standalone_apply_restarts_every_config_reader() {
        let config = hub();
        let all = every_config_reader(&config);
        assert_eq!(all[0], "gateway");
        assert!(all.contains(&"rekuest-takt".to_string()));
        assert!(all.contains(&"mikro".to_string()));
        assert!(!all.contains(&"bank".to_string()));
    }

    #[test]
    fn database_names_are_checked_before_they_reach_sql() {
        assert!(plain_identifier("bank"));
        assert!(plain_identifier("kuvert_2"));
        assert!(!plain_identifier("bank; DROP DATABASE mikro"));
        assert!(!plain_identifier("Bank"));
        assert!(!plain_identifier("2bank"));
        assert!(!plain_identifier(""));
    }
}
