//! Starts the hub an earlier release generated, on the images of its time, and updates it.
//!
//! `hub_health` proves a hub created today works. This proves the hubs that already exist
//! get there: `fixtures/releases/0.13.0` is what Konstruktor 0.13.0 wrote, and the services
//! it then pulled as `latest` are pinned here to what `latest` was. One `updates::apply`
//! has to carry that to a hub on today's images that passes what `hub_health` asks — and
//! an update that fails before it replaces anything has to leave the hub as it found it.
//!
//! Opt-in twice over, like `hub_health`: `#[ignore]`, and `KONSTRUKTOR_E2E=1`.
//!
//! ```sh
//! KONSTRUKTOR_E2E=1 cargo test -p konstruktor-core --test hub_upgrade -- --ignored --nocapture
//! ```
//!
//! The fixture names its ports (18480, 18443), so two runs at once collide.

use std::path::{Path, PathBuf};
use std::time::Duration;

use konstruktor_core::health::{self, ServiceHealth};
use konstruktor_core::migrate;
use konstruktor_core::profile::{self, read_profile};
use konstruktor_core::updates::{self, UpdateEvent, UpdateRequest};

/// What the services' `latest` was when 0.13.0 was current.
const IMAGES_OF_ITS_TIME: [(&str, &str); 6] = [
    ("jhnnsrs/rekuest:latest", "jhnnsrs/rekuest:5.2.0"),
    ("jhnnsrs/rekuest-takt:latest", "jhnnsrs/rekuest-takt:5.2.0"),
    ("jhnnsrs/mikro:latest", "jhnnsrs/mikro:4.0.0"),
    ("jhnnsrs/fluss:latest", "jhnnsrs/fluss:2.3.0"),
    ("jhnnsrs/kabinet:latest", "jhnnsrs/kabinet:3.4.0"),
    ("jhnnsrs/kraph:latest", "jhnnsrs/kraph:1.1.0"),
];

fn compose(dir: &Path, args: &[&str]) -> std::process::Output {
    konstruktor_core::docker::command()
        .args(["compose"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("docker runs")
}

fn secs(var: &str, default: u64) -> Duration {
    Duration::from_secs(
        std::env::var(var)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(default),
    )
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Takes the hub down — volumes included — however the test ends, a failed assertion too.
struct Teardown(PathBuf);

impl Drop for Teardown {
    fn drop(&mut self) {
        eprintln!("tearing the hub down…");
        compose(&self.0, &["down", "--volumes", "--remove-orphans"]);
    }
}

fn logs(dir: &Path, service: &str) {
    let out = compose(dir, &["logs", "--no-color", "--tail", "120", service]);
    eprintln!(
        "----- logs: {service} -----\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `health::check`, failing with the unhealthy services' logs.
async fn healthy(dir: &Path) -> Vec<ServiceHealth> {
    let config = read_profile(dir).expect("the profile").config;
    let results = health::check(dir, &config, &|event| eprintln!("{event:?}"))
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

/// The image each of the hub's containers runs, by compose service.
fn running_images(dir: &Path) -> std::collections::BTreeMap<String, String> {
    let out = compose(dir, &["ps", "--format", "{{.Service}}|{{.Image}}"]);
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let (service, image) = line.split_once('|')?;
            Some((service.trim().to_string(), image.trim().to_string()))
        })
        .collect()
}

fn psql(dir: &Path, sql: &str) -> String {
    let config = read_profile(dir).expect("the profile").config;
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
            sql,
        ],
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The names in one of the lists rekuest is configured with.
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

/// An organization, made by Rekuest itself so that it reacts to it. See `hub_health`.
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

async fn update(dir: &Path) -> Result<updates::UpdateReport, updates::UpdateError> {
    updates::apply(
        dir,
        &UpdateRequest {
            services: Vec::new(),
            advances: Vec::new(),
            pull: true,
            backup_into: None,
            health_check: true,
        },
        &|event| match event {
            UpdateEvent::Line { line, .. } => eprintln!("  {line}"),
            other => eprintln!("{other:?}"),
        },
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a whole hub in Docker; run with KONSTRUKTOR_E2E=1 and --ignored"]
async fn a_hub_of_0_13_is_updated_onto_todays_releases() {
    if std::env::var("KONSTRUKTOR_E2E").as_deref() != Ok("1") {
        eprintln!("skipping: set KONSTRUKTOR_E2E=1 to spawn a hub");
        return;
    }

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hub-upgrade-e2e");
    if dir.exists() {
        compose(&dir, &["down", "--volumes", "--remove-orphans"]);
        std::fs::remove_dir_all(&dir).expect("old hub dir is removed");
    }
    copy(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/releases/0.13.0"),
        &dir,
    );
    // The profile keeps following `latest`, as that hub's does; only what is run says
    // what `latest` was.
    let compose_file = dir.join("docker-compose.yaml");
    let mut text = std::fs::read_to_string(&compose_file).unwrap();
    for (latest, then) in IMAGES_OF_ITS_TIME {
        text = text.replace(&format!("image: {latest}\n"), &format!("image: {then}\n"));
    }
    std::fs::write(&compose_file, &text).unwrap();
    let config = read_profile(&dir).expect("0.13.0's profile is read").config;
    assert_eq!(migrate::layout(&dir, &config), 2);

    let _teardown = Teardown(dir.clone());
    let up = compose(&dir, &["up", "-d"]);
    assert!(
        up.status.success(),
        "docker compose up failed:\n{}",
        String::from_utf8_lossy(&up.stderr)
    );
    let settle = secs("KONSTRUKTOR_E2E_SETTLE_SECS", 60);
    eprintln!(
        "the 0.13 hub is up; letting it settle for {}s…",
        settle.as_secs()
    );
    tokio::time::sleep(settle).await;
    healthy(&dir).await;
    let before = running_images(&dir);
    assert_eq!(before["rekuest"], "jhnnsrs/rekuest:5.2.0");

    // --- an update that cannot fetch an image changes nothing --------------------------
    let profile_text = std::fs::read_to_string(profile::profile_path(&dir)).unwrap();
    let mut broken = read_profile(&dir).unwrap();
    broken
        .config
        .set_service_image("kraph", "jhnnsrs/kraph:no-such-release");
    profile::write_profile(&dir, &broken).unwrap();
    let refused = update(&dir).await;
    assert!(refused.is_err(), "an image that does not exist was fetched");
    assert_eq!(
        std::fs::read_to_string(&compose_file).unwrap(),
        text,
        "the compose file was not put back"
    );
    assert!(
        !std::fs::read_to_string(dir.join("configs/rekuest.yaml"))
            .unwrap()
            .contains("takt_socket"),
        "rekuest's config was not put back"
    );
    assert_eq!(migrate::layout(&dir, &config), 2);
    assert_eq!(running_images(&dir), before, "a container was replaced");
    std::fs::write(profile::profile_path(&dir), profile_text).unwrap();

    // --- the update ---------------------------------------------------------------------
    let report = update(&dir).await.expect("the update runs");
    assert_eq!(report.migrated.len(), 2, "{:?}", report.migrated);
    assert!(report.refused.is_empty(), "{:?}", report.refused);
    for service in ["rekuest", "mikro", "fluss", "kabinet", "kraph"] {
        assert!(
            report.updated.iter().any(|s| s == service),
            "{service} was not updated: {:?}",
            report.updated
        );
    }
    assert_eq!(migrate::layout(&dir, &config), migrate::CURRENT_LAYOUT);
    let after = running_images(&dir);
    // On the major the files were written for, not on wherever `latest` goes next.
    let seeded = konstruktor_core::config::hub::build_hub_config(&Default::default());
    assert_eq!(Some(&after["rekuest"]), seeded.rekuest.image.as_ref());
    assert_eq!(Some(&after["rekuest-takt"]), seeded.takt_image().as_ref());
    assert_eq!(Some(&after["mikro"]), seeded.mikro.image.as_ref());
    assert_eq!(
        read_profile(&dir).unwrap().config.rekuest.image,
        seeded.rekuest.image
    );

    let results = healthy(&dir).await;
    let takt = results
        .iter()
        .find(|s| s.service == "rekuest-takt")
        .expect("the health check looked at rekuest-takt");
    assert!(takt.healthy, "rekuest-takt: {}", takt.detail);

    // --- and it does what a hub created today does --------------------------------------
    let services = configured(&dir, "services");
    let agents = configured(&dir, "hook_agents");
    assert!(!services.is_empty() && !agents.is_empty());
    create_organization(&dir, "e2e");
    let deadline = std::time::Instant::now() + secs("KONSTRUKTOR_E2E_PROVISION_SECS", 180);
    loop {
        let catalogued = psql(&dir, "select name from facade_service");
        let provisioned = psql(
            &dir,
            "select a.name from facade_agent a join facade_implementation i on i.agent_id = a.id \
             where a.kind = 'WEBHOOK' group by a.name",
        );
        let has = |found: &str, name: &String| found.lines().any(|line| line.trim() == name);
        let missing: Vec<String> = services
            .iter()
            .filter(|name| !has(&catalogued, name))
            .map(|name| format!("service {name}"))
            .chain(
                agents
                    .iter()
                    .filter(|name| !has(&provisioned, name))
                    .map(|name| format!("hook agent {name}")),
            )
            .collect();
        if missing.is_empty() {
            break;
        }
        if std::time::Instant::now() > deadline {
            logs(&dir, "rekuest-takt");
            logs(&dir, "rekuest");
            panic!("after the update rekuest still lacks {missing:?}");
        }
        eprintln!("waiting for rekuest to provision {missing:?}…");
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}
