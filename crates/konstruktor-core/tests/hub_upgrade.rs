//! Moves a running hub's service from one build to another, and checks what that takes.
//!
//! `hub_health` proves a hub created today works. This proves it can be moved on: a hub
//! is created on one build of a service — the whole way `hub create` goes, its own
//! coordination server included, so nobody has to accept anything — and one
//! `updates::apply` carries that service to another build. What an update owes a service:
//!
//! - the new release is asked what it would do to the database, and can say (its `plan`
//!   job), before and after;
//! - its database is prepared for the new build exactly once, before its container is
//!   replaced (its `migrate` job), and written down as prepared for that build;
//! - the service answers afterwards, on the new build;
//! - stopping the hub and starting it again prepares nothing: a start only prepares for a
//!   build the database has not been prepared for.
//!
//! Opt-in twice over, like `hub_health`: `#[ignore]`, and `KONSTRUKTOR_E2E=1`.
//!
//! ```sh
//! KONSTRUKTOR_E2E=1 \
//! KONSTRUKTOR_E2E_IMAGES=lok=lok:named-db,example=example:named-db \
//! KONSTRUKTOR_E2E_UPGRADE_TO=example=example:named-db-b \
//!   cargo test -p konstruktor-core --test hub_upgrade -- --ignored --nocapture
//! ```
//!
//! `KONSTRUKTOR_E2E_IMAGES` (`service=image` pairs, as in `hub_health`) names the builds
//! the hub is created on. `KONSTRUKTOR_E2E_UPGRADE_TO` names, the same way, the one
//! service that is moved and the build it is moved to; the hub runs that service — beside
//! Rekuest, when the first list names one — and nothing else. Both builds have to be on
//! this machine or fetchable: neither is fetched by the update itself, so one built here
//! is run as it is.
//!
//! `KONSTRUKTOR_E2E_READY_SECS` (default 600) is how long the hub gets to answer.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use konstruktor_core::catalog::ServiceId;
use konstruktor_core::config::hub::HubConfig;
use konstruktor_core::create::{create_hub, HubAnswers};
use konstruktor_core::profile::read_profile;
use konstruktor_core::updates::{self, Advance, UpdateEvent, UpdateRequest};
use konstruktor_core::{contract, lock, ready};
use tokio_util::sync::CancellationToken;

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

fn ready_timeout() -> Duration {
    std::env::var("KONSTRUKTOR_E2E_READY_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(600))
}

/// `service=image` pairs, separated by commas.
fn pairs(var: &str) -> BTreeMap<String, String> {
    std::env::var(var)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (service, image) = pair
                .split_once('=')
                .unwrap_or_else(|| panic!("{var} is service=image pairs, and `{pair}` is not one"));
            (service.trim().to_string(), image.trim().to_string())
        })
        .collect()
}

/// Takes the hub down — volumes included — however the test ends, a failed assertion too.
struct Teardown(PathBuf);

impl Drop for Teardown {
    fn drop(&mut self) {
        eprintln!("tearing the hub down…");
        compose(&self.0, &["down", "--volumes", "--remove-orphans"]);
    }
}

fn logs(dir: &Path, service: &str) -> String {
    let out = compose(dir, &["logs", "--no-color", "--tail", "80", service]);
    format!(
        "----- logs: {service} -----\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Waits until every endpoint of the hub answers, failing with the service's log.
async fn ready(dir: &Path, config: &HubConfig, service: &str, when: &str) {
    let endpoints = ready::wait(config, ready_timeout(), &|_| {})
        .await
        .expect("the hub publishes a port");
    let unready: Vec<String> = endpoints
        .iter()
        .filter(|e| !e.ready)
        .map(|e| format!("{} ({}) → {:?} {:?}", e.name, e.url, e.status, e.detail))
        .collect();
    assert!(
        unready.is_empty(),
        "{when}, not everything answered:\n{}\n\n{}",
        unready.join("\n"),
        logs(dir, service)
    );
}

/// The image the running container of `service` was made from, as the engine names it.
fn running_image(dir: &Path, service: &str) -> String {
    let out = compose(dir, &["ps", "--format", "{{.Image}}", service]);
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The id `image` resolves to on this machine.
async fn id_of(image: &str) -> String {
    konstruktor_core::docker::image_states(&[(String::new(), image.to_string())])
        .await
        .expect("the engine answers")
        .into_iter()
        .next()
        .and_then(|state| state.image_id)
        .unwrap_or_else(|| panic!("`{image}` is not on this machine"))
}

/// What the release `service` is on would do to its database, by its own `plan` job.
async fn planned(dir: &Path, service: &str) -> updates::ServicePreview {
    let previews = updates::preview(dir, &[service.to_string()], &[], &|_| {})
        .await
        .expect("the update can be previewed");
    let preview = previews
        .into_iter()
        .find(|preview| preview.service == service)
        .expect("the service is previewed");
    assert!(
        preview.refused.is_none(),
        "the release would be refused: {preview:?}"
    );
    // A plan that did not run says so in the notes; one that ran lists what is pending.
    assert!(
        !preview
            .notes
            .iter()
            .any(|note| note.contains("migrations") || note.contains("could not")),
        "the release's plan did not run: {:?}",
        preview.notes
    );
    preview
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a whole hub in Docker; run with KONSTRUKTOR_E2E=1 and --ignored"]
async fn a_service_is_moved_to_another_build_and_prepared_for_it_once() {
    if std::env::var("KONSTRUKTOR_E2E").as_deref() != Ok("1") {
        eprintln!("skipping: set KONSTRUKTOR_E2E=1 to spawn a hub");
        return;
    }
    let images = pairs("KONSTRUKTOR_E2E_IMAGES");
    let moves = pairs("KONSTRUKTOR_E2E_UPGRADE_TO");
    let Some((service, to)) = moves.iter().next() else {
        eprintln!("skipping: KONSTRUKTOR_E2E_UPGRADE_TO names no build to move a service to");
        return;
    };
    let from = images.get(service).unwrap_or_else(|| {
        panic!("KONSTRUKTOR_E2E_IMAGES has to name the build `{service}` starts on")
    });
    assert_ne!(from, to, "the two builds are named alike");
    let (before, after) = (id_of(from).await, id_of(to).await);
    assert_ne!(
        before, after,
        "`{from}` and `{to}` are one build: there is nothing to move between"
    );

    // --- a hub, created the way `hub create` creates one --------------------------------
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hub-upgrade-e2e");
    let dir = root.join("kx-upgrade-e2e");
    // A run that crashed hard (no drop) may have left its stack behind.
    if dir.exists() {
        compose(&dir, &["down", "--volumes", "--remove-orphans"]);
    }
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a scratch folder");
    // The registry of this run is its own: nothing here is listed among the real hubs.
    std::env::set_var(konstruktor_core::registry::DATA_DIR_ENV, root.join("data"));

    let id = ServiceId::parse(service).expect("a service's name");
    let mut services = vec![id];
    let rekuest = images.contains_key("rekuest") && id != ServiceId::Rekuest;
    if rekuest {
        services.insert(0, ServiceId::Rekuest);
    }
    let answers: HubAnswers = serde_json::from_value(serde_json::json!({
        "dir": dir.to_string_lossy(),
        "name": "kx-upgrade-e2e",
        "coord_server": "local",
        "identifier": "kx-upgrade-e2e",
        "rekuest_server": if rekuest || id == ServiceId::Rekuest { "local" } else { "none" },
        "services": services,
        "http_port": free_port(),
        "hosts": [{"host": "localhost", "kind": "loopback"}],
        "mesh_mode": "none",
        "images": {service: from},
        "default_images": images,
    }))
    .expect("the answers");

    let _teardown = Teardown(dir.clone());
    create_hub(&answers, &CancellationToken::new(), &|event| {
        eprintln!("{event:?}")
    })
    .await
    .expect("the hub is created and started");
    let config = read_profile(&dir).expect("the profile").config;
    ready(&dir, &config, service, "After it was created").await;

    assert_eq!(running_image(&dir, service), *from);
    assert_eq!(
        lock::read(&dir).prepared.get(service),
        Some(&before),
        "its database was prepared for the build it was created on"
    );
    // Its release can say what it would do to the database: nothing more, on the build
    // the database was just prepared for.
    let nothing_pending = planned(&dir, service).await;
    assert!(nothing_pending.migrations.is_empty(), "{nothing_pending:?}");

    // --- one update, to the other build --------------------------------------------------
    let narrated: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let report = updates::apply(
        &dir,
        &UpdateRequest {
            services: Vec::new(),
            advances: vec![Advance {
                service: service.clone(),
                from: from.clone(),
                to: to.clone(),
            }],
            // Both builds are on this machine already, and one may exist nowhere else.
            pull: false,
            backup_into: None,
            health_check: true,
        },
        &|event| {
            eprintln!("{event:?}");
            if let UpdateEvent::Step { title } = event {
                narrated.lock().unwrap().push(title);
            }
        },
    )
    .await
    .unwrap_or_else(|error| panic!("the update failed: {error}\n\n{}", logs(&dir, service)));

    assert!(report.refused.is_empty(), "{:?}", report.refused);
    assert_eq!(report.updated, std::slice::from_ref(service));
    let unhealthy: Vec<String> = report
        .health
        .expect("a health check was asked for")
        .into_iter()
        .filter(|health| !health.healthy)
        .map(|health| format!("{}: {}", health.service, health.detail))
        .collect();
    assert!(
        unhealthy.is_empty(),
        "{unhealthy:?}\n\n{}",
        logs(&dir, service)
    );

    // Its database was prepared for the new build — once, as a step of the update.
    let migrating = format!("Migrating {service}'s database");
    let steps = narrated.lock().unwrap().clone();
    assert_eq!(
        steps.iter().filter(|step| **step == migrating).count(),
        1,
        "{steps:?}"
    );
    assert_eq!(
        lock::read(&dir).prepared.get(service),
        Some(&after),
        "it is written down as prepared for the build it runs now"
    );

    // --- afterwards: on the new build, answering, with nothing left to do ----------------
    let config = read_profile(&dir).expect("the profile").config;
    assert_eq!(
        config.service(id).image.as_deref(),
        Some(to.as_str()),
        "the profile names the build it was moved to"
    );
    assert_eq!(running_image(&dir, service), *to);
    ready(&dir, &config, service, "After the update").await;
    let settled = planned(&dir, service).await;
    assert!(settled.migrations.is_empty(), "{settled:?}");
    assert!(
        contract::unprepared(&dir, &config).await.is_empty(),
        "every database is prepared for the build its service runs"
    );

    // --- a plain restart prepares nothing ---------------------------------------------------
    let stopped = compose(&dir, &["stop"]);
    assert!(stopped.status.success(), "the hub could not be stopped");
    let said: Mutex<Vec<String>> = Mutex::new(Vec::new());
    konstruktor_core::start::start(&dir, &|line| {
        eprintln!("  {}", line.line);
        said.lock().unwrap().push(line.line);
    })
    .await
    .expect("the hub starts again");
    let said = said.lock().unwrap().clone();
    assert!(
        !said.iter().any(|line| line.starts_with("Preparing ")),
        "a restart prepared a database again: {said:?}"
    );
    assert_eq!(lock::read(&dir).prepared.get(service), Some(&after));
    ready(&dir, &config, service, "After a restart").await;
    assert_eq!(running_image(&dir, service), *to);
}
