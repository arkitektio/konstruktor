//! What a coordination server is generated as beyond a hub's stack: Lok's public config, the
//! mesh control server's, and the containers that run beside Lok — the account frontend, the
//! mesh, and the job that keeps the mesh in step.
//!
//! Lok's config here is not a self-contained hub's ([`crate::generate::lok`]): there is no
//! hub to register, no mandate key, and no answering at whatever address a request names.
//! Lok issues tokens as one origin and answers only there, as everything that trusts it
//! expects.

use serde_norway::{Mapping, Value};

use crate::config::coord::{CoordBlock, IonskaleBlock, TRUSTED_ROOTS_FILE};
use crate::config::hub::{HubConfig, LokBlock, DB_COMPOSE_SERVICE};
use crate::generate::service::{list, map, s};

/// Where ionskale's config is written, and mounted from.
pub const IONSKALE_CONFIG_FILE: &str = "configs/ionskale.yaml";
/// Where the account frontend finds what its landing page says.
pub const LANDING_FILE: &str = "configs/kontrol/landing.json";

/// The container that runs Lok's `reconcile_meshes` job every so often: Lok has no scheduler,
/// and a client session that merely expires raises no event.
pub const RECONCILER_HOST: &str = "lok-reconciler";
/// How often it runs, in seconds: what Lok's documentation asks for.
pub const RECONCILE_EVERY_SECONDS: u32 = 900;
/// The job, and what it is run with: the sidecar pass only, which is the periodic one.
pub const RECONCILE_JOB: &str = "reconcile_meshes";
pub const RECONCILE_ARGS: &[&str] = &["--sidecars-only"];

/// `configs/lok.yaml` for a coordination server.
pub fn build_lok_config(config: &HubConfig, lok: &LokBlock, coord: &CoordBlock) -> Value {
    let origin = coord.origin.trim_end_matches('/');
    let admin = &config.global_admin;

    let mut django = vec![
        (
            "admin",
            map(vec![
                (
                    "email",
                    config
                        .global_admin_email
                        .as_deref()
                        .map(s)
                        .unwrap_or(Value::Null),
                ),
                ("password", s(&config.global_admin_password)),
                ("username", s(admin)),
            ]),
        ),
        // Only a lab one serves plain HTTP, where OAuth would otherwise be refused.
        ("allow_insecure_transport", Value::from(!coord.https())),
        // The OAuth, fakts and discovery endpoints are public and bearer-authenticated: an
        // app in a browser on any host asks them. Everything on a session stays same-origin.
        ("cors_allow_all_origins", Value::from(true)),
        ("csrf_trusted_origins", list(vec![s(origin)])),
        ("debug", Value::from(false)),
        ("force_script_name", s(&lok.host)),
        // Its own name, and the names it is asked at inside the stack: the gateway's (and
        // the probes', which arrive as `localhost`) and its own.
        (
            "hosts",
            list(
                lok_hosts(config, lok, coord)
                    .iter()
                    .map(|host| s(host))
                    .collect(),
            ),
        ),
        ("secret_key", s(&lok.secret_key)),
        ("trusted_proxy_depth", Value::from(1)),
    ];
    if !coord.https() {
        // A session cookie marked secure is never sent back over plain HTTP.
        django.push(("secure_cookies", Value::from(false)));
    }

    let mut entries = vec![
        ("django", map(django)),
        ("postgres", crate::generate::lok::postgres(config, &lok.db)),
        ("redis", crate::generate::lok::redis(config)),
        ("authentikate", crate::generate::lok::authentikate(lok)),
        ("datalayer", crate::generate::lok::datalayer(config, lok)),
        (
            "deployment",
            map(vec![
                ("name", s(&coord.name)),
                // Absolute: Lok sits behind a proxy, and the well-known names these.
                ("configure_url", s(&format!("{origin}/configure/{{code}}"))),
                (
                    "hub_configure_url",
                    s(&format!("{origin}/hubconfigure/{{code}}")),
                ),
                (
                    "mesh_configure_url",
                    s(&format!("{origin}/meshconfigure/{{code}}")),
                ),
            ]),
        ),
        (
            "lok",
            map(vec![
                ("key_id", s(&lok.key_id)),
                ("public_key", s(&lok.key_pair.public_key)),
            ]),
        ),
        ("private_key", s(&lok.key_pair.private_key)),
        // The bare origin: Lok's routes carry its prefix themselves, so `<origin>/lok`
        // would advertise `/lok/lok/o/token/`.
        ("oidc_issuer", s(origin)),
        // Never on a server others rely on: the address would be whatever a caller said.
        ("discovery_follows_request", Value::from(false)),
        ("kontrol_frontend_url", s(origin)),
        (
            "account",
            map(vec![("allow_signup", Value::from(coord.signup.is_open()))]),
        ),
        (
            "users",
            list(
                lok.users
                    .iter()
                    .map(|user| {
                        let mut pairs = vec![
                            ("username", s(&user.username)),
                            ("password", s(&user.password)),
                        ];
                        if let Some(email) = &user.email {
                            pairs.push(("email", s(email)));
                        }
                        if user.superuser {
                            pairs.push(("is_superuser", Value::from(true)));
                            pairs.push(("is_staff", Value::from(true)));
                        }
                        map(pairs)
                    })
                    .collect(),
            ),
        ),
        (
            "organizations",
            list(vec![map(vec![
                ("id", Value::from(lok.organization.id)),
                ("name", s(&lok.organization.name)),
                ("identifier", s(&lok.organization.identifier)),
                ("owner", s(admin)),
                // There is no hub of its own to configure it with.
                ("auto_configure", Value::from(false)),
            ])]),
        ),
        (
            "memberships",
            list(
                lok.users
                    .iter()
                    .map(|user| {
                        map(vec![
                            ("user", s(&user.username)),
                            ("organization", s(&lok.organization.identifier)),
                            ("roles", list(vec![s("admin")])),
                        ])
                    })
                    .collect(),
            ),
        ),
    ];

    if let Some(email) = &coord.email {
        entries.push((
            "email",
            map(vec![
                ("host", s(&email.host)),
                ("port", Value::from(email.port)),
                ("use_tls", Value::from(email.use_tls)),
                ("user", s(&email.user)),
                ("password", s(&email.password)),
                ("email", s(&email.from)),
            ]),
        ));
    }

    if let Some(mesh) = &coord.mesh {
        entries.push((
            "ionscale",
            map(vec![
                // Inside the stack, past the gateway: Lok drives it on the stack's network.
                (
                    "server_url",
                    s(&format!("http://{}:{}", mesh.host, mesh.internal_port)),
                ),
                ("coord_url", s(&mesh.url())),
                ("service_token", s(&mesh.service_token)),
                ("magic_dns_suffix", s(&mesh.dns_suffix)),
                ("auto_create_mesh", Value::from(true)),
            ]),
        ));
        entries.push((
            "openid_apps",
            list(vec![map(vec![
                ("client_name", s("Mesh")),
                ("client_id", s(&mesh.client_id)),
                ("client_secret", s(&mesh.client_secret)),
                ("redirect_uris", list(vec![s(&mesh.callback_url())])),
                ("scope", s("openid profile email")),
            ])]),
        ));
    }

    map(entries)
}

/// What Lok answers to: its origin's host, and the names it is asked at inside the stack.
pub fn lok_hosts(config: &HubConfig, lok: &LokBlock, coord: &CoordBlock) -> Vec<String> {
    let mut hosts = vec![
        coord.host().to_string(),
        config.gateway.host.clone(),
        lok.host.clone(),
        "localhost".to_string(),
    ];
    hosts.dedup();
    hosts
}

/// `configs/ionskale.yaml`.
///
/// Behind the gateway, which terminates TLS for its name as for Lok's: one process owns 443.
/// Its users log in through Lok's OpenID provider, scoped to the organization their token
/// names — the tailnet of an organization is offered to its members and nobody else.
pub fn build_ionskale_config(
    config: &HubConfig,
    coord: &CoordBlock,
    mesh: &IonskaleBlock,
) -> Value {
    map(vec![
        ("listen_addr", s(&format!(":{}", mesh.internal_port))),
        ("public_addr", s(&format!("{}:443", mesh.domain))),
        ("stun_listen_addr", s(&format!(":{}", mesh.stun_port))),
        (
            "stun_public_addr",
            s(&format!("{}:{}", mesh.domain, mesh.stun_port)),
        ),
        (
            "tls",
            map(vec![
                ("disable", Value::from(true)),
                ("force_https", Value::from(false)),
            ]),
        ),
        (
            "database",
            map(vec![
                ("type", s("postgres")),
                (
                    "url",
                    s(&format!(
                        "postgres://{}:{}@{}:5432/{}?sslmode=disable",
                        config.db.postgres_user,
                        config.db.postgres_password,
                        DB_COMPOSE_SERVICE,
                        mesh.db
                    )),
                ),
            ]),
        ),
        (
            "auth",
            map(vec![
                (
                    "provider",
                    map(vec![
                        ("issuer", s(coord.origin.trim_end_matches('/'))),
                        ("client_id", s(&mesh.client_id)),
                        ("client_secret", s(&mesh.client_secret)),
                        ("additional_scopes", list(vec![s("profile"), s("email")])),
                    ]),
                ),
                (
                    "organizations",
                    map(vec![
                        ("claim", s("org")),
                        ("roles_claim", s("roles")),
                        ("admin_roles", list(vec![s("admin")])),
                        ("required", Value::from(true)),
                    ]),
                ),
                (
                    "service_tokens",
                    list(vec![map(vec![
                        ("name", s("lok")),
                        ("token", s(&mesh.service_token)),
                    ])]),
                ),
            ]),
        ),
        ("dns", map(vec![("magic_dns_suffix", s(&mesh.dns_suffix))])),
        ("logging", map(vec![("level", s("info"))])),
    ])
}

/// The account frontend's landing page: what it is called.
pub fn build_landing(coord: &CoordBlock) -> serde_json::Value {
    serde_json::json!({ "name": coord.name })
}

/// `secrets/access.json` for a coordination server: where it is, and how to administer it.
pub fn build_access(config: &HubConfig, lok: &LokBlock, coord: &CoordBlock) -> serde_json::Value {
    let origin = coord.origin.trim_end_matches('/');
    serde_json::json!({
        "version": 1,
        "kind": "coord",
        "url": origin,
        // What a hub is created with: `konstruktor hub create --server <this>`.
        "server": origin,
        "issuer": lok.issuer,
        "well_known": format!("{origin}/.well-known/fakts"),
        "admin": {
            "username": config.global_admin,
            "password": config.global_admin_password,
            "admin_site": format!("{origin}/lok/admin/"),
        },
        "organization": lok.organization.identifier,
        "organization_id": lok.organization.id,
        "mesh": coord.mesh.as_ref().map(|mesh| mesh.url()),
    })
}

/// The account frontend's compose service.
pub fn kontrol_service(coord: &CoordBlock) -> Value {
    map(vec![
        ("image", s(&coord.kontrol.image)),
        (
            "volumes",
            list(vec![s(&format!(
                "./{LANDING_FILE}:/usr/share/nginx/html/landing.json:ro"
            ))]),
        ),
        ("stop_grace_period", s("2s")),
    ])
}

/// The mesh control server's compose service. Its STUN port is published as it is, since it
/// is what nodes are told; control and relay go through the gateway.
pub fn ionskale_service(config: &HubConfig, coord: &CoordBlock, mesh: &IonskaleBlock) -> Value {
    let lok = config.running_lok().map(|lok| lok.host.as_str());
    let mut depends_on = vec![crate::generate::compose::after(
        DB_COMPOSE_SERVICE,
        "service_healthy",
    )];
    depends_on.push(crate::generate::compose::after(
        &config.gateway.host,
        "service_started",
    ));
    if let Some(lok) = lok {
        depends_on.push(crate::generate::compose::after(lok, "service_started"));
    }

    let mut volumes = vec![s(&format!(
        "./{IONSKALE_CONFIG_FILE}:/etc/ionscale/config.yaml:ro"
    ))];
    let mut entries = vec![
        ("image", s(&mesh.image)),
        (
            "command",
            list(vec![
                s("server"),
                s("--config"),
                s("/etc/ionscale/config.yaml"),
            ]),
        ),
        (
            "ports",
            list(vec![s(&format!("{0}:{0}/udp", mesh.stun_port))]),
        ),
        ("depends_on", map(depends_on)),
    ];
    // It fetches Lok's OpenID configuration at the coordination server's own address, which
    // a CA nobody else trusts may have signed: Go reads every file in SSL_CERT_DIR, so the
    // system's roots stay.
    if coord.extra_roots.is_some() {
        volumes.push(s(&format!(
            "./{TRUSTED_ROOTS_FILE}:/etc/ssl/certs/konstruktor-roots.pem:ro"
        )));
        entries.push((
            "environment",
            map(vec![("SSL_CERT_DIR", s("/etc/ssl/certs"))]),
        ));
    }
    entries.push(("volumes", list(volumes)));
    entries.push(("stop_grace_period", s("2s")));
    map(entries)
}

/// The container that runs Lok's mesh reconciliation every [`RECONCILE_EVERY_SECONDS`]: on
/// Lok's image, with Lok's config, running the job its image names.
pub fn reconciler_service(config: &HubConfig, lok: &LokBlock, job: &[String]) -> Value {
    let command = job
        .iter()
        .map(String::as_str)
        .chain(RECONCILE_ARGS.iter().copied())
        .map(shell_word)
        .collect::<Vec<_>>()
        .join(" ");
    let mut depends_on = crate::generate::compose::infrastructure(config, true, true, false);
    depends_on.push(crate::generate::compose::after(
        &lok.host,
        "service_started",
    ));
    map(vec![
        ("image", s(&lok.image)),
        (
            "entrypoint",
            list(vec![
                s("sh"),
                s("-c"),
                s(&format!(
                    "while :; do sleep {RECONCILE_EVERY_SECONDS}; {command} || true; done"
                )),
            ]),
        ),
        (
            "volumes",
            list(vec![s(&format!(
                "./configs/{}.yaml:/workspace/config.yaml",
                lok.host
            ))]),
        ),
        ("depends_on", map(depends_on)),
        ("stop_grace_period", s("1s")),
    ])
}

/// `word` quoted for `sh` where it needs to be.
fn shell_word(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:".contains(c))
    {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// How every long-running container of a coordination server is kept: brought back after a
/// crash, a daemon restart and a reboot — it is a server, and nobody's desktop decides when
/// it runs — and its logs rotated, so a year of requests does not fill the disk.
pub fn keep_running(services: &mut Value, one_shot: &[&str]) {
    let Some(services) = services.as_mapping_mut() else {
        return;
    };
    for (name, service) in services.iter_mut() {
        let Some(service) = service.as_mapping_mut() else {
            continue;
        };
        let name = name.as_str().unwrap_or_default();
        service.remove("deploy");
        service.insert(
            "restart".into(),
            s(if one_shot.contains(&name) {
                "on-failure"
            } else {
                "unless-stopped"
            }),
        );
        let mut options = Mapping::new();
        options.insert("max-size".into(), s("10m"));
        options.insert("max-file".into(), s("5"));
        service.insert(
            "logging".into(),
            map(vec![
                ("driver", s("json-file")),
                ("options", Value::Mapping(options)),
            ]),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_is_quoted_only_when_it_has_to_be() {
        assert_eq!(shell_word("reconcile_meshes"), "reconcile_meshes");
        assert_eq!(shell_word("--sidecars-only"), "--sidecars-only");
        assert_eq!(shell_word("it's"), r"'it'\''s'");
        assert_eq!(shell_word("a b"), "'a b'");
    }
}
