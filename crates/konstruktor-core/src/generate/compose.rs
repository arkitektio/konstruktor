use serde_norway::{Mapping, Value};

use crate::catalog::ServiceId;
use crate::config::hub::{HubConfig, ServiceBlock};
use crate::config::mesh::MESH_STATE_DIR;
use crate::generate::service::{list, map, s};

/// `docker-compose.yaml`, and the bucket manifest the minio init container reads.

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
    id.bucket_purposes()
        .iter()
        .filter_map(|purpose| service.bucket(purpose).map(|b| b.bucket_name.clone()))
        .collect()
}

fn compose_service(service: &ServiceBlock) -> Value {
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
        ("depends_on", list(vec![s("redis"), s("db"), s("minio")])),
        ("stop_grace_period", s("2s")),
        (
            "volumes",
            list(vec![s(&format!(
                "./configs/{}.yaml:/workspace/config.yaml",
                service.host
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
    ])
}

/// The bucket + user manifest `minio_init` reads. `None` when nothing declares a bucket.
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
            list(buckets
                .iter()
                .map(|name| map(vec![("name", s(name))]))
                .collect()),
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
            "db",
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
                ("command", s("server /data")),
                (
                    "environment",
                    map(vec![
                        ("MINIO_ROOT_USER", s(&config.minio.root_user)),
                        ("MINIO_ROOT_PASSWORD", s(&config.minio.root_password)),
                    ]),
                ),
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
                        ("MINIO_ROOT_USER", s(&config.minio.root_user)),
                        ("MINIO_ROOT_PASSWORD", s(&config.minio.root_password)),
                        (
                            "MINIO_HOST",
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
        insert(&mut services, &service.host, compose_service(service));
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

    if let Some(mesh) = mesh {
        // The sidecar holds the network namespace and the gateway moves into it, which is
        // what puts the hub on the tailnet under its own name rather than behind whatever
        // address the host happens to have. The published ports move with it: docker binds
        // them on the namespace's owner, and `network_mode: service:` forbids declaring
        // `ports` or `networks` on the member.
        let mut environment = vec![
            ("TS_AUTHKEY", s(&mesh.auth_key)),
            ("TS_HOSTNAME", s(&mesh.hostname)),
            ("TS_STATE_DIR", s(MESH_STATE_DIR)),
            // The kernel networking path; userspace mode would not carry the gateway's
            // traffic for it.
            ("TS_USERSPACE", s("false")),
        ];
        let extra_args;
        if let Some(coord) = &mesh.coord_url {
            extra_args = format!("--login-server={coord}");
            environment.push(("TS_EXTRA_ARGS", s(&extra_args)));
        }

        insert(
            &mut services,
            &mesh.host,
            map(vec![
                ("image", s(&mesh.image)),
                ("hostname", s(&mesh.hostname)),
                ("environment", map(environment)),
                ("ports", list(ports.clone())),
                (
                    "networks",
                    list(vec![s(&config.internal_network), s("default")]),
                ),
                (
                    "volumes",
                    list(vec![
                        s(&format!("{}:{}", mesh.volume_name, MESH_STATE_DIR)),
                        s("/dev/net/tun:/dev/net/tun"),
                    ]),
                ),
                ("cap_add", list(vec![s("net_admin"), s("sys_module")])),
                // Not `unless-stopped`: the sidecar should recover from its own
                // crashes, but never come back on its own after the daemon or the host
                // restarts — the app is what decides whether a stack is running.
                ("restart", s("on-failure")),
            ]),
        );

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
