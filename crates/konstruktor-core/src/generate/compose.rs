use serde_norway::{Mapping, Value};

use crate::catalog::ServiceId;
use crate::config::hub::{
    HubConfig, LivekitBlock, ServiceBlock, DB_COMPOSE_SERVICE, LIVEKIT_INTERNAL_PORT,
    TAKT_SOCKET_DIR, TAKT_SOCKET_PATH, TAKT_SOCKET_VOLUME,
};
use crate::config::mesh::{
    MESH_ENV_FILE, MESH_SOCKET as TAILSCALE_SOCKET, MESH_SOCKET_DIR as TAILSCALE_SOCKET_DIR,
    MESH_SOCKET_VOLUME as TAILSCALE_SOCKET_VOLUME, MESH_STATE_DIR,
};
use crate::credentials::CREDENTIALS_FILENAME;
use crate::generate::service::{fernet_key_file, fernet_key_path, list, map, s};
use crate::profile::HUB_CONFIG_FILENAME as PROFILE_FILENAME;

/// `docker-compose.yaml`, and the bucket manifest the storage init container reads.

fn empty_map() -> Value {
    Value::Mapping(Mapping::new())
}

fn insert(target: &mut Value, key: &str, value: Value) {
    if let Value::Mapping(m) = target {
        m.insert(Value::from(key), value);
    }
}

/// The bucket names a service declares, in `bucket_purposes()` order.
fn buckets_of(id: ServiceId, service: &ServiceBlock) -> Vec<String> {
    service
        .bucket_names(id)
        .into_iter()
        .map(|(_, name)| name)
        .collect()
}

/// `enabled`, then the services taken out that keep their data — whose database and
/// buckets stay provisioned (see [`ServiceBlock::retained`]).
fn provisioned(config: &HubConfig, enabled: &[ServiceId]) -> Vec<ServiceId> {
    let mut out = enabled.to_vec();
    for id in config.provisioned_services() {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// Where a dev hub's checkouts live, relative to the deployment folder.
///
/// One folder per service, named after the service — `./mounts/rekuest` — so the folder
/// tells you which repository you are in without reading a remote.
pub const MOUNTS_DIR: &str = "mounts";

/// The checkout folder for one service, as compose writes it.
pub fn mount_path(service: &ServiceBlock) -> String {
    format!("./{MOUNTS_DIR}/{}", service.host)
}

fn compose_service(
    config: &HubConfig,
    service: &ServiceBlock,
    said: &crate::contract::Said,
) -> Value {
    // The config file is mounted *inside* the workspace, so on a dev hub the source mount
    // is the parent of the config mount. Docker resolves nested binds outermost-first
    // regardless of the order they are declared, so the config still lands on top of the
    // checkout rather than being hidden by it — but the two are written in that order
    // anyway, because reading them the other way round invites the wrong conclusion.
    let mut volumes = Vec::new();
    if service.mount_github {
        volumes.push(s(&format!("{}:/workspace", mount_path(service))));
    }
    volumes.push(s(&format!(
        "./configs/{}.yaml:/workspace/config.yaml",
        service.host
    )));
    if service.fernet_key.is_some() {
        volumes.push(s(&format!(
            "./{}:{}:ro",
            fernet_key_file(service),
            fernet_key_path(service)
        )));
    }
    // Rekuest reaches takt's internal API through the socket in the volume the two share.
    if service.host == config.rekuest.host && config.takt_image().is_some() {
        volumes.push(s(&format!("{TAKT_SOCKET_VOLUME}:{TAKT_SOCKET_DIR}")));
    }

    let mut entries = vec![(
        "image",
        s(service
            .image
            .as_deref()
            .expect("a service without an image is never emitted")),
    )];
    // How a service is started is its image's to say (`serve`, or `debug` when asked for;
    // nothing in the GUI turns debug on). No command is written for an image that was not
    // asked — and its own only says what it is, so a hub is never written without asking.
    if let Some(command) = said
        .get(&service.host)
        .and_then(|said| said.command(service.debug))
    {
        entries.push((
            "command",
            list(command.iter().map(|part| s(part)).collect()),
        ));
    }
    entries.extend(vec![
        // By the profile's own names: the object storage was renamed from `minio` to
        // `rustfs`, and a dependency on a service that is not in the file is an error
        // compose refuses the whole project over.
        (
            "depends_on",
            list(vec![
                s(&config.local_redis.host),
                s(DB_COMPOSE_SERVICE),
                s(&config.minio.host),
            ]),
        ),
        ("stop_grace_period", s("2s")),
        ("volumes", list(volumes)),
        (
            "deploy",
            map(vec![(
                "restart_policy",
                map(vec![
                    ("condition", s("on-failure")),
                    ("delay", s("10s")),
                    ("max_attempts", Value::from(10)),
                    ("window", s("300s")),
                ]),
            )]),
        ),
    ]);
    map(entries)
}

/// The compose service Rekuest's reaper ran as, before takt took its work over.
///
/// Nothing generates it any more. It is named only to recognise a hub whose files predate
/// takt: such a hub runs a compose file with this service and without takt's, and must be
/// regenerated before its Rekuest can move to an image that expects takt.
pub fn legacy_reaper_host(config: &HubConfig) -> String {
    format!("{}-reaper", config.rekuest.host)
}

/// The compose services that move with `service` when its image moves, and are restarted
/// with it when its config changes: takt, for Rekuest. The two are one release and read one
/// config file.
pub fn companions(config: &HubConfig, service: &str) -> Vec<String> {
    if service == config.rekuest.host {
        config.takt_host().into_iter().collect()
    } else {
        Vec::new()
    }
}

/// The service `companion` moves with, if it is one: Rekuest, for takt.
pub fn companion_of(config: &HubConfig, companion: &str) -> Option<String> {
    (config.takt_host().as_deref() == Some(companion)).then(|| config.rekuest.host.clone())
}

/// takt: its own image, Rekuest's config (read-only: the same file, mounted where the image
/// looks for it) and the volume its internal socket lives in, which only Rekuest mounts
/// too — no source mount, no object storage. Its health is the
/// image's own `HEALTHCHECK` (`takt healthcheck`), which only passes once Rekuest has
/// migrated, so it depends on Rekuest having started and waits for the rest itself.
fn takt_service(config: &HubConfig, image: &str) -> Value {
    let rekuest = &config.rekuest;
    map(vec![
        ("image", s(image)),
        (
            "depends_on",
            list(vec![
                s(DB_COMPOSE_SERVICE),
                s(&config.local_redis.host),
                s(&rekuest.host),
            ]),
        ),
        ("stop_grace_period", s("2s")),
        // The internal API is not on the port agents reach: only Rekuest mounts this socket.
        (
            "environment",
            map(vec![(
                "TAKT_INTERNAL_BIND",
                s(&format!("unix:{TAKT_SOCKET_PATH}")),
            )]),
        ),
        (
            "volumes",
            list(vec![
                s(&format!(
                    "./configs/{}.yaml:/workspace/config.yaml:ro",
                    rekuest.host
                )),
                s(&format!("{TAKT_SOCKET_VOLUME}:{TAKT_SOCKET_DIR}")),
            ]),
        ),
        (
            "deploy",
            map(vec![(
                "restart_policy",
                map(vec![
                    ("condition", s("on-failure")),
                    ("delay", s("10s")),
                    ("max_attempts", Value::from(10)),
                    ("window", s("300s")),
                ]),
            )]),
        ),
    ])
}

/// Where LiveKit's config is mounted inside its container.
const LIVEKIT_CONFIG_PATH: &str = "/etc/livekit.yaml";

/// Whether the hub publishes nothing on this machine: a mesh-only hub, reached through its
/// tailnet node alone.
fn publishes_nothing(config: &HubConfig) -> bool {
    config
        .mesh
        .as_ref()
        .is_some_and(|m| m.enabled && m.mesh_only)
}

/// `configs/livekit.yaml`: what the media server reads instead of `--dev`'s fixed key.
///
/// The key names are LiveKit's own (`config-sample.yaml`). `use_external_ip` is off on
/// purpose: it has LiveKit ask a STUN server for this machine's public address and
/// announce that, which is the one address the media ports are not reachable on. It
/// announces [`LivekitBlock::node_ip`] instead — or, without one, the container's own
/// address, which only this machine can reach.
pub fn build_livekit_config(livekit: &LivekitBlock) -> Value {
    let mut rtc = vec![
        ("tcp_port", Value::from(livekit.rtc_tcp_port)),
        ("udp_port", Value::from(livekit.rtc_udp_port)),
        ("use_external_ip", Value::from(false)),
    ];
    if let Some(ip) = livekit.node_ip.as_deref().filter(|ip| !ip.is_empty()) {
        rtc.push(("node_ip", s(ip)));
    }
    map(vec![
        ("port", Value::from(LIVEKIT_INTERNAL_PORT)),
        ("rtc", map(rtc)),
        (
            "keys",
            map(vec![(livekit.api_key.as_str(), s(&livekit.api_secret))]),
        ),
    ])
}

/// LiveKit: someone else's image, its config, and the two ports its media flows on.
///
/// Those are published as they are — LiveKit announces the port it listens on, so the two
/// sides of the mapping must match — and by this container rather than the gateway: the
/// media never passes through it. A mesh-only hub publishes nothing, here as elsewhere.
fn livekit_service(config: &HubConfig, livekit: &LivekitBlock) -> Value {
    let mut service = vec![
        ("image", s(&livekit.image)),
        ("command", list(vec![s("--config"), s(LIVEKIT_CONFIG_PATH)])),
    ];
    if !publishes_nothing(config) {
        service.push((
            "ports",
            list(vec![
                s(&format!("{0}:{0}", livekit.rtc_tcp_port)),
                s(&format!("{0}:{0}/udp", livekit.rtc_udp_port)),
            ]),
        ));
    }
    service.extend([
        ("stop_grace_period", s("2s")),
        (
            "volumes",
            list(vec![s(&format!(
                "./configs/{}.yaml:{LIVEKIT_CONFIG_PATH}:ro",
                livekit.host
            ))]),
        ),
        (
            "deploy",
            map(vec![(
                "restart_policy",
                map(vec![
                    ("condition", s("on-failure")),
                    ("delay", s("10s")),
                    ("max_attempts", Value::from(10)),
                    ("window", s("300s")),
                ]),
            )]),
        ),
    ]);
    map(service)
}

/// The bucket + user manifest the init container (`rustfs_init`) reads. `None` when nothing declares a bucket.
pub fn build_minio_init(config: &HubConfig, enabled: &[ServiceId]) -> Option<Value> {
    let buckets: Vec<String> = provisioned(config, enabled)
        .iter()
        .flat_map(|id| buckets_of(*id, config.service(*id)))
        .chain(
            config
                .running_lok()
                .map(|lok| lok.media_bucket.bucket_name.clone()),
        )
        .collect();

    if buckets.is_empty() {
        return None;
    }

    Some(map(vec![
        (
            "buckets",
            list(
                buckets
                    .iter()
                    .map(|name| map(vec![("name", s(name))]))
                    .collect(),
            ),
        ),
        (
            "users",
            list(vec![map(vec![
                ("access_key", s(&config.minio.access_key)),
                ("name", s("Default User")),
                ("policies", list(vec![s("readwrite")])),
                ("secret_key", s(&config.minio.secret_key)),
            ])]),
        ),
    ]))
}

pub fn build_compose(
    config: &HubConfig,
    enabled: &[ServiceId],
    said: &crate::contract::Said,
) -> Value {
    let mut services = Value::Mapping(Mapping::new());

    // --- infrastructure -------------------------------------------------------
    let provisioned = provisioned(config, enabled);
    let lok = config.running_lok();
    let databases: Vec<String> = provisioned
        .iter()
        .map(|id| config.service(*id).db_config.db.clone())
        .chain(lok.map(|lok| lok.db.clone()))
        .collect();

    if !databases.is_empty() {
        insert(
            &mut services,
            DB_COMPOSE_SERVICE,
            map(vec![
                ("image", s(&config.db.image)),
                (
                    "environment",
                    map(vec![
                        ("POSTGRES_MULTIPLE_DATABASES", s(&databases.join(","))),
                        ("POSTGRES_PASSWORD", s(&config.db.postgres_password)),
                        ("POSTGRES_USER", s(&config.db.postgres_user)),
                    ]),
                ),
                (
                    "volumes",
                    list(vec![s(&format!(
                        "{}:/var/lib/postgresql/data",
                        // JavaScript's `||` falls through on the empty string, so a blank
                        // mount means "use the named volume", not "mount nothing".
                        non_empty(config.db.mount.as_deref()).unwrap_or(&config.db.volume_name)
                    ))]),
                ),
            ]),
        );
    }

    if !enabled.is_empty() || lok.is_some() {
        insert(
            &mut services,
            &config.local_redis.host,
            map(vec![("image", s(&config.local_redis.image))]),
        );
    }

    let has_buckets = lok.is_some()
        || provisioned
            .iter()
            .any(|id| !buckets_of(*id, config.service(*id)).is_empty());

    if has_buckets {
        insert(
            &mut services,
            &config.minio.host,
            map(vec![
                ("image", s(&config.minio.image)),
                // RustFS reads everything from the environment; there is no `server /data`
                // command as MinIO had. The root credentials are the admin account the
                // init container provisions the services' own user with.
                (
                    "environment",
                    map(vec![
                        ("RUSTFS_ACCESS_KEY", s(&config.minio.root_user)),
                        ("RUSTFS_SECRET_KEY", s(&config.minio.root_password)),
                        ("RUSTFS_VOLUMES", s("/data")),
                        (
                            "RUSTFS_ADDRESS",
                            s(&format!(":{}", config.minio.internal_port)),
                        ),
                    ]),
                ),
                // The image runs as uid 10001 and leaves `/data` as it finds it. A bind
                // mount the engine created is root's, and a restore copies the buckets
                // back as root, so as 10001 the server dies on `Permission denied` before
                // it listens. MinIO ran as root; so does this.
                ("user", s("0:0")),
                ("stop_grace_period", s("2s")),
                (
                    "volumes",
                    list(vec![s(&format!(
                        "{}:/data",
                        non_empty(config.minio.mount.as_deref())
                            .unwrap_or(&config.minio.volume_name)
                    ))]),
                ),
            ]),
        );

        insert(
            &mut services,
            &config.minio.init_container_host,
            map(vec![
                ("image", s(&config.minio.init_container_image)),
                (
                    "volumes",
                    list(vec![s(&format!(
                        "./configs/{}.yaml:/workspace/config.yaml",
                        config.minio.init_container_host
                    ))]),
                ),
                ("stop_grace_period", s("2s")),
                (
                    "environment",
                    map(vec![
                        ("RUSTFS_ACCESS_KEY", s(&config.minio.root_user)),
                        ("RUSTFS_SECRET_KEY", s(&config.minio.root_password)),
                        (
                            "RUSTFS_HOST",
                            s(&format!(
                                "http://{}:{}",
                                config.minio.host, config.minio.internal_port
                            )),
                        ),
                    ]),
                ),
                (
                    "depends_on",
                    map(vec![(
                        config.minio.host.as_str(),
                        map(vec![("condition", s("service_started"))]),
                    )]),
                ),
            ]),
        );
    }

    // --- the coordination server, on a hub that runs its own -------------------
    if let Some(lok) = lok {
        insert(
            &mut services,
            &lok.host,
            crate::generate::lok::lok_compose_service(config, lok, said),
        );
    }

    // --- the services themselves ---------------------------------------------
    for id in enabled {
        let service = config.service(*id);
        insert(
            &mut services,
            &service.host,
            compose_service(config, service, said),
        );
    }
    if enabled.contains(&ServiceId::Rekuest) {
        if let (Some(takt), Some(image)) = (config.takt_host(), config.takt_image()) {
            insert(&mut services, &takt, takt_service(config, &image));
        }
    }

    // --- the model provider, when this hub runs its own -----------------------
    //
    // Upstream's generator has no such service: `ollama_config: {kind: local}` names a
    // provider and stops there. This is the one place the generated stack deliberately
    // goes beyond what the Python CLI produces, and it only does so when somebody asked
    // for it — a hub that did not is byte-identical to upstream's output.
    if let Some(ollama) = config.running_ollama() {
        insert(
            &mut services,
            &ollama.host,
            map(vec![
                ("image", s(&ollama.image)),
                // Models are pulled at runtime and are gigabytes each, so the volume is
                // the point of the service — without it every restart re-downloads them.
                (
                    "volumes",
                    list(vec![s(&format!("{}:/root/.ollama", ollama.volume_name))]),
                ),
            ]),
        );
    }

    // --- Lovekit's media server, while Lovekit runs ---------------------------
    //
    // Beyond upstream, like the model provider above: its generator has no LiveKit, and
    // a hub without Lovekit is byte-identical to what it writes.
    let livekit = config
        .running_livekit()
        .filter(|_| enabled.contains(&ServiceId::Lovekit));
    if let Some(livekit) = livekit {
        insert(
            &mut services,
            &livekit.host,
            livekit_service(config, livekit),
        );
    }

    // --- gateway, and the mesh sidecar when there is one ----------------------
    let mut ports: Vec<Value> = Vec::new();
    if let Some(port) = config.gateway.exposed_http_port {
        ports.push(s(&format!("{port}:80")));
    }
    if let Some(port) = config.gateway.exposed_https_port {
        ports.push(s(&format!("{port}:443")));
    }
    // LiveKit's signalling is the gateway's to serve, on a port of its own — the same
    // inside and out, since that is the port every alias of it names.
    if let Some(livekit) = livekit.filter(|_| !publishes_nothing(config)) {
        ports.push(s(&format!("{0}:{0}", livekit.signal_port)));
    }

    let mesh = config.mesh.as_ref().filter(|m| m.enabled);
    let reporter = config.reporter.as_ref().filter(|r| r.enabled);
    // The sidecar's LocalAPI socket, shared so the reporter can say what the tailnet
    // calls this node. Only when there is a reporter to read it.
    let share_socket = mesh.is_some() && reporter.is_some();

    if let Some(mesh) = mesh {
        // The sidecar holds the network namespace and the gateway moves into it, which is
        // what puts the hub on the tailnet under its own name rather than behind whatever
        // address the host happens to have. The published ports move with it: docker binds
        // them on the namespace's owner, and `network_mode: service:` forbids declaring
        // `ports` or `networks` on the member.
        let mut environment: Vec<(&str, Value)> = mesh
            .sidecar_environment()
            .into_iter()
            .map(|(key, value)| (key, s(&value)))
            .collect();
        if share_socket {
            environment.push(("TS_SOCKET", s(TAILSCALE_SOCKET)));
        }
        let mut sidecar_volumes = vec![
            s(&format!("{}:{}", mesh.volume_name, MESH_STATE_DIR)),
            s("/dev/net/tun:/dev/net/tun"),
        ];
        if share_socket {
            sidecar_volumes.push(s(&format!(
                "{TAILSCALE_SOCKET_VOLUME}:{TAILSCALE_SOCKET_DIR}"
            )));
        }

        // A member of the sidecar's namespace has no name of its own on the network, so
        // the sidecar answers to the gateway's too: plugin apps and services beside the
        // stack reach Caddy as `gateway` whether or not the hub is on a mesh.
        let gateway_alias = || map(vec![("aliases", list(vec![s(&config.gateway.host)]))]);
        let networks = map(vec![
            (config.internal_network.as_str(), gateway_alias()),
            ("default", gateway_alias()),
        ]);

        let mut sidecar = vec![
            ("image", s(&mesh.image)),
            ("hostname", s(&mesh.hostname)),
            ("environment", map(environment)),
            // The key, kept out of this file. See `MESH_ENV_FILE`.
            ("env_file", list(vec![s(MESH_ENV_FILE)])),
        ];
        // A mesh-only hub publishes nothing, and an empty `ports:` is noise.
        if !ports.is_empty() {
            sidecar.push(("ports", list(ports.clone())));
        }
        sidecar.extend([
            ("networks", networks),
            ("volumes", list(sidecar_volumes)),
            ("cap_add", list(vec![s("net_admin"), s("sys_module")])),
            // Not `unless-stopped`: the sidecar should recover from its own crashes, but
            // never come back on its own after the daemon or the host restarts — the app
            // is what decides whether a stack is running.
            ("restart", s("on-failure")),
        ]);

        insert(&mut services, &mesh.host, map(sidecar));

        insert(
            &mut services,
            &config.gateway.host,
            map(vec![
                ("image", s(&config.gateway.image)),
                ("network_mode", s(&format!("service:{}", mesh.host))),
                // `restart`: the gateway lives in the sidecar's network namespace, and a
                // sidecar that is recreated or restarts takes that namespace with it —
                // leaving Caddy running in a dead one, reachable by nothing, until it too
                // is restarted.
                (
                    "depends_on",
                    map(vec![(
                        mesh.host.as_str(),
                        map(vec![
                            ("condition", s("service_started")),
                            ("restart", Value::from(true)),
                        ]),
                    )]),
                ),
                (
                    "volumes",
                    list(vec![s("./configs/Caddyfile:/etc/caddy/Caddyfile")]),
                ),
            ]),
        );
    } else {
        insert(
            &mut services,
            &config.gateway.host,
            map(vec![
                ("image", s(&config.gateway.image)),
                ("ports", list(ports)),
                (
                    "networks",
                    list(vec![s(&config.internal_network), s("default")]),
                ),
                (
                    "volumes",
                    list(vec![s("./configs/Caddyfile:/etc/caddy/Caddyfile")]),
                ),
            ]),
        );
    }

    // --- the health reporter ---------------------------------------------------
    // Tells the coordination server the hub is alive, as the hub — see `hubhealth`. It
    // reaches the gateway by name on the default network, and reads the grant and the
    // profile it needs from the folder, read-only.
    if let Some(reporter) = reporter {
        let mut mounts = vec![
            s(&format!(
                "./{CREDENTIALS_FILENAME}:/seed/{CREDENTIALS_FILENAME}:ro"
            )),
            s(&format!("./{PROFILE_FILENAME}:/seed/{PROFILE_FILENAME}:ro")),
            s(&format!("{}:/state", reporter.volume_name)),
        ];
        if share_socket {
            mounts.push(s(&format!(
                "{TAILSCALE_SOCKET_VOLUME}:{TAILSCALE_SOCKET_DIR}:ro"
            )));
        }
        insert(
            &mut services,
            &reporter.host,
            map(vec![
                ("image", s(&reporter.image)),
                ("command", list(vec![s("hub-report")])),
                ("volumes", list(mounts)),
                ("depends_on", list(vec![s(&config.gateway.host)])),
                // Same reasoning as the sidecar: recover from a crash, but never start on
                // its own behind the app's back.
                ("restart", s("on-failure")),
            ]),
        );
    }

    // --- volumes and networks -------------------------------------------------
    // Only the mounts that are *not* bind mounts need a named volume declared.
    let mut volumes = Value::Mapping(Mapping::new());
    if non_empty(config.db.mount.as_deref()).is_none() {
        insert(&mut volumes, &config.db.volume_name, empty_map());
    }
    if non_empty(config.minio.mount.as_deref()).is_none() {
        insert(&mut volumes, &config.minio.volume_name, empty_map());
    }
    if let Some(mesh) = mesh {
        // The node identity has to survive `docker compose down`, or every restart rejoins
        // the tailnet as a new machine — and the pre-auth key is single-use.
        insert(&mut volumes, &mesh.volume_name, empty_map());
    }
    if let Some(ollama) = config.running_ollama() {
        insert(&mut volumes, &ollama.volume_name, empty_map());
    }
    if let Some(reporter) = reporter {
        insert(&mut volumes, &reporter.volume_name, empty_map());
    }
    if share_socket {
        insert(&mut volumes, TAILSCALE_SOCKET_VOLUME, empty_map());
    }
    if enabled.contains(&ServiceId::Rekuest) && config.takt_image().is_some() {
        insert(&mut volumes, TAKT_SOCKET_VOLUME, empty_map());
    }

    map(vec![
        ("services", services),
        (
            "networks",
            map(vec![(
                config.internal_network.as_str(),
                map(vec![
                    ("driver", s("bridge")),
                    ("name", s(&config.internal_network)),
                ]),
            )]),
        ),
        ("volumes", volumes),
    ])
}

/// JavaScript's `a || b`: an empty string falls through, not just null.
fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|v| !v.is_empty())
}
