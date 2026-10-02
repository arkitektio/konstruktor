use konstruktor_core::catalog::{ServiceId, SERVICE_IDS};
use konstruktor_core::config::hub::{build_hub_config, HubConfig, HubConfigOptions};
use konstruktor_core::config::mesh::MeshOptions;
use konstruktor_core::generate::{generate_hub_files, IssuedIdentity};
use serde_norway::Value;

/// Lovekit is the one service that brings a container of somebody else's with it: a
/// LiveKit media server, its config, a site on the gateway and three published ports.
/// These pin what a hub with it generates — and that a hub without it generates none of
/// it, since the golden fixtures are upstream's and upstream has no LiveKit.
fn hub(services: Vec<ServiceId>, mesh: Option<MeshOptions>) -> HubConfig {
    build_hub_config(&HubConfigOptions {
        device_id: "device".into(),
        coord_server: "go.arkitekt.live".into(),
        services: Some(services),
        mesh,
        ..Default::default()
    })
}

fn yaml(files: &std::collections::BTreeMap<String, String>, name: &str) -> Value {
    let text = files
        .get(name)
        .unwrap_or_else(|| panic!("{name} was not generated"));
    serde_norway::from_str(text).unwrap_or_else(|e| panic!("{name} is not YAML: {e}"))
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_sequence()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_hub_with_lovekit_runs_a_media_server() {
    let mut config = hub(vec![ServiceId::Mikro, ServiceId::Lovekit], None);
    config.place_livekit(&["localhost".into(), "192.168.1.20".into()]);
    let livekit = config
        .running_livekit()
        .expect("minted with Lovekit")
        .clone();
    let files = generate_hub_files(&config, &IssuedIdentity::default());

    // LiveKit's own config: the one key pair, the ports, and the address it announces.
    let media = yaml(&files, "configs/livekit.yaml");
    assert_eq!(media["port"].as_u64(), Some(7880));
    assert_eq!(media["rtc"]["tcp_port"].as_u64(), Some(2757));
    assert_eq!(media["rtc"]["udp_port"].as_u64(), Some(2758));
    assert_eq!(media["rtc"]["use_external_ip"].as_bool(), Some(false));
    assert_eq!(media["rtc"]["node_ip"].as_str(), Some("192.168.1.20"));
    assert_eq!(
        media["keys"][livekit.api_key.as_str()].as_str(),
        Some(livekit.api_secret.as_str())
    );
    assert!(
        livekit.api_secret.len() >= 32,
        "LiveKit refuses a shorter one"
    );

    // Lovekit signs its tokens with that pair, and calls the API inside the stack.
    let lovekit = yaml(&files, "configs/lovekit.yaml");
    assert_eq!(
        lovekit["livekit"]["api_key"].as_str(),
        Some(livekit.api_key.as_str())
    );
    assert_eq!(
        lovekit["livekit"]["api_secret"].as_str(),
        Some(livekit.api_secret.as_str())
    );
    assert_eq!(
        lovekit["livekit"]["api_url"].as_str(),
        Some("http://livekit:7880")
    );

    // The container reads the file, and publishes the media ports as they are.
    let compose = yaml(&files, "docker-compose.yaml");
    let service = &compose["services"]["livekit"];
    assert_eq!(service["image"].as_str(), Some(livekit.image.as_str()));
    assert_eq!(
        strings(&service["command"]),
        ["--config", "/etc/livekit.yaml"]
    );
    assert_eq!(strings(&service["ports"]), ["2757:2757", "2758:2758/udp"]);
    assert_eq!(
        strings(&service["volumes"]),
        ["./configs/livekit.yaml:/etc/livekit.yaml:ro"]
    );

    // Signalling is the gateway's: a site of its own, on a port published as it is.
    assert!(strings(&compose["services"]["gateway"]["ports"]).contains(&"2756:2756".into()));
    let caddyfile = &files["configs/Caddyfile"];
    assert!(
        caddyfile.contains(":2756 {\n\treverse_proxy livekit:7880\n}"),
        "{caddyfile}"
    );
}

#[test]
fn a_hub_without_lovekit_generates_none_of_it() {
    let files = generate_hub_files(
        &hub(vec![ServiceId::Mikro], None),
        &IssuedIdentity::default(),
    );
    assert!(!files.contains_key("configs/livekit.yaml"));
    let everything: String = files.values().cloned().collect();
    assert!(!everything.to_lowercase().contains("livekit"));
    assert!(!everything.contains("7880") && !everything.contains("2756"));
}

/// A mesh-only hub opens no port on this machine, the media ports included; the
/// signalling stays the gateway's, inside the sidecar's namespace.
#[test]
fn a_mesh_only_hub_publishes_none_of_the_media_ports() {
    let mut config = hub(
        vec![ServiceId::Lovekit],
        Some(MeshOptions {
            hostname: "lab-hub".into(),
            auth_key: "tskey-auth-EXAMPLE".into(),
            coord_url: None,
            login: None,
        }),
    );
    config.mesh.as_mut().expect("a mesh").mesh_only = true;
    config.gateway.exposed_http_port = None;
    config.gateway.exposed_https_port = None;
    let files = generate_hub_files(&config, &IssuedIdentity::default());

    let compose = yaml(&files, "docker-compose.yaml");
    assert!(compose["services"]["livekit"]["image"].is_string());
    assert!(compose["services"]["livekit"].get("ports").is_none());
    let published: String = serde_norway::to_string(&compose).expect("serializes");
    assert!(!published.contains("2756:2756"), "{published}");
    assert!(files["configs/Caddyfile"].contains(":2756 {"));
}

/// The two that are services like any other: a config each, a route each, and storage
/// only for the one that stores something.
#[test]
fn dokuments_and_lokate_are_generated_like_any_service() {
    let files = generate_hub_files(
        &hub(vec![ServiceId::Dokuments, ServiceId::Lokate], None),
        &IssuedIdentity::default(),
    );

    let dokuments = yaml(&files, "configs/dokuments.yaml");
    assert_eq!(dokuments["postgres"]["db_name"].as_str(), Some("dokuments"));
    assert_eq!(
        dokuments["django"]["force_script_name"].as_str(),
        Some("dokuments")
    );
    assert!(
        dokuments["datalayer"]["media"]["bucket"].is_string(),
        "Dokuments' settings read its media bucket on boot"
    );

    let lokate = yaml(&files, "configs/lokate.yaml");
    assert_eq!(lokate["postgres"]["db_name"].as_str(), Some("lokate"));
    assert!(
        lokate.get("datalayer").is_none(),
        "Lokate stores no objects"
    );

    let compose = yaml(&files, "docker-compose.yaml");
    for host in ["dokuments", "lokate"] {
        assert!(compose["services"][host]["image"].is_string(), "{host}");
        assert!(
            files["configs/Caddyfile"].contains(&format!("reverse_proxy {host}:80")),
            "{host}"
        );
    }
}

/// Docker itself accepts a hub running everything there is, plain and on a mesh — the
/// media ports beside `network_mode: service:` are the fussy part.
#[test]
fn docker_accepts_a_hub_running_every_service() {
    let available = konstruktor_core::docker::command()
        .args(["compose", "version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !available {
        eprintln!("skipping: no docker compose on this machine");
        return;
    }

    for (label, mesh) in [
        ("everything", None),
        (
            "everything-meshed",
            Some(MeshOptions {
                hostname: "lab-hub".into(),
                auth_key: "tskey-auth-EXAMPLE".into(),
                coord_url: Some("https://mesh.example.org".into()),
                login: None,
            }),
        ),
    ] {
        let config = hub(SERVICE_IDS.to_vec(), mesh);
        let files = generate_hub_files(&config, &IssuedIdentity::default());
        let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(label);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        konstruktor_core::generate::write::write_generated_files(&dir, &files)
            .expect("files are written");

        let output = konstruktor_core::docker::command()
            .args(["compose", "config", "-q"])
            .current_dir(&dir)
            .output()
            .expect("docker runs");
        assert!(
            output.status.success(),
            "docker compose rejected the generated project ({label}):\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
