//! The coordination server a self-contained hub runs: Lok's config, its compose service,
//! and the access document that says how to reach what was just built.
//!
//! Everything a hub otherwise gets from a coordination server by being accepted is
//! written here instead, into Lok's own config, as what Lok starts out with: an account
//! and an organization to act in, the hub's manifest as a *preconfigured* hub that every
//! new organization is given, and redeem tokens an app trades for a client without anybody
//! approving anything. Lok's `migrate` applies them, in that order, as part of preparing
//! its database ([`crate::contract::prepare_databases`]) — before it is started, so a Lok
//! that answers has them.

use serde_norway::{Mapping, Value};

use crate::config::hub::{origin, scheme_of, HubConfig, LokBlock, DB_COMPOSE_SERVICE};
use crate::connect::manifest::{
    advertised_port, build_aliases, build_hub_request, internal_alias, HubManifest,
    HubManifestOptions, InstanceRequest, ServiceManifest,
};
use crate::generate::service::{list, map, s};

/// The manifest identifier Lok is advertised under, as upstream's generator names it.
pub const LOK_MANIFEST: &str = "live.arkitekt.lok";

/// The kommunity partner the hub's manifest is registered through. Lok has no other way
/// to be told about a hub without somebody accepting it.
const PARTNER: &str = "konstruktor";

/// The role the seeded account holds in its organization. One of the roles Lok creates
/// with every organization; a role it does not know fails the whole membership.
const SEEDED_ROLE: &str = "admin";

/// Where the access document is written. Under `secrets/`, so it is readable by its owner
/// alone like every other key file in the folder.
pub const ACCESS_FILE: &str = "secrets/access.json";

/// Lok's verification key as every service's authentikate reads it: the public half of the
/// signing key, inline. Not a key set to fetch: the issuer is a name, and the key is one
/// Konstruktor minted itself.
pub fn rsa_issuer(lok: &LokBlock) -> Value {
    map(vec![
        ("iss", s(&lok.issuer)),
        ("kid", s(&lok.key_id)),
        ("kind", s("rsa")),
        ("public_key", s(&lok.key_pair.public_key)),
    ])
}

/// The hub's own manifest, as Lok registers it on boot: every service at every advertised
/// address, the object store, and Lok itself.
///
/// Built by [`build_hub_request`], which is what a hub sends a coordination server it
/// asks to be accepted by — so a self-contained hub advertises exactly what any other
/// hub does: every service at each advertised address, as an absolute alias a client
/// outside the stack opens, and at the gateway's name on the stack's own network, as a
/// `docker` alias only a container in that network can.
pub fn preconfigured_hub(
    config: &HubConfig,
    lok: &LokBlock,
    said: &crate::contract::Said,
) -> HubManifest {
    let mut hub = build_hub_request(
        config,
        &HubManifestOptions {
            identifier: lok.hub_identifier.clone(),
            description: config.global_description.clone(),
            node_id: config.device_id.clone(),
            hosts: lok.advertised_hosts.clone(),
            reachable_hosts: Vec::new(),
            request_auth_key: false,
            mesh_alias: false,
            internal_host: Some(config.gateway.host.clone()),
            expiration_seconds: None,
            // What each service is registered with — its scopes, its roles — is what its
            // image said of it.
            described: said.clone(),
        },
    )
    .hub;

    // No instance keys. A hub sends them so the coordination server can publish its trust
    // bundle; here the bundle is written inline into every service's config, so Lok has
    // nothing to vouch for. And Lok does more with a key than publish it: it hands it to
    // clients as the instance's *challenge key*, which makes them demand a signed answer
    // from the alias's health check — one no service gives, so every alias would fail.
    for instance in &mut hub.instances {
        instance.manifest.challenge_key = None;
        mark_in_network(&mut instance.aliases);
    }

    let ssl = config.gateway.ssl;
    let mut aliases = build_aliases(
        &lok.advertised_hosts,
        advertised_port(config),
        ssl,
        &lok.host,
        &[],
    );
    aliases.push(internal_alias(&config.gateway.host, ssl, &lok.host));
    mark_in_network(&mut aliases);

    hub.instances.push(InstanceRequest {
        identifier: "Lok".to_string(),
        description: Some("Users, organizations and permissions".to_string()),
        manifest: ServiceManifest {
            identifier: LOK_MANIFEST.to_string(),
            version: "1.0.0".to_string(),
            description: Some("Users, organizations and permissions".to_string()),
            logo: None,
            roles: Vec::new(),
            scopes: Vec::new(),
            node_id: config.device_id.clone(),
            instance_id: "default".to_string(),
            public_sources: Vec::new(),
            // Lok is the issuer, not an instance of the trust bundle it publishes.
            challenge_key: None,
        },
        aliases,
    });
    hub
}

/// `configs/lok.yaml`.
///
/// Lok's settings model ignores keys it does not know, so a misspelt one is not an error —
/// it is silently the default. Every key here is one `lok_server/configuration.py` reads.
pub fn build_lok_config(config: &HubConfig, lok: &LokBlock, said: &crate::contract::Said) -> Value {
    let csrf = config
        .csrf_trusted_origins
        .clone()
        .unwrap_or_else(|| vec!["http://localhost".into(), "https://localhost".into()]);

    // The owner is the seeded account: an organization without one falls back to
    // whichever superuser exists, and there is none on a first boot.
    let owner = lok
        .users
        .first()
        .map(|u| u.username.as_str())
        .unwrap_or_default();
    let organization = &lok.organization;

    let hub = serde_norway::to_value(preconfigured_hub(config, lok, said))
        .expect("a hub manifest is plain data");

    map(vec![
        (
            "django",
            map(vec![
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
                        ("username", s(&config.global_admin)),
                    ]),
                ),
                // Lok refuses OAuth requests over plain HTTP unless told the deployment
                // runs without TLS on purpose — which a gateway serving `http://` does.
                ("allow_insecure_transport", Value::from(!config.gateway.ssl)),
                (
                    "csrf_trusted_origins",
                    list(csrf.iter().map(|o| s(o)).collect()),
                ),
                ("debug", Value::from(false)),
                ("force_script_name", s(&lok.host)),
                ("hosts", list(vec![s("*")])),
                ("secret_key", s(&lok.secret_key)),
                // The gateway is the one proxy in front of it: without this every request
                // shares the gateway's address, and one rate limit.
                ("trusted_proxy_depth", Value::from(1)),
            ]),
        ),
        (
            "postgres",
            map(vec![
                ("db_name", s(&lok.db)),
                ("engine", s("django.db.backends.postgresql")),
                ("host", s(DB_COMPOSE_SERVICE)),
                ("password", s(&config.db.postgres_password)),
                ("port", Value::from(5432)),
                ("username", s(&config.db.postgres_user)),
            ]),
        ),
        (
            "redis",
            map(vec![
                ("host", s(&config.local_redis.host)),
                ("port", Value::from(config.local_redis.internal_port)),
            ]),
        ),
        (
            "authentikate",
            map(vec![
                ("audience", s("*")),
                ("issuers", list(vec![rsa_issuer(lok)])),
                ("static_tokens", Value::Mapping(Mapping::new())),
            ]),
        ),
        (
            "datalayer",
            map(vec![
                ("access_key", s(&config.minio.access_key)),
                ("secret_key", s(&config.minio.secret_key)),
                ("host", s(&config.minio.host)),
                ("port", Value::from(config.minio.internal_port)),
                ("protocol", s("http")),
                (
                    "media",
                    map(vec![("bucket", s(&lok.media_bucket.bucket_name))]),
                ),
            ]),
        ),
        ("deployment", map(vec![("name", s(&lok.hub_identifier))])),
        (
            "lok",
            map(vec![
                ("key_id", s(&lok.key_id)),
                ("public_key", s(&lok.key_pair.public_key)),
            ]),
        ),
        ("private_key", s(&lok.key_pair.private_key)),
        // The `iss` of its tokens, and with the next key nothing more: a name.
        ("oidc_issuer", s(&lok.issuer)),
        // Advertise the login endpoints at the address each request arrived at rather
        // than at one fixed address. A self-contained hub is reached at several — the
        // published port from the host, another machine's view of this one, the
        // gateway's name from a container beside it — and owns all of them. A Lok that
        // does not know this key ignores it, and then advertises nonsense built on the
        // name above: `ready::wait` checks for exactly that.
        ("discovery_follows_request", Value::from(true)),
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
                        map(pairs)
                    })
                    .collect(),
            ),
        ),
        (
            "organizations",
            list(vec![map(vec![
                ("name", s(&organization.name)),
                ("identifier", s(&organization.identifier)),
                ("owner", s(owner)),
                // What registers the preconfigured hub below in this organization.
                ("auto_configure", Value::from(true)),
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
                            ("organization", s(&organization.identifier)),
                            ("roles", list(vec![s(SEEDED_ROLE)])),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "redeem_tokens",
            list(
                lok.redeem_tokens
                    .iter()
                    .map(|token| {
                        map(vec![
                            ("token", s(&token.token)),
                            ("user", s(&token.user)),
                            ("organization", s(&organization.identifier)),
                            ("hub", s(&lok.hub_identifier)),
                            // Both default to a limit, and both are spelt out as none: the
                            // tokens are this deployment's own, in a file only its owner
                            // reads, and an app that restarts redeems again.
                            ("expires_in_days", Value::Null),
                            ("max_redemptions", Value::Null),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "kommunity_partners",
            list(vec![map(vec![
                ("identifier", s(PARTNER)),
                ("name", s("This hub")),
                ("partner_kind", s("preauthorized")),
                ("auto_configure", Value::from(true)),
                ("preconfigured_hub", hub),
            ])]),
        ),
    ])
}

/// Lok's compose service. A service like the others — same command, same dependencies —
/// and deliberately not built by `compose_service`: it has no `ServiceBlock`, and no
/// checkout to mount.
pub fn lok_compose_service(
    config: &HubConfig,
    lok: &LokBlock,
    said: &crate::contract::Said,
) -> Value {
    let mut entries = vec![("image", s(&lok.image))];
    // Started as its image says, like any service.
    if let Some(command) = said.get(&lok.host).and_then(|said| said.command(false)) {
        entries.push((
            "command",
            list(command.iter().map(|part| s(part)).collect()),
        ));
    }
    entries.extend(vec![
        // Like any service that uses all three: after a healthy database, and after the
        // job that creates its bucket has finished.
        (
            "depends_on",
            map(crate::generate::compose::infrastructure(
                config, true, true, true,
            )),
        ),
        ("stop_grace_period", s("2s")),
        (
            "volumes",
            list(vec![s(&format!(
                "./configs/{}.yaml:/workspace/config.yaml",
                lok.host
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
    map(entries)
}

/// `StagingAlias.kind` for an address that only exists on the stack's own docker network.
/// Clients are handed it like any other; one running in that network tries it first.
pub const DOCKER_ALIAS_KIND: &str = "docker";

/// Turns the in-network alias [`internal_alias`] builds into a `docker` one.
///
/// Here rather than in `internal_alias`, which also writes the manifest a hub sends to a
/// coordination server somebody else runs — and what kinds *that* server knows is not
/// this module's to assume.
fn mark_in_network(aliases: &mut [crate::connect::manifest::StagingAlias]) {
    for alias in aliases.iter_mut().filter(|alias| alias.id == "internal") {
        alias.kind = DOCKER_ALIAS_KIND.to_string();
    }
}

/// Where the gateway answers from this machine: `localhost` when the hub advertises it,
/// the first advertised address otherwise, at the published port. One of several
/// addresses that work — the hub can be logged into at any it answers on — and the one
/// a script on this machine should use.
pub fn gateway_url(config: &HubConfig, lok: &LokBlock) -> String {
    let hosts = &lok.advertised_hosts;
    let host = hosts
        .iter()
        .find(|h| h.host == "localhost")
        .or(hosts.first())
        .map(|h| h.host.as_str())
        .unwrap_or("localhost");
    origin(scheme_of(config), host, advertised_port(config))
}

/// `secrets/access.json`: how to reach the hub that was just built, for a program.
///
/// A self-contained hub is made to be driven — by a test suite, a script, a CI job — and
/// none of those can read a person's summary off a terminal. This is that summary as
/// data: where the gateway is, where an app points its `FAKTS_URL`, the account and the
/// organization it acts in, and the redeem tokens it can trade for a client. Derived
/// from the profile and nothing else, so it is rewritten with every other generated file
/// and can never say something the stack does not.
pub fn build_access(config: &HubConfig, lok: &LokBlock) -> serde_json::Value {
    let gateway = gateway_url(config, lok);
    let scheme = scheme_of(config);
    let port = advertised_port(config);

    let mut services = serde_json::Map::new();
    let mut entry = |name: &str, host: &str, identifier: String, health: &str| {
        services.insert(
            name.to_string(),
            serde_json::json!({
                "identifier": identifier,
                "url": format!("{gateway}/{host}"),
                "health_url": crate::health::health_url(scheme, port, host, health),
            }),
        );
    };
    entry(
        &lok.host,
        &lok.host,
        LOK_MANIFEST.to_string(),
        crate::health::HEALTH_PATH,
    );
    // Each under what its image says it is registered as: a service that was never asked
    // has nothing an app could ask for it by.
    for id in config.enabled_services() {
        let block = config.service(id);
        if let Some(identifier) = &block.identifier {
            entry(
                id.as_str(),
                &block.host,
                identifier.clone(),
                block.health_path(),
            );
        }
    }

    serde_json::json!({
        "version": 1,
        "gateway_url": gateway,
        // What an app is pointed at. Its well-known is served from the gateway's root.
        "fakts_url": gateway,
        "issuer": lok.issuer,
        "hub": lok.hub_identifier,
        "organization": lok.organization.identifier,
        "admin": {
            "username": config.global_admin,
            "password": config.global_admin_password,
        },
        "users": lok.users.iter().map(|user| serde_json::json!({
            "username": user.username,
            "password": user.password,
        })).collect::<Vec<_>>(),
        "redeem_tokens": lok.redeem_tokens.iter().map(|t| t.token.clone()).collect::<Vec<_>>(),
        "services": services,
    })
}
