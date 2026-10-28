//! A coordination server of one's own, and a hub connected to it — end to end, in Docker.
//!
//! `coord create` in plain HTTP, the way a lab runs one: it is waited on, its well-known has
//! to name the address it was made with, and a hub is then created unconnected and connected
//! to it, accepted into its organization by its administrator through Lok's management API —
//! where a person would click "accept" in kontrol. The hub's services have to reach the
//! coordination server's keys from inside their containers, which is why everything is
//! reached at an address of this machine and never at `localhost` (Docker's bridge gateway,
//! or `KONSTRUKTOR_E2E_HOST_ADDRESS`).
//!
//! It pulls and starts two stacks, so it is opt-in twice over:
//!
//! ```sh
//! KONSTRUKTOR_E2E=1 cargo test -p konstruktor-core --test coord_lab -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use konstruktor_core::catalog::ServiceId;
use konstruktor_core::config::coord::SignupPolicy;
use konstruktor_core::connect::manifest::AdvertisedHost;
use konstruktor_core::coord::{create_coord, CoordAnswers};
use konstruktor_core::create::{connect, create_hub, ConnectAnswers, HubAnswers, MeshKeyRequest};
use serde_json::json;
use tokio_util::sync::CancellationToken;

mod support;
use support::lok::{codes, free_port, host_address, Lok};

const ADMIN: &str = "admin";
const PASSWORD: &str = "e2e-admin-password";

fn compose(dir: &Path, args: &[&str]) -> std::process::Output {
    konstruktor_core::docker::command()
        .args(["compose"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("docker runs")
}

/// Takes every stack down — volumes included — however the test ends.
struct Teardown(Vec<PathBuf>);

impl Drop for Teardown {
    fn drop(&mut self) {
        for dir in self.0.iter().rev() {
            if dir.join("docker-compose.yaml").exists() {
                eprintln!("tearing {} down…", dir.display());
                compose(dir, &["down", "--volumes", "--remove-orphans"]);
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a coordination server and a hub in Docker; run with KONSTRUKTOR_E2E=1 and --ignored"]
async fn a_hub_is_accepted_by_a_coordination_server_of_ones_own() {
    if std::env::var("KONSTRUKTOR_E2E").as_deref() != Ok("1") {
        eprintln!("skipping: set KONSTRUKTOR_E2E=1 to spawn the stacks");
        return;
    }
    if !konstruktor_core::docker::probe().await.is_ready() {
        eprintln!("skipping: no container engine ready here");
        return;
    }

    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("coord-lab-e2e");
    let (coord, lab) = (root.join("kx-coord"), root.join("kx-lab"));
    for dir in [&lab, &coord] {
        if dir.join("docker-compose.yaml").exists() {
            compose(dir, &["down", "--volumes", "--remove-orphans"]);
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a scratch folder");
    std::env::set_var(konstruktor_core::registry::DATA_DIR_ENV, root.join("data"));
    let _teardown = Teardown(vec![coord.clone(), lab.clone()]);
    let address = host_address();

    // --- the coordination server ----------------------------------------------------------
    let answers = CoordAnswers {
        dir: coord.to_string_lossy().to_string(),
        name: "kx-coord".into(),
        domain: address.clone(),
        tls: None,
        http_port: free_port(),
        https_port: free_port(),
        mesh_domain: None,
        mesh_dns_suffix: None,
        admin: ADMIN.into(),
        admin_password: Some(PASSWORD.into()),
        admin_email: None,
        organization: Some("KX Lab".into()),
        email: None,
        signup: SignupPolicy::Closed,
        public_ips: Vec::new(),
        skip_dns_check: false,
        images: Default::default(),
        default_images: Default::default(),
        storage: Default::default(),
        start: true,
    };
    let created = create_coord(&answers, &|event| eprintln!("{event:?}"))
        .await
        .expect("the coordination server is created");
    let unready: Vec<_> = created.endpoints.iter().filter(|e| !e.ready).collect();
    assert!(unready.is_empty(), "everything answers: {unready:?}");
    assert_eq!(
        konstruktor_core::profile::holds_a_deployment(&coord),
        Some(konstruktor_core::profile::DeploymentKind::Coord)
    );

    let server = konstruktor_core::coord::origin(&answers);
    let well_known: serde_json::Value = reqwest::get(format!("{server}/.well-known/fakts"))
        .await
        .expect("the well-known answers")
        .json()
        .await
        .expect("as JSON");
    assert_eq!(well_known["issuer"], server.as_str());
    assert_eq!(
        well_known["hub_configure"],
        format!("{server}/hubconfigure/{{code}}").as_str()
    );
    let organization = created
        .config
        .running_lok()
        .expect("it runs Lok")
        .organization
        .identifier
        .clone();

    // --- a hub, created connected nowhere, then connected to it --------------------------
    let name = lab.file_name().unwrap().to_string_lossy().to_string();
    let hub: HubAnswers = serde_json::from_value(json!({
        "dir": lab.to_string_lossy(),
        "name": name,
        "coord_server": "",
        "identifier": name,
        "rekuest_server": "local",
        "services": [ServiceId::Rekuest],
        "http_port": free_port(),
        "https_port": free_port(),
        "hosts": [{"host": address, "kind": konstruktor_core::hosts::classify_host(&address)}],
        "mesh_mode": "none",
        "start": true,
    }))
    .expect("the hub's answers");
    let mut hub = hub;
    hub.default_images = konstruktor_core::updates::newest_seeds().await.0;
    create_hub(&hub, &|event| eprintln!("{event:?}"))
        .await
        .expect("the hub is created");

    let lok = Lok::login(&server, ADMIN, PASSWORD).await;
    let cancel = CancellationToken::new();
    let (on, mut displayed) = codes();
    let asked = ConnectAnswers {
        dir: lab.clone(),
        coord_server: server.clone(),
        identifier: name.clone(),
        description: None,
        hosts: vec![AdvertisedHost {
            host: address.clone(),
            kind: konstruktor_core::hosts::classify_host(&address),
        }],
        reachable_hosts: Vec::new(),
        mesh_key: MeshKeyRequest::Auto,
        services: None,
        described: Default::default(),
    };
    let connecting = connect(&asked, &cancel, &on);
    let accepting = async {
        let code = displayed.recv().await.expect("the hub displays a code");
        lok.accept_hub(&code, &organization).await;
    };
    let (connected, ()) = tokio::join!(connecting, accepting);
    let connected = connected.expect("the hub is connected");
    konstruktor_core::start::start(&lab, &|line| eprintln!("{}", line.line))
        .await
        .expect("the hub starts again on its new configs");

    // Its services take this server's tokens: its issuer, its organization.
    let rekuest = std::fs::read_to_string(lab.join("configs/rekuest.yaml")).unwrap();
    assert!(
        rekuest.contains(&format!("iss: {server}")),
        "the hub's services trust the coordination server's issuer:\n{rekuest}"
    );
    let accepted = connected
        .organization
        .expect("the organization that accepted it");
    assert!(
        rekuest.contains(&format!("{server}#{accepted}")),
        "and that organization's tokens:\n{rekuest}"
    );
    // And reach its keys from where they verify tokens: inside their container.
    let fetched = compose(
        &lab,
        &[
            "exec",
            "-T",
            "rekuest",
            "python",
            "-c",
            &format!(
                "import urllib.request; print(urllib.request.urlopen('{server}/lok/o/jwks/', timeout=10).status)"
            ),
        ],
    );
    assert_eq!(
        String::from_utf8_lossy(&fetched.stdout).trim(),
        "200",
        "the keys from inside a service: {}",
        String::from_utf8_lossy(&fetched.stderr)
    );

    // Asked the way a client elsewhere asks, only the signup policy may be off: Lok 8 does
    // not read `account.allow_signup` yet.
    let checked = konstruktor_core::ready::check(&created.config)
        .await
        .expect("it publishes a port");
    let failing: Vec<_> = checked
        .iter()
        .filter(|e| !e.ready && e.name != "signup")
        .collect();
    assert!(failing.is_empty(), "{failing:?}");

    // Kept like a hub: backed up with its accounts.
    let backups = root.join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    let report = konstruktor_core::backup::run(
        &konstruktor_core::backup::BackupRequest {
            dir: coord.clone(),
            target: backups,
        },
        &|event| eprintln!("{event:?}"),
    )
    .await
    .expect("the coordination server is backed up");
    eprintln!("backed up: {report:?}");
}
