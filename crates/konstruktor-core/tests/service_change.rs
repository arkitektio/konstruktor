//! Adds a service to a running hub and takes it out again, against real Docker.
//!
//! `tests/reauthorize.rs` covers what a service change writes; this covers what applying it
//! does to a stack that is already running — the part no mock reaches. Postgres runs its init
//! script only on an empty data directory, so the added service's database has to be created
//! in the running cluster; compose does not look into bind-mounted files, so the gateway and
//! Rekuest have to be restarted to read their rewritten configs.
//!
//! No coordination server: the change is folded into the profile the way `reauthorize` does
//! it once the grant is accepted, and then `apply_services` runs exactly as
//! `change_services` would call it. Opt-in like `hub_health`:
//!
//! ```sh
//! KONSTRUKTOR_E2E=1 cargo test -p konstruktor-core --test service_change -- --ignored --nocapture
//! ```
//!
//! `KONSTRUKTOR_E2E_SETTLE_SECS` (default 60) and `KONSTRUKTOR_E2E_PROVISION_SECS`
//! (default 180) mean what they mean there. `KONSTRUKTOR_E2E_BANK_IMAGE` runs bank from
//! another image than the profile's — a local build of its source, when the published one
//! lags behind it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use konstruktor_core::catalog::ServiceId;
use konstruktor_core::compose::ComposeLine;
use konstruktor_core::config::hub::{build_hub_config, HubConfig, HubConfigOptions};
use konstruktor_core::generate::write::write_generated_files;
use konstruktor_core::generate::{generate_hub_files, IssuedIdentity};
use konstruktor_core::health::{self, ServiceHealth};
use konstruktor_core::profile::{hub_profile, read_profile, write_profile};
use konstruktor_core::services::{self, ServiceChange};

fn compose(dir: &Path, args: &[&str]) -> std::process::Output {
    konstruktor_core::docker::command()
        .args(["compose"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("docker runs")
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("a free port")
}

fn secs(var: &str, default: u64) -> Duration {
    Duration::from_secs(
        std::env::var(var)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(default),
    )
}

struct Teardown(PathBuf);

impl Drop for Teardown {
    fn drop(&mut self) {
        eprintln!("tearing the hub down…");
        compose(&self.0, &["down", "--volumes", "--remove-orphans"]);
    }
}

fn logs(dir: &Path, service: &str) {
    let out = compose(dir, &["logs", "--no-color", "--tail", "80", service]);
    eprintln!(
        "----- logs: {service} -----\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `health::check`, failing with the unhealthy services' logs.
async fn healthy(dir: &Path, config: &HubConfig) -> Vec<ServiceHealth> {
    let results = health::check(dir, config, &|event| eprintln!("{event:?}"))
        .await
        .expect("the engine answers");
    let unhealthy: Vec<&ServiceHealth> = results.iter().filter(|s| !s.healthy).collect();
    if !unhealthy.is_empty() {
        for s in &unhealthy {
            logs(dir, &s.service);
        }
        panic!(
            "unhealthy: {:?}",
            unhealthy
                .iter()
                .map(|s| format!("{}: {}", s.service, s.detail))
                .collect::<Vec<_>>()
        );
    }
    results
}

/// What `reauthorize` does with a service change once it is accepted — plan, fold into
/// the profile, keys and secrets, regenerate, write — and then what `change_services` does
/// to apply it. Returns the profile as written.
async fn change_and_apply(dir: &Path, add: &[ServiceId], remove: &[ServiceId]) -> HubConfig {
    let mut config = read_profile(dir).expect("the profile").config;
    let plan = services::plan(
        &config,
        &ServiceChange {
            add: add.to_vec(),
            remove: remove.to_vec(),
        },
    )
    .expect("a valid change");
    services::apply_plan(&mut config, &plan);
    // A published image can lag its source (bank's did, across the move to instance
    // keys); point at a local build instead.
    if let Ok(image) = std::env::var("KONSTRUKTOR_E2E_BANK_IMAGE") {
        if config.bank.enabled {
            config.bank.image = Some(image);
        }
    }
    config.ensure_instance_keys();
    config.ensure_service_secrets();

    let before = services::snapshot_configs(dir);
    let files = generate_hub_files(&config, &IssuedIdentity::default());
    write_profile(dir, &hub_profile(config.clone())).expect("profile is written");
    write_generated_files(dir, &files).expect("files are written");
    let changed = services::changed_configs(&before, &services::snapshot_configs(dir));
    let restart = services::services_to_restart(&config, &changed, &plan);
    eprintln!("changed configs {changed:?}; restarting {restart:?}");

    let print = |line: ComposeLine| eprintln!("  {}", line.line);
    services::apply_services(dir, &restart, &print)
        .await
        .expect("the change applies");
    config
}

fn psql(dir: &Path, config: &HubConfig, database: &str, sql: &str) -> String {
    let out = compose(
        dir,
        &[
            "exec",
            "-T",
            "db",
            "psql",
            "-U",
            &config.db.postgres_user,
            "-d",
            database,
            "-tAc",
            sql,
        ],
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn running(dir: &Path, service: &str) -> bool {
    let out = compose(dir, &["ps", "--status", "running", "-q", service]);
    !String::from_utf8_lossy(&out.stdout).trim().is_empty()
}

/// An organization, made the way the coordination server's first token would make it: by
/// Rekuest itself, so that it reacts to it. Rekuest gives a new organization its agents on
/// a thread of its own, which this short-lived process has to wait for.
fn create_organization(dir: &Path, slug: &str) {
    let code = format!(
        "import threading\n\
         from authentikate.models import Organization\n\
         Organization.objects.create(slug='{slug}')\n\
         [t.join() for t in threading.enumerate() if t.name == 'provision-{slug}']"
    );
    let out = compose(
        dir,
        &[
            "exec",
            "-T",
            "rekuest",
            "python",
            "manage.py",
            "shell",
            "-c",
            &code,
        ],
    );
    assert!(
        out.status.success(),
        "the organization was not created:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Rekuest's WEBHOOK agents with how many actions each has — as `hub_health` reads them.
fn provisioned_agents(dir: &Path, config: &HubConfig) -> std::collections::BTreeMap<String, u32> {
    let query = "select a.name, count(i.id) from facade_agent a \
                 left join facade_implementation i on i.agent_id = a.id \
                 where a.kind = 'WEBHOOK' group by a.name";
    let out = compose(
        dir,
        &[
            "exec",
            "-T",
            "db",
            "psql",
            "-U",
            &config.db.postgres_user,
            "-d",
            &config.rekuest.db_config.db,
            "-tAF",
            "|",
            "-c",
            query,
        ],
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let (name, count) = line.split_once('|')?;
            Some((name.trim().to_string(), count.trim().parse().ok()?))
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a whole hub in Docker; run with KONSTRUKTOR_E2E=1 and --ignored"]
async fn bank_is_added_to_a_running_hub_and_removed_keeping_its_data() {
    if std::env::var("KONSTRUKTOR_E2E").as_deref() != Ok("1") {
        eprintln!("skipping: set KONSTRUKTOR_E2E=1 to spawn a hub");
        return;
    }

    let config = build_hub_config(&HubConfigOptions {
        device_id: "e2e".into(),
        coord_server: "go.arkitekt.live".into(),
        http_port: Some(free_port()),
        https_port: Some(free_port()),
        ..Default::default()
    });
    assert!(!config.bank.enabled);

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("service-change-e2e");
    if dir.exists() {
        compose(&dir, &["down", "--volumes", "--remove-orphans"]);
        std::fs::remove_dir_all(&dir).expect("old hub dir is removed");
    }
    std::fs::create_dir_all(&dir).expect("hub dir");
    write_profile(&dir, &hub_profile(config.clone())).expect("profile is written");
    write_generated_files(
        &dir,
        &generate_hub_files(&config, &IssuedIdentity::default()),
    )
    .expect("files are written");

    let _teardown = Teardown(dir.clone());
    let up = compose(&dir, &["up", "-d"]);
    assert!(
        up.status.success(),
        "docker compose up failed:\n{}",
        String::from_utf8_lossy(&up.stderr)
    );
    let settle = secs("KONSTRUKTOR_E2E_SETTLE_SECS", 60);
    eprintln!("hub is up; letting it settle for {}s…", settle.as_secs());
    tokio::time::sleep(settle).await;
    healthy(&dir, &config).await;
    // Hook agents belong to organizations, and a hub that never met its coordination
    // server has none: this is the one bank's agent is expected in further down.
    create_organization(&dir, "e2e");
    assert_eq!(
        psql(
            &dir,
            &config,
            "postgres",
            "SELECT 1 FROM pg_database WHERE datname = 'bank'"
        ),
        "",
        "a fresh hub without bank has no bank database"
    );

    // --- add bank -----------------------------------------------------------------------
    let config = change_and_apply(&dir, &[ServiceId::Bank], &[]).await;
    assert!(config.bank.enabled);

    // Its database, created in the running cluster, as the init script would have.
    assert_eq!(
        psql(
            &dir,
            &config,
            "postgres",
            "SELECT 1 FROM pg_database WHERE datname = 'bank'"
        ),
        "1",
        "the bank database was not created"
    );
    assert_eq!(
        psql(
            &dir,
            &config,
            "postgres",
            "SELECT 1 FROM pg_roles WHERE rolname = 'bank'"
        ),
        "1",
        "the bank role was not created"
    );
    assert_eq!(
        psql(
            &dir,
            &config,
            "bank",
            "SELECT 1 FROM pg_extension WHERE extname = 'vector'"
        ),
        "1",
        "the vector extension is missing in bank"
    );

    // Running, and answering through the gateway — which only routes `/bank` once it has
    // been restarted onto the rewritten Caddyfile.
    if !running(&dir, "bank") {
        logs(&dir, "bank");
        panic!("the bank container is not running");
    }
    let results = healthy(&dir, &config).await;
    let bank = results
        .iter()
        .find(|s| s.service == "bank")
        .expect("the health check asked bank");
    assert!(
        bank.url.as_deref().is_some_and(|u| u.contains("/bank/")),
        "{:?}",
        bank.url
    );

    // Rekuest and takt, restarted onto the rewritten rekuest.yaml, provision bank's agent.
    let deadline = std::time::Instant::now() + secs("KONSTRUKTOR_E2E_PROVISION_SECS", 180);
    let agents = loop {
        let found = provisioned_agents(&dir, &config);
        if found.get("bank").copied().unwrap_or(0) > 0 || std::time::Instant::now() > deadline {
            break found;
        }
        eprintln!("waiting for rekuest to provision bank (have {found:?})…");
        tokio::time::sleep(Duration::from_secs(10)).await;
    };
    if agents.get("bank").copied().unwrap_or(0) == 0 {
        logs(&dir, "rekuest-takt");
        logs(&dir, "bank");
        logs(&dir, "rekuest");
        panic!("rekuest did not provision bank's hook agent: {agents:?}");
    }

    // --- remove it again ----------------------------------------------------------------
    let config = change_and_apply(&dir, &[], &[ServiceId::Bank]).await;
    assert!(!config.bank.enabled && config.bank.retained);
    // Asked of the engine, not of compose: bank is no longer in the file, and compose
    // would answer "no such service" with nothing on stdout either way.
    let project = konstruktor_core::compose::project_name(&dir.to_string_lossy());
    let listed = konstruktor_core::docker::command()
        .args([
            "ps",
            "--all",
            "-q",
            "--filter",
            &format!("label=com.docker.compose.project={project}"),
            "--filter",
            "label=com.docker.compose.service=bank",
        ])
        .output()
        .expect("docker runs");
    assert!(
        String::from_utf8_lossy(&listed.stdout).trim().is_empty(),
        "the bank container is still there"
    );
    assert_eq!(
        psql(
            &dir,
            &config,
            "postgres",
            "SELECT 1 FROM pg_database WHERE datname = 'bank'"
        ),
        "1",
        "removing bank lost its database"
    );
    // The rest of the hub is untouched by all of it.
    healthy(&dir, &config).await;
}
