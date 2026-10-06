//! Spawns a real hub and asks every service whether it works.
//!
//! `compose_validity` proves Docker accepts the generated project; this proves the project
//! actually *runs* — every container stays up, Postgres accepts connections, and each
//! service answers its health check through the gateway. It pulls every image and takes
//! minutes, so it is opt-in twice over: `#[ignore]`, and `KONSTRUKTOR_E2E=1`.
//!
//! ```sh
//! KONSTRUKTOR_E2E=1 cargo test -p konstruktor-core --test hub_health -- --ignored --nocapture
//! ```
//!
//! `KONSTRUKTOR_E2E_SETTLE_SECS` (default 60) is how long the hub gets after `up` before it
//! is asked; `health::check` then waits and retries on its own on top of that.
//! `KONSTRUKTOR_E2E_PROVISION_SECS` (default 180) is how long Rekuest then gets to provision
//! a HookAgent for every hooked service.
//!
//! `KONSTRUKTOR_E2E_IMAGES` runs the hub on other images than the seeded ones, as
//! `service=image` pairs separated by commas (`rekuest=jhnnsrs/rekuest:6.1.0-rc.1`; takt
//! follows Rekuest unless `rekuest-takt=` names its own). It is how a service asks, before
//! a release is published, whether a hub written by this Konstruktor runs on it.
//!
//! No coordination server is involved: the hub is generated directly, the way
//! `create_hub` does after the authorization, with a default issued identity.
//!
//! The compose project is left to take its name from the directory, as a real hub's does:
//! `health::check` runs its own `compose exec` there without a `-p`, so a pinned project
//! name would have it looking for Postgres in a project that does not exist.

use std::path::{Path, PathBuf};
use std::time::Duration;

use konstruktor_core::config::hub::HubConfigOptions;
use konstruktor_core::generate::{generate_hub_files, IssuedIdentity};
use konstruktor_core::health::{self, ServiceHealth};
use konstruktor_core::profile::{hub_profile, write_profile};

mod support;

const DEFAULT_SETTLE: Duration = Duration::from_secs(60);

fn docker_compose_available() -> bool {
    konstruktor_core::docker::command()
        .args(["compose", "version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn compose(dir: &Path, args: &[&str]) -> std::process::Output {
    konstruktor_core::docker::command()
        .args(["compose"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("docker runs")
}

/// A port nothing is listening on right now, so the test never collides with a real hub
/// on its default port on the same machine.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("a free port")
}

fn settle_time() -> Duration {
    std::env::var("KONSTRUKTOR_E2E_SETTLE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_SETTLE)
}

/// Takes the hub down — volumes included — however the test ends, a failed assertion too.
struct Teardown(PathBuf);

impl Drop for Teardown {
    fn drop(&mut self) {
        eprintln!("tearing the hub down…");
        compose(&self.0, &["down", "--volumes", "--remove-orphans"]);
    }
}

fn report(unhealthy: &[&ServiceHealth]) -> String {
    let mut out = String::from("service | state | restarts | http | detail\n");
    for s in unhealthy {
        out.push_str(&format!(
            "{} | {} | {} | {} | {}\n",
            s.service,
            s.container_state.as_deref().unwrap_or("-"),
            s.restarts_seen,
            s.http_status.map_or("-".to_string(), |c| c.to_string()),
            s.detail,
        ));
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a whole hub in Docker; run with KONSTRUKTOR_E2E=1 and --ignored"]
async fn every_service_of_a_fresh_hub_is_healthy() {
    if std::env::var("KONSTRUKTOR_E2E").as_deref() != Ok("1") {
        eprintln!("skipping: set KONSTRUKTOR_E2E=1 to spawn a hub");
        return;
    }
    if !docker_compose_available() {
        eprintln!("skipping: no docker compose on this machine");
        return;
    }

    let mut config = support::hub(&HubConfigOptions {
        device_id: "e2e".into(),
        coord_server: "go.arkitekt.live".into(),
        http_port: Some(free_port()),
        https_port: Some(free_port()),
        ..Default::default()
    });
    let named = std::env::var("KONSTRUKTOR_E2E_IMAGES").unwrap_or_default();
    for pair in named.split(',').filter(|pair| !pair.trim().is_empty()) {
        let (service, image) = pair
            .trim()
            .split_once('=')
            .expect("KONSTRUKTOR_E2E_IMAGES is service=image pairs");
        eprintln!("{service} runs {image}");
        config.set_service_image(service, image);
    }
    let files = generate_hub_files(&config, &IssuedIdentity::default(), &Default::default());

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hub-e2e");
    // A run that crashed hard (no drop) may have left its stack behind.
    if dir.exists() {
        compose(&dir, &["down", "--volumes", "--remove-orphans"]);
        std::fs::remove_dir_all(&dir).expect("old hub dir is removed");
    }
    std::fs::create_dir_all(&dir).expect("hub dir");
    write_profile(&dir, &hub_profile(config.clone())).expect("profile is written");
    konstruktor_core::migrate::write_hub(&dir, &config, &files).expect("files are written");

    let _teardown = Teardown(dir.clone());
    // Started as `konstruktor up` starts it: every image is fetched and its build written
    // into the compose file before the first container exists, so the run is on today's
    // releases and the hub is on exact builds from its first second.
    // Fetched and asserted first: `start` forgives an image it cannot fetch, which is
    // right for a hub and wrong for a test — a run on last week's cached build proves
    // nothing about today's. An image named for this run may exist on this machine alone.
    let mut pull_args = vec!["pull", "--quiet"];
    if !named.trim().is_empty() {
        pull_args.push("--ignore-pull-failures");
    }
    let pull = compose(&dir, &pull_args);
    assert!(
        pull.status.success(),
        "docker compose pull failed:\n{}",
        String::from_utf8_lossy(&pull.stderr)
    );
    let started = konstruktor_core::start::start(&dir, &|line| eprintln!("  {}", line.line)).await;
    assert!(
        started.is_ok(),
        "the hub did not start: {:?}",
        started.err()
    );
    // A service whose image answers the hub contract wrote its own config, from the facts
    // this installer wrote for it — every image named for this run is expected to.
    let self_written = konstruktor_core::lock::read(&dir).rendered;
    eprintln!(
        "configs written by their own images: {:?}",
        self_written.keys().collect::<Vec<_>>()
    );
    for pair in named.split(',').filter(|pair| !pair.trim().is_empty()) {
        let service = pair.trim().split('=').next().unwrap_or_default();
        if config
            .enabled_services()
            .into_iter()
            .any(|id| config.service(id).host == service)
        {
            assert!(
                self_written.contains_key(service)
                    && dir.join(format!("facts/{service}.yaml")).is_file(),
                "{service}'s image did not write its own config"
            );
        }
    }
    // Their databases were prepared by that start, for the builds they run — so the next
    // start of the same builds has nothing to prepare, and the services only serve.
    assert_eq!(
        konstruktor_core::contract::unprepared(&dir, &config).await,
        Vec::<(String, String)>::new()
    );
    let pins = konstruktor_core::lock::read(&dir).pins;
    let written = std::fs::read_to_string(dir.join("docker-compose.yaml")).unwrap();
    // Not of an image named for this run: one built on this machine has no build to name.
    for service in ["mikro", "kabinet", "db"]
        .into_iter()
        .filter(|service| !named.contains(&format!("{service}=")))
    {
        let build = pins
            .get(service)
            .and_then(|pin| pin.reference())
            .unwrap_or_else(|| panic!("{service} was started without a build written down"));
        assert!(
            written.contains(&format!("image: {build}\n")),
            "the compose file does not name {build}"
        );
    }

    let settle = settle_time();
    eprintln!("hub is up; letting it settle for {}s…", settle.as_secs());
    tokio::time::sleep(settle).await;

    let results = health::check(&dir, &config, &|event| eprintln!("{event:?}"))
        .await
        .expect("the engine answers");

    assert!(!results.is_empty(), "the health check looked at nothing");
    let unhealthy: Vec<&ServiceHealth> = results.iter().filter(|s| !s.healthy).collect();
    if !unhealthy.is_empty() {
        // The logs are what makes a CI failure diagnosable without re-running it.
        for s in &unhealthy {
            let logs = compose(&dir, &["logs", "--no-color", "--tail", "80", &s.service]);
            eprintln!(
                "----- logs: {} -----\n{}{}",
                s.service,
                String::from_utf8_lossy(&logs.stdout),
                String::from_utf8_lossy(&logs.stderr)
            );
        }
        panic!("unhealthy services:\n{}", report(&unhealthy));
    }

    // --- takt ----------------------------------------------------------------------
    // Rekuest's other half: the agent protocol, every deadline and schedule, and the clock
    // of Rekuest's upkeep jobs. `health::check` judges it by its image's own healthcheck.
    // It has to be there and healthy: without it no agent connects and nothing fires.
    let takt = results
        .iter()
        .find(|s| s.service == "rekuest-takt")
        .expect("the health check looked at rekuest-takt");
    assert!(takt.healthy, "rekuest-takt: {}", takt.detail);

    // --- the service catalog ------------------------------------------------------------
    // Rekuest catalogues every `rekuest.services` entry when takt asks it to, from what the
    // service answers at `_rekuest/service`. Every request on the way is signed with
    // instance keys, so a catalogued service proves that chain — takt asks, Rekuest asks
    // the service, and each side finds the other's key in the (inline) trust bundle.
    let services = configured(&dir, "services");
    assert!(!services.is_empty(), "the hub has hooked services");
    let deadline = std::time::Instant::now() + provision_timeout();
    let catalogued = loop {
        let found = catalogued_services(
            &dir,
            &config.db.postgres_user,
            config
                .service(konstruktor_core::catalog::ServiceId::Rekuest)
                .database()
                .expect("rekuest has a database"),
        );
        if services.iter().all(|name| found.contains(name)) || std::time::Instant::now() > deadline
        {
            break found;
        }
        eprintln!("waiting for rekuest to catalogue {services:?} (have {found:?})…");
        tokio::time::sleep(Duration::from_secs(10)).await;
    };
    let missing: Vec<&String> = services
        .iter()
        .filter(|name| !catalogued.contains(*name))
        .collect();
    if !missing.is_empty() {
        takt_and_rekuest_logs(&dir);
        panic!("rekuest did not catalogue {missing:?}; its catalog: {catalogued:?}");
    }

    // --- one hook agent per hooked service, in an organization ---------------------------
    // Hook agents belong to organizations, and a hub that never met its coordination server
    // has none: one is created here, which is when Rekuest gives it its agents. Rekuest
    // makes each through takt's internal API, so an agent *with actions* proves Rekuest
    // reaches takt where only it can (the socket the two mount). Every hooked service
    // declares at least its embeddings sweep.
    let expected = configured(&dir, "hook_agents");
    assert!(!expected.is_empty(), "the hub has hook agents");
    create_organization(&dir, "e2e");
    let deadline = std::time::Instant::now() + provision_timeout();
    let provisioned = loop {
        let found = provisioned_agents(
            &dir,
            &config.db.postgres_user,
            config
                .service(konstruktor_core::catalog::ServiceId::Rekuest)
                .database()
                .expect("rekuest has a database"),
        );
        let missing: Vec<&String> = expected
            .iter()
            .filter(|service| found.get(*service).copied().unwrap_or(0) == 0)
            .collect();
        if missing.is_empty() || std::time::Instant::now() > deadline {
            break found;
        }
        eprintln!("waiting for rekuest to provision {missing:?} (have {found:?})…");
        tokio::time::sleep(Duration::from_secs(10)).await;
    };

    eprintln!("rekuest's hook agents and their action counts: {provisioned:?}");
    let missing: Vec<&String> = expected
        .iter()
        .filter(|service| provisioned.get(*service).copied().unwrap_or(0) == 0)
        .collect();
    if !missing.is_empty() {
        takt_and_rekuest_logs(&dir);
        panic!(
            "rekuest did not provision a hook agent with actions for {missing:?}; \
             agents and their action counts: {provisioned:?}"
        );
    }
}

fn takt_and_rekuest_logs(dir: &Path) {
    for service in ["rekuest-takt", "rekuest"] {
        let logs = compose(dir, &["logs", "--no-color", "--tail", "120", service]);
        eprintln!(
            "----- logs: {service} -----\n{}{}",
            String::from_utf8_lossy(&logs.stdout),
            String::from_utf8_lossy(&logs.stderr)
        );
    }
}

/// How long rekuest gets to provision every service agent once the hub is healthy. The
/// takt asks again 30 s after a pass in which a service was unreachable, so one that was slow to boot
/// costs a round or two.
fn provision_timeout() -> Duration {
    std::env::var("KONSTRUKTOR_E2E_PROVISION_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(180))
}

/// The names in one of the lists rekuest was configured with (`rekuest.services`,
/// `rekuest.hook_agents`), as the generated config has them.
fn configured(dir: &Path, list: &str) -> Vec<String> {
    let text = std::fs::read_to_string(dir.join("configs/rekuest.yaml")).expect("rekuest.yaml");
    let doc: serde_norway::Value = serde_norway::from_str(&text).expect("rekuest.yaml parses");
    doc["rekuest"][list]
        .as_sequence()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|e| e["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
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

/// The names in Rekuest's service catalog, read from its database.
fn catalogued_services(dir: &Path, user: &str, database: &str) -> Vec<String> {
    let out = compose(
        dir,
        &[
            "exec",
            "-T",
            "db",
            "psql",
            "-U",
            user,
            "-d",
            database,
            "-tA",
            "-c",
            "select name from facade_service",
        ],
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

/// Rekuest's WEBHOOK agents, by name, with how many actions each implements — read from
/// its database, since the hub has no coordination server to mint a token for its API.
fn provisioned_agents(
    dir: &Path,
    user: &str,
    database: &str,
) -> std::collections::BTreeMap<String, u32> {
    let query = "select a.name, count(i.id) from facade_agent a \
                 left join facade_implementation i on i.agent_id = a.id \
                 where a.kind = 'WEBHOOK' group by a.name";
    let out = compose(
        dir,
        &[
            "exec", "-T", "db", "psql", "-U", user, "-d", database, "-tAF", "|", "-c", query,
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
