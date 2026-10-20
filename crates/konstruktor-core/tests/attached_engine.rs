//! A hub created unconnected, connected to a coordination server, and given a plugin
//! engine that is then updated in place — end to end, against a real Lok.
//!
//! The coordination server is a self-contained hub of its own: its Lok is the one a
//! connected hub and an attached app are accepted at. Where a person would accept them in
//! a browser, the test does, through Lok's management API as the user the coordination
//! server was seeded with. Nothing else stands in for anything: the engine is the real
//! release from ghcr.io, and it has to log in and register with the hub's own Rekuest.
//!
//! Everything has to reach the coordination server at one address: this process, the
//! hub's services fetching its keys, the engine logging in. Docker's bridge gateway is one
//! (`KONSTRUKTOR_E2E_HOST_ADDRESS` names another).
//!
//! It pulls and starts two whole stacks, so it is opt-in twice over:
//!
//! ```sh
//! KONSTRUKTOR_E2E=1 cargo test -p konstruktor-core --test attached_engine -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use konstruktor_core::apps::{self, AttachAnswers, UpdateAnswers};
use konstruktor_core::catalog::ServiceId;
use konstruktor_core::connect::manifest::AdvertisedHost;
use konstruktor_core::create::{
    connect, create_hub, ConnectAnswers, CreateEvent, HubAnswers, MeshKeyRequest,
};
use konstruktor_core::profile::read_profile;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// The engine's release it is attached at: one that runs, but cannot verify a hub's signed
/// challenges, so it never gets further than its login. The update has to rescue it.
const ENGINE_BEFORE: &str = "2.2.0";

/// The user the coordination server is seeded with, and the organization they are in.
const USER: &str = "ada";
const PASSWORD: &str = "e2e-password";
const ORGANIZATION: &str = "demo";

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

/// An address of this machine that its containers reach it at, too.
fn host_address() -> String {
    if let Ok(address) = std::env::var("KONSTRUKTOR_E2E_HOST_ADDRESS") {
        return address;
    }
    let output = konstruktor_core::docker::command()
        .args([
            "network",
            "inspect",
            "bridge",
            "--format",
            "{{(index .IPAM.Config 0).Gateway}}",
        ])
        .output()
        .expect("docker runs");
    let address = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(
        !address.is_empty(),
        "docker's bridge has no gateway address"
    );
    address
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

fn logs(dir: &Path, service: &str) -> String {
    let output = compose(dir, &["logs", "--no-log-prefix", service]);
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn hub_answers(
    dir: &Path,
    coord_server: &str,
    services: Vec<ServiceId>,
    address: &str,
) -> HubAnswers {
    let name = dir.file_name().unwrap().to_string_lossy().to_string();
    serde_json::from_value(json!({
        "dir": dir.to_string_lossy(),
        "name": name,
        "coord_server": coord_server,
        "identifier": name,
        "rekuest_server": "local",
        "services": services,
        "http_port": free_port(),
        "https_port": free_port(),
        "hosts": [
            {"host": address, "kind": konstruktor_core::hosts::classify_host(address)},
        ],
        "mesh_mode": "none",
        "start": true,
        "seed": {"organization": ORGANIZATION, "user": USER, "user_password": PASSWORD},
    }))
    .expect("the answers")
}

/// Lok's management API, as the person who would otherwise click "accept": a session of
/// the seeded user, its CSRF token sent back the way the SPA sends it.
struct Lok {
    base: String,
    client: reqwest::Client,
    cookies: Vec<(String, String)>,
}

impl Lok {
    async fn login(base: &str, user: &str, password: &str) -> Self {
        let client = reqwest::Client::builder()
            // The session cookie comes with the redirect, which is not to be followed.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("a client");
        let mut lok = Self {
            base: base.trim_end_matches('/').to_string(),
            client,
            cookies: Vec::new(),
        };
        let login = format!("{}/lok/accounts/login/", lok.base);
        let page = lok.client.get(&login).send().await.expect("the login page");
        lok.keep(&page);
        let form = [
            ("csrfmiddlewaretoken", lok.cookie("csrftoken")),
            ("login", user.to_string()),
            ("password", password.to_string()),
        ];
        let answered = lok
            .client
            .post(&login)
            .header("Cookie", lok.cookie_header())
            .header("Referer", &login)
            .form(&form)
            .send()
            .await
            .expect("the login answers");
        lok.keep(&answered);
        assert!(
            !lok.cookie("sessionid").is_empty(),
            "{user} could not log in to {base} ({})",
            answered.status()
        );
        lok
    }

    fn keep(&mut self, response: &reqwest::Response) {
        for header in response.headers().get_all("set-cookie") {
            let Some((name, value)) = header
                .to_str()
                .ok()
                .and_then(|v| v.split(';').next())
                .and_then(|pair| pair.split_once('='))
            else {
                continue;
            };
            self.cookies.retain(|(kept, _)| kept != name);
            self.cookies.push((name.to_string(), value.to_string()));
        }
    }

    fn cookie(&self, name: &str) -> String {
        self.cookies
            .iter()
            .find(|(kept, _)| kept == name)
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    }

    fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    async fn graphql(&self, query: &str, variables: Value) -> Value {
        let answer: Value = self
            .client
            .post(format!("{}/lok/managementgraphql/", self.base))
            .header("Cookie", self.cookie_header())
            .header("X-CSRFToken", self.cookie("csrftoken"))
            .header("Referer", format!("{}/", self.base))
            .json(&json!({"query": query, "variables": variables}))
            .send()
            .await
            .expect("lok answers")
            .json()
            .await
            .expect("lok answers JSON");
        assert!(answer["errors"].is_null(), "{query}: {answer}");
        answer["data"].clone()
    }

    /// Accepts the hub that displayed `code` into the organization `slug`.
    async fn accept_hub(&self, code: &str, slug: &str) {
        let pending = self
            .graphql(
                "query($c: String!) { hubDeviceCodeByCode(code: $c) { id } }",
                json!({"c": code}),
            )
            .await;
        let organizations = self
            .graphql("{ organizations { id slug } }", json!({}))
            .await;
        let organization = organizations["organizations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["slug"] == slug)
            .unwrap_or_else(|| panic!("no organization {slug}: {organizations}"))["id"]
            .clone();
        self.graphql(
            "mutation($i: AcceptHubDeviceCodeInput!) { acceptHubDeviceCode(input: $i) { id } }",
            json!({"i": {
                "deviceCode": pending["hubDeviceCodeByCode"]["id"],
                "code": code,
                "organization": organization,
                "allowIonscale": false,
            }}),
        )
        .await;
    }

    /// Accepts the app that displayed `code` into the hub named `hub`.
    async fn accept_app(&self, code: &str, hub: &str) {
        let pending = self
            .graphql(
                "query($c: String!) { deviceCodeByCode(deviceCode: $c) { id } }",
                json!({"c": code}),
            )
            .await;
        let hubs = self.graphql("{ hubs { id name } }", json!({})).await;
        let hub_id = hubs["hubs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["name"] == hub)
            .unwrap_or_else(|| panic!("no hub {hub}: {hubs}"))["id"]
            .clone();
        self.graphql(
            "mutation($i: AcceptDeviceCodeInput!) { acceptDeviceCode(input: $i) { id } }",
            json!({"i": {
                "deviceCode": pending["deviceCodeByCode"]["id"],
                "code": code,
                "hub": hub_id,
                "allowIonscale": false,
            }}),
        )
        .await;
    }
}

/// The device code an acceptance displays, as a channel the approver waits on.
fn codes() -> (impl Fn(CreateEvent) + Sync, mpsc::UnboundedReceiver<String>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let on = move |event: CreateEvent| {
        if let CreateEvent::Staged { user_code, .. } = &event {
            let _ = sender.send(user_code.clone());
        }
        eprintln!("{event:?}");
    };
    (on, receiver)
}

/// What the engine in `dir` said in its starts, one per start: each begins with its banner.
fn starts(dir: &Path) -> Vec<String> {
    logs(dir, apps::SERVICE)
        .split("Arkitekt v")
        .skip(1)
        .map(str::to_string)
        .collect()
}

/// What the engine said in the start after the first `seen`, once it has registered with
/// Rekuest in it — or `None` when it does not within three minutes.
async fn registered(dir: &Path, seen: usize) -> Option<String> {
    for _ in 0..60 {
        if let Some(start) = starts(dir).get(seen) {
            if start.contains("Registered") {
                return Some(start.clone());
            }
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    None
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns two hubs and a plugin engine in Docker; run with KONSTRUKTOR_E2E=1 and --ignored"]
async fn a_hub_is_connected_and_its_engine_updated_in_place() {
    if std::env::var("KONSTRUKTOR_E2E").as_deref() != Ok("1") {
        eprintln!("skipping: set KONSTRUKTOR_E2E=1 to spawn the hubs");
        return;
    }
    if !konstruktor_core::docker::probe().await.is_ready() {
        eprintln!("skipping: no container engine ready here");
        return;
    }

    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("attached-engine-e2e");
    let (coord, lab, engine) = (
        root.join("kx-coord"),
        root.join("kx-lab"),
        root.join("kx-engine"),
    );
    // A run that crashed hard (no drop) may have left its stacks behind.
    for dir in [&engine, &lab, &coord] {
        if dir.join("docker-compose.yaml").exists() {
            compose(dir, &["down", "--volumes", "--remove-orphans"]);
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a scratch folder");
    // This run's registry is its own: nothing here is listed among the real deployments.
    std::env::set_var(konstruktor_core::registry::DATA_DIR_ENV, root.join("data"));
    let _teardown = Teardown(vec![coord.clone(), lab.clone(), engine.clone()]);
    let address = host_address();
    let images = konstruktor_core::updates::newest_seeds().await.0;

    // --- the coordination server: a hub that runs its own Lok -----------------------------
    let mut answers = hub_answers(&coord, "local", vec![ServiceId::Rekuest], &address);
    answers.default_images = images.clone();
    create_hub(&answers, &|event| eprintln!("{event:?}"))
        .await
        .expect("the coordination server is created");
    let coord_config = read_profile(&coord).expect("its profile").config;
    konstruktor_core::ready::wait(&coord_config, Duration::from_secs(600), &|_| {})
        .await
        .expect("it publishes a port");
    let server = format!("http://{address}:{}", answers.http_port);

    // --- a hub, created connected nowhere ----------------------------------------------------
    let mut answers = hub_answers(
        &lab,
        "",
        vec![ServiceId::Rekuest, ServiceId::Kabinet],
        &address,
    );
    answers.default_images = images;
    create_hub(&answers, &|event| eprintln!("{event:?}"))
        .await
        .expect("the hub is created");
    let rekuest_config = std::fs::read_to_string(lab.join("configs/rekuest.yaml")).unwrap();
    assert!(
        !rekuest_config.contains("allowed_organizations"),
        "a hub connected nowhere names no organization:\n{rekuest_config}"
    );

    // --- connected: accepted into the seeded organization --------------------------------
    let lok = Lok::login(&server, USER, PASSWORD).await;
    let cancel = CancellationToken::new();
    let (on, mut displayed) = codes();
    let asked = ConnectAnswers {
        dir: lab.clone(),
        coord_server: server.clone(),
        identifier: "kx-lab".into(),
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
        lok.accept_hub(&code, ORGANIZATION).await;
    };
    let (connected, ()) = tokio::join!(connecting, accepting);
    let connected = connected.expect("the hub is connected");
    // Created without a mesh, it is connected without one.
    assert!(!connected.mesh_requested);
    // As `hub connect` and the desktop app do: the running hub is started again, which
    // renders its services' configs for the grant.
    konstruktor_core::start::start(&lab, &|line| eprintln!("{}", line.line))
        .await
        .expect("the hub starts again on its new configs");
    let organization = connected
        .organization
        .expect("the organization that accepted it");
    let rekuest_config = std::fs::read_to_string(lab.join("configs/rekuest.yaml")).unwrap();
    assert!(
        rekuest_config.contains(&format!("- '{organization}'")),
        "its services take that organization's tokens, and only those:\n{rekuest_config}"
    );

    // --- a plugin engine beside it, at a release that cannot reach its services ----------
    let (on, mut displayed) = codes();
    let asked = AttachAnswers {
        dir: engine.to_string_lossy().to_string(),
        name: "kx-engine".into(),
        release: format!("{}:{ENGINE_BEFORE}", apps::PLUGIN_ENGINE_RELEASE),
        hub: Some(lab.to_string_lossy().to_string()),
        allow: vec!["engine".into()],
        start: true,
        ..Default::default()
    };
    let attaching = apps::attach_app(&asked, &cancel, &on);
    let accepting = async {
        let code = displayed.recv().await.expect("the engine displays a code");
        lok.accept_app(&code, "kx-lab").await;
    };
    let (attached, ()) = tokio::join!(attaching, accepting);
    let attached = attached.expect("the engine is attached");
    assert_eq!(
        attached.organization.as_deref(),
        Some(organization.as_str())
    );
    let before = apps::read_app_record(&engine).expect("its record");

    // --- updated in place, accepted again -------------------------------------------------
    let plan = apps::plan_update(&engine, None).await.expect("a plan");
    assert!(plan.is_newer(), "a release after {ENGINE_BEFORE} is out");
    assert_eq!(plan.kept, ["engine"], "the container engine stays granted");
    let (on, mut displayed) = codes();
    let asked = UpdateAnswers {
        dir: engine.to_string_lossy().to_string(),
        ..Default::default()
    };
    let updating = apps::update_app(&plan, &asked, &cancel, &on);
    let accepting = async {
        let code = displayed.recv().await.expect("the update displays a code");
        lok.accept_app(&code, "kx-lab").await;
    };
    let (updated, ()) = tokio::join!(updating, accepting);
    let updated = updated.expect("the engine is updated");
    assert_eq!(updated.from, ENGINE_BEFORE);

    let after = apps::read_app_record(&engine).expect("its record");
    assert_eq!(after.app.version, updated.to);
    assert_eq!(
        after.app.instance_id, before.app.instance_id,
        "the same installation"
    );
    assert_ne!(
        after.fakts.client_id, before.fakts.client_id,
        "a client of its own"
    );
    let running = compose(&engine, &["ps", "--format", "{{.Image}}", apps::SERVICE]);
    assert_eq!(
        String::from_utf8_lossy(&running.stdout).trim(),
        after.app.image,
        "the container was recreated on the new release"
    );

    // --- and the new release is in: logged in, its challenges verified, registered -------
    let first = registered(&engine, 0).await.unwrap_or_else(|| {
        panic!(
            "the engine did not register with the hub's Rekuest:\n{}",
            logs(&engine, apps::SERVICE)
        )
    });
    assert!(
        first.contains("with a token"),
        "its first start logs in: {first}"
    );

    // Started again, it keeps the session it has: no second login with a spent token.
    let seen = starts(&engine).len();
    compose(&engine, &["restart", apps::SERVICE]);
    let again = registered(&engine, seen).await.unwrap_or_else(|| {
        panic!(
            "the engine did not come back after a restart:\n{}",
            logs(&engine, apps::SERVICE)
        )
    });
    assert!(
        !again.contains("with a token"),
        "it logged in again: {again}"
    );
}
