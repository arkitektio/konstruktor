use serde_norway::{Mapping, Value};

use crate::catalog::ServiceId;
use crate::config::hub::{HubConfig, ServiceBlock, DB_COMPOSE_SERVICE};
use crate::config::mesh::{
    MESH_SOCKET as TAILSCALE_SOCKET, MESH_SOCKET_DIR as TAILSCALE_SOCKET_DIR,
    MESH_SOCKET_VOLUME as TAILSCALE_SOCKET_VOLUME, MESH_STATE_DIR,
};
use crate::credentials::CREDENTIALS_FILENAME;
use crate::generate::service::{list, map, s};
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
    service.bucket_names(id).into_iter().map(|(_, name)| name).collect()
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

fn compose_service(config: &HubConfig, service: &ServiceBlock) -> Value {
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

    map(vec![
        (
            "image",
            s(service
                .image
                .as_deref()
                .expect("a service without an image is never emitted")),
        ),
        // `run-debug.sh` is the debug variant; nothing in the GUI turns debug on.
        (
            "command",
            s(if service.debug {
                "bash run-debug.sh"
            } else {
                "bash run.sh"
            }),
        ),
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
    ])
}

/// The bucket + user manifest the init container (`rustfs_init`) reads. `None` when nothing declares a bucket.
pub fn build_minio_init(config: &HubConfig, enabled: &[ServiceId]) -> Option<Value> {
    let buckets: Vec<String> = enabled
        .iter()
        .flat_map(|id| buckets_of(*id, config.service(*id)))
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

pub fn build_compose(config: &HubConfig, enabled: &[ServiceId]) -> Value {
    let mut services = Value::Mapping(Mapping::new());

    // --- infrastructure -------------------------------------------------------
    let databases: Vec<String> = enabled
        .iter()
        .map(|id| config.service(*id).db_config.db.clone())
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

    if !enabled.is_empty() {
        insert(
            &mut services,
            &config.local_redis.host,
            map(vec![("image", s(&config.local_redis.image))]),
        );
    }

    let has_buckets = enabled
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

    // --- the services themselves ---------------------------------------------
    for id in enabled {
        let service = config.service(*id);
        insert(&mut services, &service.host, compose_service(config, service));
    }

    // --- the model provider, when this hub runs its own -----------------------
    //
    // Upstream's generator has no such service: `ollama_config: {kind: local}` names a
    // provider and stops there. This is the one place the generated stack deliberately
    // goes beyond what the Python CLI produces, and it only does so when somebody asked
    // for it — a hub that did not is byte-identical to upstream's output.
    if let Some(ollama) = config.local_ollama.as_ref().filter(|o| o.enabled) {
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

    // --- gateway, and the mesh sidecar when there is one ----------------------
    let mut ports: Vec<Value> = Vec::new();
    if let Some(port) = config.gateway.exposed_http_port {
        ports.push(s(&format!("{port}:80")));
    }
    if let Some(port) = config.gateway.exposed_https_port {
        ports.push(s(&format!("{port}:443")));
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
            sidecar_volumes.push(s(&format!("{TAILSCALE_SOCKET_VOLUME}:{TAILSCALE_SOCKET_DIR}")));
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
                ("depends_on", list(vec![s(&mesh.host)])),
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
            s(&format!("./{CREDENTIALS_FILENAME}:/seed/{CREDENTIALS_FILENAME}:ro")),
            s(&format!("./{PROFILE_FILENAME}:/seed/{PROFILE_FILENAME}:ro")),
            s(&format!("{}:/state", reporter.volume_name)),
        ];
        if share_socket {
            mounts.push(s(&format!("{TAILSCALE_SOCKET_VOLUME}:{TAILSCALE_SOCKET_DIR}:ro")));
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
    if let Some(ollama) = config.local_ollama.as_ref().filter(|o| o.enabled) {
        insert(&mut volumes, &ollama.volume_name, empty_map());
    }
    if let Some(reporter) = reporter {
        insert(&mut volumes, &reporter.volume_name, empty_map());
    }
    if share_socket {
        insert(&mut volumes, TAILSCALE_SOCKET_VOLUME, empty_map());
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
