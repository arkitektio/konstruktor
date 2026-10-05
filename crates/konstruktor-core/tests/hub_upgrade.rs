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
//! `KONSTRUKTOR_E2E_IMAGES` (`service=image` pairs, as in `hub_health`) updates onto those
//! images instead of the seeded ones — a release that is not published yet. One built on
//! this machine is run as it is; nothing is fetched for it.
//!
//! `KONSTRUKTOR_E2E_FAILING_UPGRADE` names, the same way, a build of Rekuest whose
//! `manage.py upgrade` fails (and the takt to run beside it). With it, a second test
//! checks that such an update leaves the hub exactly as it found it.
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

async fn update(dir: &Path, pull: bool) -> Result<updates::UpdateReport, updates::UpdateError> {
    updates::apply(
        dir,
        &UpdateRequest {
            services: Vec::new(),
            advances: Vec::new(),
            pull,
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

/// The hub 0.13.0 generated, started on the images of its time and healthy.
async fn a_running_hub_of_0_13() -> (PathBuf, Teardown) {
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

    let teardown = Teardown(dir.clone());
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
    assert_eq!(running_images(&dir)["rekuest"], "jhnnsrs/rekuest:5.2.0");
    (dir, teardown)
}

fn chosen_before(service: &str, named: &[(String, String)]) -> bool {
    named.iter().any(|(name, _)| name == service)
}

/// `service=image` pairs from an environment variable.
fn images_named(var: &str) -> Vec<(String, String)> {
    std::env::var(var)
        .unwrap_or_default()
        .split(',')
        .filter_map(|pair| pair.trim().split_once('='))
        .map(|(service, image)| (service.to_string(), image.to_string()))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a whole hub in Docker; run with KONSTRUKTOR_E2E=1 and --ignored"]
async fn a_hub_of_0_13_is_updated_onto_todays_releases() {
    if std::env::var("KONSTRUKTOR_E2E").as_deref() != Ok("1") {
        eprintln!("skipping: set KONSTRUKTOR_E2E=1 to spawn a hub");
        return;
    }

    let (dir, _teardown) = a_running_hub_of_0_13().await;
    let compose_file = dir.join("docker-compose.yaml");
    let text = std::fs::read_to_string(&compose_file).unwrap();
    let config = read_profile(&dir).unwrap().config;
    let before = running_images(&dir);

    // --- an update that cannot fetch an image changes nothing --------------------------
    let profile_text = std::fs::read_to_string(profile::profile_path(&dir)).unwrap();
    let mut broken = read_profile(&dir).unwrap();
    broken
        .config
        .set_service_image("kraph", "jhnnsrs/kraph:no-such-release");
    profile::write_profile(&dir, &broken).unwrap();
    let refused = update(&dir, true).await;
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

    // What an operator set for this hub has to be in the config the new release runs on.
    konstruktor_core::overrides::set(&dir, "mikro", "django.log_level", "WARNING")
        .expect("the override is kept");

    // --- the update ---------------------------------------------------------------------
    let named = images_named("KONSTRUKTOR_E2E_IMAGES");
    if !named.is_empty() {
        let mut chosen = read_profile(&dir).unwrap();
        for (service, image) in &named {
            eprintln!("{service} is updated onto {image}");
            chosen.config.set_service_image(service, image);
        }
        profile::write_profile(&dir, &chosen).unwrap();
    }
    // --- what the update would do, asked first -----------------------------------------
    // The releases are fetched and asked; nothing of the hub is written or replaced.
    let hub_before = (
        std::fs::read_to_string(&compose_file).unwrap(),
        std::fs::read_to_string(dir.join("configs/rekuest.yaml")).unwrap(),
        running_images(&dir),
    );
    let asked: Vec<String> = ["rekuest", "mikro", "fluss", "kabinet", "kraph"]
        .map(String::from)
        .to_vec();
    let previews = updates::preview(&dir, &asked, &|event| eprintln!("{event:?}"))
        .await
        .expect("the preview runs");
    eprintln!("{previews:#?}");
    assert_eq!(
        (
            std::fs::read_to_string(&compose_file).unwrap(),
            std::fs::read_to_string(dir.join("configs/rekuest.yaml")).unwrap(),
            running_images(&dir),
        ),
        hub_before,
        "asking what an update would do changed the hub"
    );
    assert!(!dir.join(".konstruktor/preview").exists());
    let of = |service: &str| {
        previews
            .iter()
            .find(|said| said.service == service)
            .unwrap()
    };
    assert_eq!(of("rekuest").from.as_deref(), Some("5.2.0"));
    assert!(of("mikro").to.is_some(), "{:?}", of("mikro"));
    if chosen_before("rekuest", &named) {
        // A release that describes itself says what of its config changes — the two lists
        // Rekuest 6 reads — and which migrations it brings.
        let rekuest = of("rekuest");
        assert_eq!(rekuest.refused, None);
        assert!(
            rekuest
                .config_changes
                .iter()
                .any(|key| key.starts_with("rekuest.services")),
            "{:?}",
            rekuest.config_changes
        );
        assert!(!rekuest.migrations.is_empty(), "{:?}", rekuest.notes);
    }

    let report = update(&dir, true).await.expect("the update runs");
    assert_eq!(
        report.migrated.len() as u32,
        migrate::CURRENT_LAYOUT - 2,
        "{:?}",
        report.migrated
    );
    assert!(report.refused.is_empty(), "{:?}", report.refused);
    for service in ["rekuest", "mikro", "fluss", "kabinet", "kraph"] {
        assert!(
            report.updated.iter().any(|s| s == service),
            "{service} was not updated: {:?}",
            report.updated
        );
    }
    assert_eq!(migrate::layout(&dir, &config), migrate::CURRENT_LAYOUT);
    let mikro: serde_norway::Value =
        serde_norway::from_str(&std::fs::read_to_string(dir.join("configs/mikro.yaml")).unwrap())
            .unwrap();
    assert_eq!(
        mikro["django"]["log_level"].as_str(),
        Some("WARNING"),
        "what the operator set did not survive the update"
    );
    let after = running_images(&dir);
    // On the major the files were written for, not on wherever `latest` goes next —
    // unless this run named an image, which somebody choosing one is left on.
    let seeded = konstruktor_core::config::hub::build_hub_config(&Default::default());
    let chosen = |service: &str| named.iter().any(|(name, _)| *name == service);
    // The profile names the channel; what runs is an exact build of it.
    let on_channel = |service: &str, channel: Option<String>| {
        let channel = channel.expect("a seeded image");
        assert!(
            after[service].starts_with(&format!("{channel}@sha256:")),
            "{service} runs {}, not a build of {channel}",
            after[service]
        );
    };
    on_channel("mikro", seeded.mikro.image.clone());
    if !chosen("rekuest") {
        on_channel("rekuest", seeded.rekuest.image.clone());
        assert_eq!(
            read_profile(&dir).unwrap().config.rekuest.image,
            seeded.rekuest.image
        );
    }
    if !chosen("rekuest") && !chosen("rekuest-takt") {
        on_channel("rekuest-takt", seeded.takt_image());
    }

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

    // --- it runs exact builds, and nothing but an update moves them -----------------------
    // The compose file names a digest for every image that has one, and Docker agrees it
    // describes what runs: `up` again makes no container anew.
    // Of the services that stay up: the bucket init container runs once and exits, and
    // every `up` runs it again.
    let run_once = seeded.minio.init_container_host.clone();
    let containers = |dir: &Path| {
        String::from_utf8_lossy(&compose(dir, &["ps", "--format", "{{.Service}}|{{.ID}}"]).stdout)
            .lines()
            .filter_map(|line| line.split_once('|'))
            .filter(|(service, _)| *service != run_once)
            .map(|(_, id)| id.to_string())
            .collect::<std::collections::BTreeSet<String>>()
    };
    let pinned = std::fs::read_to_string(&compose_file).unwrap();
    for service in ["rekuest", "mikro", "fluss", "kabinet", "kraph", "db"] {
        if chosen(service) {
            continue;
        }
        let image = konstruktor_core::lock::read(&dir)
            .pins
            .get(service)
            .and_then(|pin| pin.reference())
            .unwrap_or_else(|| panic!("{service} has no build written down"));
        assert!(image.contains("@sha256:"), "{image}");
        assert!(
            pinned.contains(&format!("image: {image}\n")),
            "the compose file does not name {image}"
        );
    }
    let before_up = containers(&dir);
    let up = compose(&dir, &["up", "-d"]);
    assert!(up.status.success());
    assert_eq!(containers(&dir), before_up, "`up` replaced a container");

    // A channel that moves on this machine moves nothing in the hub: the tag kraph
    // follows is pointed at another image, and `up` still runs the build written down.
    let seeded_kraph = seeded.kraph.image.clone().unwrap();
    let kept = format!("{seeded_kraph}-kept-by-hub-upgrade");
    let docker = |args: &[&str]| {
        konstruktor_core::docker::command()
            .args(args)
            .output()
            .expect("docker runs")
    };
    assert!(docker(&["tag", &seeded_kraph, &kept]).status.success());
    assert!(docker(&["tag", "jhnnsrs/kraph:1.1.0", &seeded_kraph])
        .status
        .success());
    let up = compose(&dir, &["up", "-d"]);
    docker(&["tag", &kept, &seeded_kraph]);
    docker(&["rmi", &kept]);
    assert!(up.status.success());
    assert_eq!(
        containers(&dir),
        before_up,
        "a tag that moved on this machine replaced a container"
    );

    // --- frozen, an update leaves it alone -------------------------------------------------
    let files_before = (
        pinned.clone(),
        std::fs::read_to_string(profile::profile_path(&dir)).unwrap(),
    );
    let held = konstruktor_core::freeze::hold(&dir, &[]).expect("the hub is frozen");
    assert!(held.iter().any(|service| service == "mikro"), "{held:?}");
    assert_eq!(
        (
            std::fs::read_to_string(&compose_file).unwrap(),
            std::fs::read_to_string(profile::profile_path(&dir)).unwrap()
        ),
        files_before,
        "freezing changed a file"
    );
    let mut request = UpdateRequest {
        services: vec!["mikro".into(), "rekuest".into()],
        advances: Vec::new(),
        pull: true,
        backup_into: None,
        health_check: false,
    };
    let report = updates::apply(&dir, &request, &|event| eprintln!("{event:?}"))
        .await
        .expect("an update of a frozen hub runs");
    assert!(report.updated.is_empty(), "{:?}", report.updated);
    assert_eq!(report.refused.len(), 2, "{:?}", report.refused);
    assert_eq!(
        containers(&dir),
        before_up,
        "a frozen container was replaced"
    );

    // --- released, an update moves it again -----------------------------------------------
    konstruktor_core::freeze::release(&dir, &[]).expect("the freeze is lifted");
    request.services = vec!["kraph".into()];
    let report = updates::apply(&dir, &request, &|event| eprintln!("{event:?}"))
        .await
        .expect("the update runs");
    assert_eq!(report.updated, ["kraph"]);
    assert!(report.refused.is_empty(), "{:?}", report.refused);

    // --- back to an earlier build, and forward again -----------------------------------------
    // Kraph is put on the build before the one its channel points at, as a hub that has
    // not been updated for a while is; an update moves it, a rollback puts it back and
    // holds it there, and released it moves on.
    let channel = seeded.kraph.image.clone().unwrap();
    let earlier =
        konstruktor_core::pins::resolve(&[("kraph".into(), "jhnnsrs/kraph:1.1.0".into())])
            .await
            .remove("kraph")
            .and_then(|pin| pin.digest)
            .expect("kraph 1.1.0 is on this machine");
    let newest = konstruktor_core::lock::read(&dir).pins["kraph"]
        .digest
        .clone()
        .unwrap();
    assert_ne!(earlier, newest, "kraph's channel is no further than 1.1.0");
    konstruktor_core::pins::record(
        &dir,
        [(
            "kraph".to_string(),
            konstruktor_core::lock::Pin {
                image: channel.clone(),
                digest: Some(earlier.clone()),
            },
        )]
        .into(),
    )
    .unwrap();
    profile::rewrite(&dir, read_profile(&dir).unwrap().config, &[]).unwrap();
    assert!(compose(&dir, &["up", "-d", "--no-deps", "kraph"])
        .status
        .success());
    let runs = |dir: &Path| running_images(dir)["kraph"].clone();
    assert_eq!(runs(&dir), format!("{channel}@{earlier}"));

    let report = updates::apply(&dir, &request, &|event| eprintln!("{event:?}"))
        .await
        .expect("the update runs");
    assert_eq!(report.updated, ["kraph"]);
    assert_eq!(runs(&dir), format!("{channel}@{newest}"));

    let back = konstruktor_core::rollback::plan(&dir).expect("there is a state to go back to");
    assert_eq!(
        back.changes
            .iter()
            .map(|change| (change.service.as_str(), change.to.clone()))
            .collect::<Vec<_>>(),
        [("kraph", format!("{channel}@{earlier}"))]
    );
    konstruktor_core::rollback::run(&dir, &back, &|line| eprintln!("  {}", line.line))
        .await
        .expect("the rollback runs");
    assert_eq!(runs(&dir), format!("{channel}@{earlier}"));
    assert_eq!(
        read_profile(&dir).unwrap().config.kraph.image,
        Some(channel.clone()),
        "the profile still follows its channel"
    );
    // Held there: an update does not undo the rollback.
    let report = updates::apply(&dir, &request, &|event| eprintln!("{event:?}"))
        .await
        .expect("an update of a rolled-back hub runs");
    assert!(report.updated.is_empty(), "{:?}", report.updated);
    assert_eq!(runs(&dir), format!("{channel}@{earlier}"));
    // Released, it moves on.
    konstruktor_core::freeze::release(&dir, &["kraph".to_string()]).unwrap();
    let report = updates::apply(&dir, &request, &|event| eprintln!("{event:?}"))
        .await
        .expect("the update runs");
    assert_eq!(report.updated, ["kraph"]);
    assert_eq!(runs(&dir), format!("{channel}@{newest}"));
    healthy(&dir).await;
}

/// A release whose own upgrade fails stops the update before anything is replaced: the
/// hub is on the builds it ran, on the files it had, and answers.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a whole hub in Docker; run with KONSTRUKTOR_E2E=1 and --ignored"]
async fn an_upgrade_that_fails_leaves_the_hub_as_it_was() {
    let failing = images_named("KONSTRUKTOR_E2E_FAILING_UPGRADE");
    if std::env::var("KONSTRUKTOR_E2E").as_deref() != Ok("1") || failing.is_empty() {
        eprintln!("skipping: set KONSTRUKTOR_E2E=1 and KONSTRUKTOR_E2E_FAILING_UPGRADE");
        return;
    }

    let (dir, _teardown) = a_running_hub_of_0_13().await;
    let config = read_profile(&dir).unwrap().config;
    let mut chosen = read_profile(&dir).unwrap();
    for (service, image) in &failing {
        chosen.config.set_service_image(service, image);
    }
    profile::write_profile(&dir, &chosen).unwrap();
    let files = |dir: &Path| {
        (
            std::fs::read_to_string(dir.join("docker-compose.yaml")).unwrap(),
            std::fs::read_to_string(dir.join("configs/rekuest.yaml")).unwrap(),
        )
    };
    let files_before = files(&dir);
    let builds_before = running_images(&dir);

    let stopped = update(&dir, true).await;
    assert!(
        matches!(&stopped, Err(updates::UpdateError::Migration(why)) if why.contains("could not upgrade itself")),
        "{:?}",
        stopped.map(|report| report.updated)
    );
    assert_eq!(files(&dir), files_before, "the files were not put back");
    assert_eq!(migrate::layout(&dir, &config), 2);
    assert!(konstruktor_core::lock::read(&dir).pins.is_empty());
    assert_eq!(
        running_images(&dir),
        builds_before,
        "a container was replaced, or one that was stopped did not come back"
    );
    healthy(&dir).await;
}
