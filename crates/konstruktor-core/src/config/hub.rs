use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::catalog::{ServiceId, SERVICE_IDS};
use crate::config::mesh::{build_mesh_block, MeshBlock, MeshOptions};
use crate::connect::manifest::AdvertisedHost;
use crate::hosts::HostCategory;
use crate::secrets::{
    generate_alpha_numeric_string, generate_django_secret_key, generate_name, KeyPair,
};

/// The `hub_config.yaml` Konstruktor writes: what a hub *is* — its services, its
/// infrastructure, and every secret it minted.
///
/// A hub runs data and compute services and trusts a remote coordination server for
/// identity, so by default there is no `lok`, no users and no organizations. The one
/// exception is a *self-contained* hub (`coord_server: local`), which runs its own — see
/// [`LokBlock`].
///
/// The services are a map, by name ([`HubConfig::services`]): which services there are is
/// not something this file's shape decides. What each needs — buckets, a key, secrets — is
/// what its image said when the hub was created or the service added, written into its
/// block so that every later generation reads it back instead of asking again.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalBucket {
    pub kind: String,
    pub bucket_name: String,
}

impl LocalBucket {
    fn new(name: &str) -> Self {
        Self {
            kind: "local".into(),
            bucket_name: name.into(),
        }
    }
}

/// The database every service has unless its image says otherwise, by the name it asks
/// for it under.
pub const MAIN_DATABASE: &str = "main";

/// How long a Postgres name may be: `NAMEDATALEN` - 1, in bytes. A longer one is cut off
/// silently, which would let two names become one.
pub const POSTGRES_NAME_LENGTH: usize = 63;

/// Whether `name` is one Postgres takes as it is written: a lower-case letter, then
/// lower-case letters, digits and underscores. Nothing that would have to be quoted.
pub fn plain_identifier(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Why a database a service asked for cannot be provided.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidDatabase {
    #[error(
        "`{service}` asks for a database called `{name}`, which is not a name Postgres takes \
         unquoted: a lower-case letter, then lower-case letters, digits and underscores"
    )]
    Name { service: String, name: String },
    #[error("`{service}` asks for the database `{name}` twice")]
    Twice { service: String, name: String },
    #[error(
        "`{service}`'s database `{name}` would be called `{database}` here, which is longer \
         than the {POSTGRES_NAME_LENGTH} bytes Postgres keeps of a name"
    )]
    TooLong {
        service: String,
        name: String,
        database: String,
    },
    #[error(
        "`{service}`'s database `{name}` would be called `{database}` here, and that is \
         already a database of `{other}`"
    )]
    Taken {
        service: String,
        name: String,
        database: String,
        other: String,
    },
}

/// What the hub calls the database a service asked for under `name`: `<service>_<name>`
/// (`mikro` + `main` → `mikro_main`). A service's name is itself a plain identifier
/// ([`crate::catalog::ServiceId::parse`]), so the two are joined as they are.
///
/// **This is the one place a database name is made, and it only ever makes a plain
/// identifier of at most [`POSTGRES_NAME_LENGTH`] bytes.** That is what makes it safe to
/// write the name unquoted wherever a database is created: `ensure_database`'s `CREATE
/// DATABASE` and `CREATE ROLE` ([`crate::services`]), and the database image's own init
/// script, which splices each entry of `POSTGRES_MULTIPLE_DATABASES` into SQL as it
/// stands. A name is checked again here even though a service built on
/// `arkitekt-service` cannot print a bad one: any image can describe itself.
pub fn database_name(service: &str, name: &str) -> Result<String, InvalidDatabase> {
    if !plain_identifier(name) {
        return Err(InvalidDatabase::Name {
            service: service.to_string(),
            name: name.to_string(),
        });
    }
    let database = format!("{service}_{name}");
    // A service that got its name past `ServiceId::parse` cannot fail this; one read out
    // of a profile somebody edited can.
    if !plain_identifier(&database) {
        return Err(InvalidDatabase::Name {
            service: service.to_string(),
            name: name.to_string(),
        });
    }
    if database.len() > POSTGRES_NAME_LENGTH {
        return Err(InvalidDatabase::TooLong {
            service: service.to_string(),
            name: name.to_string(),
            database,
        });
    }
    Ok(database)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kinded {
    pub kind: String,
}

impl Kinded {
    fn local() -> Self {
        Self {
            kind: "local".into(),
        }
    }
    fn global() -> Self {
        Self {
            kind: "global".into(),
        }
    }
}

/// The buckets a service stores into, by purpose (`media` → `mikromedia`), in the order
/// its image declared them.
///
/// The order is kept because it is the order the buckets are created in and their routes
/// appear in the Caddyfile, and a map sorted by key would reshuffle both. A purpose is the
/// service's own word, so any name is one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Buckets(Vec<(String, LocalBucket)>);

impl Buckets {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, purpose: &str) -> Option<&LocalBucket> {
        self.0
            .iter()
            .find(|(held, _)| held == purpose)
            .map(|(_, bucket)| bucket)
    }

    /// Declares a bucket for `purpose`, after the ones there are. One that is declared
    /// already keeps its bucket: its name is where the service's objects are.
    pub fn declare(&mut self, purpose: &str, bucket_name: &str) -> bool {
        if self.get(purpose).is_some() {
            return false;
        }
        self.0
            .push((purpose.to_string(), LocalBucket::new(bucket_name)));
        true
    }

    /// Every bucket, as `(purpose, bucket name)`, in declaration order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(purpose, bucket)| (purpose.as_str(), bucket.bucket_name.as_str()))
    }

    /// The bucket names, in declaration order.
    pub fn names(&self) -> Vec<String> {
        self.iter().map(|(_, name)| name.to_string()).collect()
    }
}

impl Serialize for Buckets {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.0.iter().map(|(purpose, bucket)| (purpose, bucket)))
    }
}

impl<'de> Deserialize<'de> for Buckets {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct InOrder;
        impl<'de> serde::de::Visitor<'de> for InOrder {
            type Value = Buckets;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a map from a purpose to its bucket")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Buckets, A::Error> {
                let mut out = Vec::new();
                while let Some(entry) = map.next_entry::<String, LocalBucket>()? {
                    out.push(entry);
                }
                Ok(Buckets(out))
            }
        }
        deserializer.deserialize_map(InOrder)
    }
}

/// Where a service that runs from source gets it. See [`ServiceBlock::runs_from`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunsFrom<'a> {
    /// A repository, cloned into `mounts/<service>`: at `revision` when the image says
    /// which commit it was built from and nobody asked for a branch.
    Repository {
        repository: &'a str,
        revision: Option<&'a str>,
    },
    /// A folder on this machine, mounted where it is: never cloned into, never written to.
    Folder(&'a str),
}

/// Whether a source somebody named is a folder on this machine rather than a repository
/// to clone: an absolute path. A repository is a URL or `user@host:path`, neither of which
/// starts that way.
pub fn is_local_folder(source: &str) -> bool {
    std::path::Path::new(source).is_absolute()
}

/// One service of a hub: where it runs, on which image, and everything the hub provides
/// it with.
///
/// The second half of the fields is what the service's image asked for when it was asked
/// what it is ([`HubConfig::provide`]): databases, buckets, a key, secrets, and whether it
/// is wired to the Redis and the operator account at all. They are written down here,
/// rather than read off the description each time, because they are *minted* — a bucket
/// holds a service's objects and a key is vouched for by the coordination server — and
/// because a hub's files are regenerated in places where no image can be asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceBlock {
    /// The operator account the service is told of. Absent for a service whose image says
    /// it has no use for one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin_config: Option<Kinded>,
    pub allowed_hosts: Vec<String>,
    pub auth_config: Kinded,
    /// The service's own databases in the hub's Postgres: the name it asked for each
    /// under (`main`) to what the hub calls it (`mikro_main`, see [`database_name`]).
    /// Empty for a service that keeps nothing in Postgres, and until its image is asked.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub databases: BTreeMap<String, String>,
    pub debug: bool,
    pub enabled: bool,
    /// Where the code in the service's image came from and where it sits in it, as the
    /// image says ([`crate::contract::Source`]): what it takes to run the service from a
    /// checkout. Absent for an image that does not say, and until it is asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<crate::contract::Source>,
    /// The source this service runs from when somebody named one, in place of what its
    /// image says: a repository to clone, or a folder on this machine that is used where
    /// it is. See [`Self::runs_from`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_override: Option<String>,
    pub host: String,
    /// What the service is registered as at the coordination server, and listed under in
    /// the hub's trust bundle (`live.arkitekt.mikro`): its image's own word for it. Absent
    /// until the image has been asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    /// Where the service answers for its health, under its own path, when its image names
    /// another place than the convention. See [`Self::health_path`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<String>,
    /// A service of the catalogue always has one. A service outside it has none until it
    /// is given the image it was named by, and a block without one runs nothing: see
    /// [`Self::runs`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    pub internal_port: u16,
    /// Run the service from its source instead of the code in its image: see
    /// [`Self::runs_from`] for which source that is.
    pub mount_github: bool,
    pub path_config: Kinded,
    /// The hub's Redis. Absent for a service whose image says it does not use it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redis_config: Option<Kinded>,
    pub secret_key: String,

    /// The buckets the service declared, by purpose. A service with any is handed the
    /// object store's credentials with them; one with none is told nothing of it.
    #[serde(default, skip_serializing_if = "Buckets::is_empty")]
    pub buckets: Buckets,
    /// This instance's Ed25519 key, for a service that asked for one: the only secret it
    /// holds for talking to the hub's other services. The private half goes into its
    /// config; the public half into the hub manifest (`challenge_key`), and the
    /// coordination server vouches for it from there. Rekuest's also signs its provenance
    /// tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_key_pair: Option<KeyPair>,
    /// The secrets the service declared, by name, each written to
    /// `secrets/<host>.<name>` and mounted read-only. Minted once and kept: Kuvert's
    /// `fernet` encrypts its mailbox credentials, and a new key would make every linked
    /// mailbox unreadable.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secrets: BTreeMap<String, String>,

    // What this build wires for one service in particular, by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ollama_config: Option<Kinded>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ensured_repositories: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_issuer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_kid: Option<String>,
    /// Taken out of the hub after it ran (`konstruktor hub services remove`). Its database
    /// and buckets stay provisioned — in the database init list and the bucket manifest —
    /// so its data is neither lost nor silently orphaned, and adding it back picks the
    /// same data, keys and secrets up again. Cleared when it is added back.
    #[serde(default, skip_serializing_if = "is_false")]
    pub retained: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl ServiceBlock {
    /// Whether this service is part of the stack: switched on, and with an image to run.
    ///
    /// Not just `enabled`: a service outside the catalogue has no image until it is given
    /// one, and a switch without one runs nothing.
    pub fn runs(&self) -> bool {
        self.enabled && self.image.is_some()
    }

    /// Where the service answers a health check, under its own path: what its image
    /// offers, or the convention every service has kept so far.
    pub fn health_path(&self) -> &str {
        self.health.as_deref().unwrap_or(crate::health::HEALTH_PATH)
    }

    /// The source a service that runs from source runs from, if anything says: what
    /// somebody named for it, else what its image says it was built from.
    pub fn runs_from(&self) -> Option<RunsFrom<'_>> {
        match self.source_override.as_deref() {
            Some(named) if is_local_folder(named) => Some(RunsFrom::Folder(named)),
            Some(named) => Some(RunsFrom::Repository {
                repository: named,
                // The revision is of the repository the image was built from, not of one
                // somebody pointed at instead.
                revision: None,
            }),
            None => self.source.as_ref().map(|source| RunsFrom::Repository {
                repository: &source.repository,
                revision: source.revision.as_deref(),
            }),
        }
    }

    /// The repository the service's code is in, where one is known: for a bug report, the
    /// hub's manifest, the dashboard. Not a folder on this machine.
    pub fn repository(&self) -> Option<&str> {
        match self.runs_from() {
            Some(RunsFrom::Repository { repository, .. }) => Some(repository),
            _ => self
                .source
                .as_ref()
                .map(|source| source.repository.as_str()),
        }
    }

    /// Where a checkout is mounted in the service's container: where its image says its
    /// code sits, or the place every service has kept it so far.
    pub fn source_path(&self) -> &str {
        self.source
            .as_ref()
            .map(|source| source.path.as_str())
            .unwrap_or(crate::contract::WORKSPACE)
    }

    /// What the hub calls the database the service asked for under `name`, if it did.
    pub fn database(&self, name: &str) -> Option<&str> {
        self.databases.get(name).map(String::as_str)
    }

    /// What the hub calls each of the service's databases, in the order of the names the
    /// service knows them by.
    pub fn database_names(&self) -> Vec<String> {
        self.databases.values().cloned().collect()
    }

    /// Whether the service is handed the object store: exactly when it declared storage.
    pub fn uses_datalayer(&self) -> bool {
        !self.buckets.is_empty()
    }

    /// Takes in what the service's image says it needs, minting what is missing and
    /// keeping everything there is; true when the block changed.
    ///
    /// Idempotent, and never narrowing what holds data or trust: a database, a bucket, a
    /// key and a secret, once there, stay — a release that stops declaring one does not
    /// make the hub forget where the rows and the objects are or which key the
    /// coordination server vouches for. What is merely wiring (the Redis, the operator
    /// account) follows the description both ways.
    ///
    /// A database the description names badly is not provided: whoever takes a
    /// description in refuses it first, saying why
    /// ([`HubConfig::databases_can_be_provided`]).
    pub fn provide(&mut self, said: &crate::contract::Description) -> bool {
        let before = self.clone();
        let needs = &said.needs;

        self.identifier = Some(said.identifier.clone());
        self.source = said.source.clone();
        self.health = Some(said.health_path())
            .filter(|path| *path != crate::health::HEALTH_PATH)
            .map(str::to_string);
        for name in &needs.databases {
            if let Ok(database) = database_name(&self.host, name) {
                self.databases.entry(name.clone()).or_insert(database);
            }
        }
        self.redis_config = needs.redis.then(Kinded::local);
        self.admin_config = needs.admin.then(Kinded::global);

        for purpose in &needs.storage {
            self.buckets
                .declare(purpose, &format!("{}{purpose}", self.host));
        }
        if needs.instance_key && self.instance_key_pair.is_none() {
            self.instance_key_pair = Some(crate::secrets::generate_ed25519_key_pair());
        }
        for name in &needs.secrets {
            self.secrets
                .entry(name.clone())
                .or_insert_with(crate::secrets::generate_fernet_key);
        }
        *self != before
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayBlock {
    pub auto_https: bool,
    pub enabled: bool,
    pub exposed_http_port: Option<u16>,
    pub exposed_https_port: Option<u16>,
    pub host: String,
    pub image: String,
    pub internal_port: u16,
    pub ssl: bool,
    pub ssl_cert: Option<String>,
}

/// The key `generate::compose` writes the database under in `services:`.
///
/// Deliberately not `db.host` — that is `daten`, the hostname the services connect to,
/// while the compose service itself has always been called `db`. Anything joining images
/// to containers has to use this one.
pub const DB_COMPOSE_SERVICE: &str = "db";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbBlock {
    pub enabled: bool,
    pub github_repo: String,
    pub host: String,
    pub image: String,
    pub mount: Option<String>,
    pub postgres_password: String,
    pub postgres_user: String,
    pub volume_name: String,
}

/// The object storage. RustFS since `jhnnsrs/init` 2.0.0, which provisions it through `rc`
/// and cannot provision MinIO any more; the block keeps its `minio` name because it is
/// the profile key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinioBlock {
    pub access_key: String,
    pub console_port: u16,
    pub enabled: bool,
    pub exposed_console_port: Option<u16>,
    pub host: String,
    pub image: String,
    pub init_container_host: String,
    pub init_container_image: String,
    pub internal_port: u16,
    pub mount: Option<String>,
    pub root_password: String,
    pub root_user: String,
    pub secret_key: String,
    pub volume_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedisBlock {
    pub enabled: bool,
    pub host: String,
    pub image: String,
    pub internal_port: u16,
}

/// Where Alpaka's language models come from.
///
/// Present only when somebody answered the question. Alpaka needs a model provider and
/// upstream's generator supplies none — `ollama_config: {kind: local}` names a provider
/// that nothing starts and no generated config points at. This block is what makes that
/// answer real, either by adding a container to the stack or by naming one that already
/// exists.
///
/// Like [`MeshBlock`] it is omitted entirely rather than written disabled: upstream's
/// model has no key for it, and there a present-but-unknown key is a hard failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OllamaBlock {
    /// Run the container in this stack. False means [`Self::url`] points somewhere else
    /// and nothing is added to the compose file.
    pub enabled: bool,
    /// What Alpaka talks to. Derived from the host and port when we run it ourselves.
    pub url: String,
    /// The compose service name, when we run it.
    pub host: String,
    pub image: String,
    pub internal_port: u16,
    /// Pulled models are gigabytes and worth keeping across a `down`, so they live in a
    /// named volume rather than in the deployment folder.
    pub volume_name: String,
}

impl OllamaBlock {
    /// A container in this stack, reached over the internal network by service name.
    pub fn local() -> Self {
        let (host, port) = ("ollama", 11434);
        Self {
            enabled: true,
            url: format!("http://{host}:{port}"),
            host: host.into(),
            image: "ollama/ollama:latest".into(),
            internal_port: port,
            volume_name: "ollama_models".into(),
        }
    }

    /// One that already exists. A bare host is taken as plain HTTP, which is what an
    /// Ollama on another machine on the same network almost always is.
    pub fn remote(url: &str) -> Self {
        let trimmed = url.trim().trim_end_matches('/');
        let url = if trimmed.contains("://") {
            trimmed.to_string()
        } else {
            format!("http://{trimmed}")
        };
        Self {
            enabled: false,
            url,
            ..Self::local()
        }
    }
}

/// The LiveKit media server Lovekit hands rooms out on.
///
/// Present once Lovekit has run on this hub — minted with it by
/// [`HubConfig::provide`] and kept when Lovekit is taken out, like a service's
/// Fernet key, so adding it back does not change the credentials. The container runs only
/// while Lovekit does ([`HubConfig::running_livekit`]).
///
/// Omitted entirely when absent: upstream's model has no key for it.
///
/// **Networking.** Signalling (HTTP and WebSocket) goes through the gateway, which listens on
/// [`Self::signal_port`] for it — LiveKit's clients append `/rtc` to the URL they are given,
/// and a dedicated port is what every LiveKit SDK handles, where a path prefix is not. The
/// media itself cannot be proxied: it flows between the client and LiveKit directly, over
/// ICE on [`Self::rtc_tcp_port`] and [`Self::rtc_udp_port`], which are published as they
/// are (LiveKit announces the port it listens on, so the two must match). The address it
/// announces is [`Self::node_ip`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LivekitBlock {
    /// The compose service name, which is also how Lovekit reaches its API.
    pub host: String,
    pub image: String,
    /// The key LiveKit knows Lovekit by. Not secret on its own, but redacted all the same.
    pub api_key: String,
    /// What Lovekit signals its access tokens with. At least 32 characters: LiveKit refuses
    /// a shorter one.
    pub api_secret: String,
    /// Where the gateway serves LiveKit's signalling, on this machine and on the mesh.
    pub signal_port: u16,
    /// ICE over TCP, the fallback where UDP is blocked. Published as is.
    pub rtc_tcp_port: u16,
    /// ICE over UDP, every stream multiplexed on one port. Published as is.
    pub rtc_udp_port: u16,
    /// The address LiveKit announces in its ICE candidates: the first private IPv4 address
    /// the hub advertises, set when it is authorized. Without one LiveKit announces its
    /// container's address, which only this machine (on Linux) can reach.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_ip: Option<String>,
}

/// LiveKit 1.13.7. Pinned, not `latest`: it is not ours, and a media server's wire protocol
/// is worth moving on purpose.
pub const LIVEKIT_IMAGE: &str = "livekit/livekit-server:v1.13.7";

/// The port LiveKit itself listens on for signalling, inside its container.
pub const LIVEKIT_INTERNAL_PORT: u16 = 7880;

impl LivekitBlock {
    /// Fresh credentials, on the ports that follow the gateway's (see [`crate::defaults`]).
    pub fn minted() -> Self {
        Self {
            host: "livekit".into(),
            image: LIVEKIT_IMAGE.into(),
            api_key: format!("API{}", generate_alpha_numeric_string(12)),
            api_secret: generate_alpha_numeric_string(48),
            signal_port: crate::defaults::LIVEKIT_SIGNAL_PORT,
            rtc_tcp_port: crate::defaults::LIVEKIT_RTC_TCP_PORT,
            rtc_udp_port: crate::defaults::LIVEKIT_RTC_UDP_PORT,
            node_ip: None,
        }
    }

    /// The URL Lovekit calls LiveKit's API on, inside the stack's network.
    pub fn api_url(&self) -> String {
        format!("http://{}:{LIVEKIT_INTERNAL_PORT}", self.host)
    }
}

/// The address LiveKit should announce, of the ones a hub advertises: the first private
/// IPv4 literal, else the first other routable IPv4 literal. Names are no use — ICE
/// candidates carry addresses — and neither are loopback, link-local or tailnet (CGNAT)
/// addresses, since the media ports are published on this machine's own interfaces.
pub fn livekit_node_ip(hosts: &[String]) -> Option<String> {
    let v4: Vec<std::net::Ipv4Addr> = hosts
        .iter()
        .filter_map(|h| h.trim().parse::<std::net::Ipv4Addr>().ok())
        .filter(|ip| {
            !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_unspecified()
                && !crate::hosts::is_cgnat(&ip.to_string())
        })
        .collect();
    v4.iter()
        .find(|ip| ip.is_private())
        .or_else(|| v4.first())
        .map(|ip| ip.to_string())
}

/// The image the `reporter` container runs: Konstruktor's CLI, in a container, published
/// to Docker Hub by every release. `latest` rather than a version, so a stack written by a
/// build that was never released still has an image to pull.
pub const REPORTER_IMAGE: &str = "jhnnsrs/reporter:latest";

/// The container that reports the hub's health to its coordination server — see
/// `crate::hubhealth`. Present once the hub has been authorized, since it logs in as
/// the client that authorization created.
///
/// Like [`MeshBlock`] it is omitted entirely rather than written disabled: upstream's
/// model has no key for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReporterBlock {
    pub enabled: bool,
    /// The compose service name.
    pub host: String,
    pub image: String,
    /// Where the rotating refresh token is kept, so a restart does not fall back to the
    /// stale one in `hub_credentials.json`.
    pub volume_name: String,
}

impl Default for ReporterBlock {
    fn default() -> Self {
        Self {
            enabled: true,
            host: "reporter".into(),
            image: REPORTER_IMAGE.into(),
            volume_name: "reporter_state".into(),
        }
    }
}

/// What `coord_server` says on a hub that runs its own coordination server, the way
/// `rekuest_server: local` says it runs its own Rekuest.
pub const LOCAL_COORD_SERVER: &str = "local";

/// Lok, the coordination server. Follows `latest` like every other image here.
pub const LOK_IMAGE: &str = "jhnnsrs/lok:latest";

/// An account Lok creates on boot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LokUser {
    pub username: String,
    pub password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// The organization Lok creates on boot, and registers this hub in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LokOrganization {
    /// The slug: what a redeem token and a membership name the organization by.
    pub identifier: String,
    pub name: String,
}

/// A token an app trades for a client of its own, without anybody accepting anything.
///
/// One token serves one app: Lok pins it to the first manifest it is redeemed with, and
/// refuses a different one afterwards. A deployment that several apps connect to
/// unattended therefore needs one per app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LokRedeemToken {
    pub token: String,
    /// The account the redeeming app acts as.
    pub user: String,
}

/// The coordination server, when this hub runs its own — a *self-contained* hub.
///
/// Everywhere else a hub is a claimant: it presents a manifest to a coordination server
/// somebody else runs and waits to be accepted. Here Konstruktor is the root of trust
/// itself, which is what makes the whole thing work unattended: it mints Lok's signing
/// key, writes the public half into every service's config, and writes the hub's own
/// manifest into Lok's config as something Lok registers on boot. Nobody is asked, and
/// nothing leaves the machine.
///
/// Like [`MeshBlock`] it is omitted entirely on a hub that has none: upstream's model has
/// no key for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LokBlock {
    pub enabled: bool,
    /// The compose service name, and the path it is routed under on the gateway.
    pub host: String,
    pub image: String,
    pub internal_port: u16,
    pub db: String,
    pub media_bucket: LocalBucket,
    pub secret_key: String,
    /// The RSA pair Lok signs with. The public half is what every service verifies
    /// against, inline — a service cannot fetch a key set from `localhost`.
    pub key_pair: KeyPair,
    /// The `kid` in the header of every token, and in each service's issuer entry.
    pub key_id: String,
    /// The `iss` of every token Lok mints, which every service matches by string
    /// equality. A name, not an address: Lok is told to advertise its endpoints at
    /// whatever address a request arrived at, so a token got at `localhost`, at a LAN
    /// address or at the gateway's name on the stack's own network is the same token.
    pub issuer: String,
    /// The hub's name inside [`Self::organization`].
    pub hub_identifier: String,
    /// The addresses the hub's services are advertised at. Kept here because there is no
    /// grant to keep them in.
    pub advertised_hosts: Vec<AdvertisedHost>,
    pub organization: LokOrganization,
    pub users: Vec<LokUser>,
    pub redeem_tokens: Vec<LokRedeemToken>,
}

/// What a front end says about the coordination server a self-contained hub runs.
#[derive(Debug, Clone)]
pub struct LokOptions {
    pub hub_identifier: String,
    /// Where clients reach the hub's services.
    pub hosts: Vec<AdvertisedHost>,
    pub organization: String,
    pub user: String,
    /// Left out, a strong one is generated.
    pub user_password: Option<String>,
    /// The redeem tokens to provision, all of them for [`Self::user`].
    pub redeem_tokens: Vec<String>,
    /// Injected by the tests; generated fresh otherwise.
    pub key_pair: Option<KeyPair>,
}

impl Default for LokOptions {
    fn default() -> Self {
        Self {
            hub_identifier: "local".into(),
            hosts: vec![AdvertisedHost {
                host: "localhost".into(),
                kind: HostCategory::Loopback,
            }],
            organization: "demo".into(),
            user: "demo".into(),
            user_password: None,
            redeem_tokens: vec![crate::secrets::generate_redeem_token()],
            key_pair: None,
        }
    }
}

/// What a self-contained hub's coordination server calls itself in its tokens.
pub const LOK_ISSUER: &str = "lok";

/// `scheme://host[:port]`, the port left off where it is the scheme's default — the way a
/// browser spells an origin.
pub(crate) fn origin(scheme: &str, host: &str, port: u16) -> String {
    let default = if scheme == "https" { 443 } else { 80 };
    if port == default {
        format!("{scheme}://{host}")
    } else {
        format!("{scheme}://{host}:{port}")
    }
}

fn build_lok_block(options: &LokOptions) -> LokBlock {
    LokBlock {
        enabled: true,
        host: "lok".into(),
        image: LOK_IMAGE.into(),
        internal_port: 80,
        // Lok is a service with a contract like any other: its database is called by the
        // same rule.
        db: database_name("lok", MAIN_DATABASE).expect("a plain name"),
        media_bucket: LocalBucket::new("lokmedia"),
        secret_key: generate_django_secret_key(),
        key_pair: options
            .key_pair
            .clone()
            .unwrap_or_else(crate::secrets::generate_rsa_key_pair),
        key_id: "lok-key-1".into(),
        issuer: LOK_ISSUER.into(),
        hub_identifier: options.hub_identifier.clone(),
        advertised_hosts: options.hosts.clone(),
        organization: LokOrganization {
            identifier: options.organization.clone(),
            name: options.organization.clone(),
        },
        users: vec![LokUser {
            username: options.user.clone(),
            password: blank(options.user_password.as_deref())
                .unwrap_or_else(|| generate_alpha_numeric_string(40)),
            email: None,
        }],
        redeem_tokens: options
            .redeem_tokens
            .iter()
            .map(|token| LokRedeemToken {
                token: token.clone(),
                user: options.user.clone(),
            })
            .collect(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubConfig {
    pub coord_server: String,
    pub csrf_trusted_origins: Option<Vec<String>>,
    pub db: DbBlock,
    pub default_service_grace_period_seconds: u32,
    pub device_id: Option<String>,
    pub domain: Option<String>,
    pub gateway: GatewayBlock,
    pub global_admin: String,
    pub global_admin_email: Option<String>,
    pub global_admin_password: String,
    pub global_description: Option<String>,
    pub internal_network: String,
    /// Lovekit's media server, once Lovekit has run here. See [`LivekitBlock`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub livekit: Option<LivekitBlock>,
    pub local_redis: RedisBlock,
    /// Present only when the hub runs its own Ollama. See [`OllamaBlock`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_ollama: Option<OllamaBlock>,
    /// Present only on a self-contained hub. See [`LokBlock`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lok: Option<LokBlock>,
    /// Present only on a hub that joined a mesh.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh: Option<MeshBlock>,
    pub minio: MinioBlock,
    pub rekuest_server: String,
    /// The image takt runs, once something pinned it (a rollback, an advanced pin). Unset,
    /// it follows Rekuest's image — see [`Self::takt_image`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takt_image: Option<String>,
    /// Present once the hub is authorized. See [`ReporterBlock`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reporter: Option<ReporterBlock>,
    /// The hub's services, by name.
    ///
    /// **Every service of the catalogue has a block**, switched on or not: a service that
    /// was never chosen keeps the image and secret key it would start with, so adding it
    /// later starts from what the profile says. A profile that lacks one — written by a
    /// build whose catalogue was shorter — is given the seeded, disabled block as it is
    /// read. **A service outside the catalogue has a block from the moment it is added**
    /// and never before: nothing here can seed one for a name it has not been told.
    ///
    /// So [`Self::service`] cannot miss for a catalogue service or for anything
    /// [`Self::enabled_services`] returned; [`Self::get`] is for a name of unknown origin.
    #[serde(deserialize_with = "every_catalogue_service")]
    pub services: BTreeMap<ServiceId, ServiceBlock>,
}

/// Reads the `services` map, and completes it to the invariant [`HubConfig::services`]
/// states: a catalogue service the profile has no block for gets the seeded, disabled one.
fn every_catalogue_service<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<ServiceId, ServiceBlock>, D::Error> {
    let mut services = BTreeMap::<ServiceId, ServiceBlock>::deserialize(deserializer)?;
    for id in SERVICE_IDS {
        services.entry(*id).or_insert_with(|| {
            let mut block = build_service_block(*id, false);
            block.enabled = false;
            block
        });
    }
    Ok(services)
}

/// The port takt listens on inside the stack; its image's default.
pub const TAKT_INTERNAL_PORT: u16 = 8080;

/// The volume Rekuest and takt share, and nothing else mounts: takt's internal API (what
/// Rekuest asks of it) is a unix socket in it, so reaching the socket is being Rekuest.
pub const TAKT_SOCKET_VOLUME: &str = "takt_run";
/// Where that volume is mounted in both containers.
pub const TAKT_SOCKET_DIR: &str = "/run/takt";
/// The socket takt binds there (`TAKT_INTERNAL_BIND`) and Rekuest connects to
/// (`rekuest.takt_socket`).
pub const TAKT_SOCKET_PATH: &str = "/run/takt/internal.sock";

/// The image of takt that belongs to a Rekuest image: the same repository with `-takt`
/// appended, under the same tag. The two are released together under the same tags, and a
/// digest pins one image only, so it is dropped.
///
/// `jhnnsrs/rekuest:next@sha256:…` → `jhnnsrs/rekuest-takt:next`.
pub fn takt_image_for(rekuest_image: &str) -> String {
    let reference = rekuest_image.split('@').next().unwrap_or(rekuest_image);
    // A colon after the last slash separates the tag; one before it is a registry's port.
    let name_starts = reference.rfind('/').map_or(0, |slash| slash + 1);
    match reference[name_starts..].rfind(':') {
        Some(colon) => {
            let (repository, tag) = reference.split_at(name_starts + colon);
            format!("{repository}-takt{tag}")
        }
        None => format!("{reference}-takt"),
    }
}

impl HubConfig {
    /// The block of a service this hub has one for, or `None`: for a name that came from
    /// somewhere that does not promise it is one of this hub's.
    pub fn get(&self, id: ServiceId) -> Option<&ServiceBlock> {
        self.services.get(&id)
    }

    /// The block of one of this hub's services.
    ///
    /// For an id that is known to have one — a catalogue service, or one this hub's own
    /// lists returned (see [`Self::services`] for the invariant). Asking for any other is
    /// a bug in the caller, and panics saying which; [`Self::get`] is the asking form.
    pub fn service(&self, id: ServiceId) -> &ServiceBlock {
        self.services
            .get(&id)
            .unwrap_or_else(|| panic!("this hub has no service called `{id}`"))
    }

    /// The mutable half of [`Self::service`].
    pub fn service_mut(&mut self, id: ServiceId) -> &mut ServiceBlock {
        self.services
            .get_mut(&id)
            .unwrap_or_else(|| panic!("this hub has no service called `{id}`"))
    }

    /// Every service this hub has a block for, running or not, in generation order.
    pub fn service_ids(&self) -> Vec<ServiceId> {
        crate::catalog::in_generation_order(self.services.keys().copied())
    }

    /// The service that runs as the compose service `host`, if one of this hub's does.
    pub fn service_at(&self, host: &str) -> Option<ServiceId> {
        self.service_ids()
            .into_iter()
            .find(|id| self.service(*id).host == host)
    }

    /// Rekuest's block. It is the one service the rest of a hub is wired around — takt runs
    /// beside it, and it is the provenance authority — so it is asked for by name.
    pub fn rekuest(&self) -> &ServiceBlock {
        self.service(ServiceId::Rekuest)
    }

    /// The compose service of takt, when this hub runs a Rekuest of its own.
    ///
    /// takt is Rekuest's other half (its own image, the same `rekuest.yaml`): every agent
    /// socket and hook intake, every deadline, schedule and trigger, and the clock of the
    /// server's upkeep jobs. A Rekuest without it serves GraphQL and nothing else — no agent
    /// connects, nothing is assigned, and its health check fails.
    pub fn takt_host(&self) -> Option<String> {
        let rekuest = self.rekuest();
        rekuest.runs().then(|| format!("{}-takt", rekuest.host))
    }

    /// The image takt runs: the pinned one, else the one that belongs to Rekuest's.
    pub fn takt_image(&self) -> Option<String> {
        let rekuest = self.rekuest();
        let image = rekuest.image.as_deref().filter(|_| rekuest.runs())?;
        Some(
            self.takt_image
                .clone()
                .unwrap_or_else(|| takt_image_for(image)),
        )
    }

    /// Where the stack's own containers reach takt, with Rekuest's path: what the hooked
    /// services report to. Rekuest itself asks takt through [`TAKT_SOCKET_PATH`] and takes
    /// only the path from this; a Rekuest image from before the socket still uses it whole.
    pub fn takt_url(&self) -> Option<String> {
        self.takt_host()
            .map(|host| format!("http://{host}:{TAKT_INTERNAL_PORT}/{}", self.rekuest().host))
    }

    /// Provides every running service with what its image says it needs, from `said` (by
    /// compose service); true when anything was added.
    ///
    /// This is where a description becomes part of the hub: buckets are named, an instance
    /// key and the declared secrets are minted, and the block remembers them — see
    /// [`ServiceBlock::provide`], which keeps whatever is there. A service `said` does not
    /// hold is left as it is: nothing is assumed of an image that was not asked.
    ///
    /// What this build wires beside one service in particular is settled here too:
    /// Lovekit's LiveKit credentials are minted once Lovekit runs, and kept with the rest
    /// of its [`LivekitBlock`] when it is taken out again.
    pub fn provide(&mut self, said: &crate::contract::Said) -> bool {
        let mut changed = false;
        for id in self.enabled_services() {
            let block = self.service_mut(id);
            if let Some(description) = said.get(&block.host) {
                changed |= block.provide(description);
            }
        }
        if self.runs(ServiceId::Lovekit) && self.livekit.is_none() {
            self.livekit = Some(LivekitBlock::minted());
            changed = true;
        }
        changed
    }

    /// Whether every database `said` asks for can be provided here, and why not otherwise:
    /// a name Postgres would not take as written, one asked for twice, one too long once
    /// it carries its service's name, or one that would be another service's database
    /// (`a` asking for `b_main` and `a_b` asking for `main` both come out as `a_b_main`).
    /// Asked before [`Self::provide`], which refuses nothing.
    ///
    /// Against everything the hub's Postgres holds or is about to: every service's
    /// databases, running or kept, and the coordination server's.
    pub fn databases_can_be_provided(
        &self,
        said: &crate::contract::Said,
    ) -> Result<(), InvalidDatabase> {
        // What each database is, by what the hub calls it.
        let mut held: BTreeMap<String, String> = BTreeMap::new();
        if let Some(lok) = &self.lok {
            held.insert(lok.db.clone(), lok.host.clone());
        }
        for id in self.service_ids() {
            let block = self.service(id);
            for database in block.databases.values() {
                held.insert(database.clone(), block.host.clone());
            }
        }
        for id in self.enabled_services() {
            let block = self.service(id);
            let Some(description) = said.get(&block.host) else {
                continue;
            };
            let asked = &description.needs.databases;
            for (at, name) in asked.iter().enumerate() {
                if asked[..at].contains(name) {
                    return Err(InvalidDatabase::Twice {
                        service: block.host.clone(),
                        name: name.clone(),
                    });
                }
                let database = database_name(&block.host, name)?;
                // One it holds already keeps the name it has, whatever that is.
                if block.databases.contains_key(name) {
                    continue;
                }
                match held.get(&database) {
                    Some(other) if *other != block.host => {
                        return Err(InvalidDatabase::Taken {
                            service: block.host.clone(),
                            name: name.clone(),
                            database,
                            other: other.clone(),
                        });
                    }
                    _ => {
                        held.insert(database, block.host.clone());
                    }
                }
            }
        }
        Ok(())
    }

    /// Every database the stack provisions, as the hub calls it: of the services that run
    /// or keep their data, in generation order, then the coordination server's.
    pub fn provisioned_databases(&self) -> Vec<String> {
        self.provisioned_services()
            .into_iter()
            .flat_map(|id| self.service(id).database_names())
            .chain(self.running_lok().map(|lok| lok.db.clone()))
            .collect()
    }

    /// The services that are to run from source and have none to run from: their image
    /// does not say where its code came from, and nobody named a source for them. Asked
    /// once the images have answered ([`Self::provide`]), before anything is written.
    pub fn without_a_source(&self) -> Vec<String> {
        self.enabled_services()
            .into_iter()
            .map(|id| self.service(id))
            .filter(|block| block.mount_github && block.runs_from().is_none())
            .map(|block| block.host.clone())
            .collect()
    }

    /// Whether `id` is one of this hub's services and part of its stack.
    pub fn runs(&self, id: ServiceId) -> bool {
        self.get(id).is_some_and(ServiceBlock::runs)
    }

    /// The LiveKit this stack runs, if it runs one: only while Lovekit does, like
    /// [`Self::running_ollama`].
    pub fn running_livekit(&self) -> Option<&LivekitBlock> {
        self.livekit
            .as_ref()
            .filter(|_| self.runs(ServiceId::Lovekit))
    }

    /// Point LiveKit at the address clients should send media to, from the hosts this hub
    /// advertises (see [`livekit_node_ip`]). Left as it is when none of them is an address.
    pub fn place_livekit(&mut self, hosts: &[String]) {
        if let Some(livekit) = self.livekit.as_mut() {
            if let Some(ip) = livekit_node_ip(hosts) {
                livekit.node_ip = Some(ip);
            }
        }
    }

    /// The services whose database and buckets the stack provisions: the enabled ones,
    /// and the ones taken out that keep their data (see [`ServiceBlock::retained`]), in
    /// generation order.
    pub fn provisioned_services(&self) -> Vec<ServiceId> {
        self.service_ids()
            .into_iter()
            .filter(|id| {
                let block = self.service(*id);
                block.runs() || block.retained
            })
            .collect()
    }

    /// The coordination server this stack runs, if it runs one.
    pub fn running_lok(&self) -> Option<&LokBlock> {
        self.lok.as_ref().filter(|lok| lok.enabled)
    }

    /// The Ollama this stack runs, if it runs one. Only while Alpaka does: it is Alpaka's
    /// provider, and a hub that took Alpaka out keeps the block (and the models volume)
    /// for when it comes back, but not the container.
    pub fn running_ollama(&self) -> Option<&OllamaBlock> {
        self.local_ollama
            .as_ref()
            .filter(|o| o.enabled && self.service(ServiceId::Alpaka).enabled)
    }

    /// Switch a service on in an existing profile. Its block is reused as it stands — the
    /// instance key, Django secret and secrets it had before are what its data was written
    /// with — and only an image it never had is seeded, from the catalogue. Rekuest is
    /// decided by `rekuest_server`, so adding it makes this hub run its own, as the wizard
    /// does.
    ///
    /// A service outside the catalogue gets its block here, the first time it is added,
    /// and no image: that is [`Self::add_service_on`]'s to give it.
    pub fn add_service(&mut self, id: ServiceId) {
        if id == ServiceId::Rekuest {
            self.rekuest_server = "local".into();
        }
        let block = self
            .services
            .entry(id)
            .or_insert_with(|| build_service_block(id, false));
        block.enabled = true;
        block.retained = false;
        if block.image.is_none() {
            block.image = id.default_image().map(str::to_string);
        }
    }

    /// [`Self::add_service`], running `image`: how a service is added by the image it is,
    /// which is the only way one outside the catalogue can be.
    pub fn add_service_on(&mut self, id: ServiceId, image: &str) {
        self.add_service(id);
        self.service_mut(id).image = Some(image.trim().to_string());
    }

    /// Switch a service off in an existing profile, keeping its data provisioned — see
    /// [`ServiceBlock::retained`]. Taking Rekuest out leaves the hub without one
    /// (`rekuest_server: none`), not trusting some other. A name this hub has no service
    /// for is not something to switch off.
    pub fn remove_service(&mut self, id: ServiceId) {
        let Some(block) = self.services.get_mut(&id) else {
            return;
        };
        block.enabled = false;
        block.retained = true;
        if id == ServiceId::Rekuest {
            self.rekuest_server = "none".into();
        }
    }

    /// The services the stack runs, in the order the generator feeds them — the
    /// catalogue's in [`crate::catalog::HUB_SERVICE_ORDER`], then any other by name:
    /// enabled, with an image (see [`ServiceBlock::runs`]).
    pub fn enabled_services(&self) -> Vec<ServiceId> {
        self.service_ids()
            .into_iter()
            .filter(|id| self.service(*id).runs())
            .collect()
    }

    /// Every image the generated stack declares, keyed by the **compose service** that
    /// runs it — the arkitekt services first, then the infrastructure.
    ///
    /// The key has to be the name compose writes into `services:`, because that is what
    /// comes back on a container as `com.docker.compose.service` and what the dashboard
    /// joins on. That is *not* always the block's `host`: the database block's host is
    /// `daten`, while `generate::compose` writes it under the literal key `db`. The pairs
    /// below mirror `build_compose` key for key, and `stack_images_match_the_compose_file`
    /// in `tests/generate.rs` fails if the two ever drift apart.
    pub fn stack_images(&self) -> Vec<(String, String)> {
        let mut images: Vec<(String, String)> = self
            .enabled_services()
            .into_iter()
            .filter_map(|id| {
                let block = self.service(id);
                block
                    .image
                    .as_ref()
                    .map(|image| (block.host.clone(), image.clone()))
            })
            .collect();
        if let (Some(host), Some(image)) = (self.takt_host(), self.takt_image()) {
            images.push((host, image));
        }

        images.push((DB_COMPOSE_SERVICE.to_string(), self.db.image.clone()));
        images.push((
            self.local_redis.host.clone(),
            self.local_redis.image.clone(),
        ));
        images.push((self.minio.host.clone(), self.minio.image.clone()));
        images.push((
            self.minio.init_container_host.clone(),
            self.minio.init_container_image.clone(),
        ));
        images.push((self.gateway.host.clone(), self.gateway.image.clone()));

        if let Some(mesh) = self.mesh.as_ref().filter(|m| m.enabled) {
            images.push((mesh.host.clone(), mesh.image.clone()));
        }
        if let Some(ollama) = self.running_ollama() {
            images.push((ollama.host.clone(), ollama.image.clone()));
        }
        if let Some(livekit) = self.running_livekit() {
            images.push((livekit.host.clone(), livekit.image.clone()));
        }
        if let Some(lok) = self.running_lok() {
            images.push((lok.host.clone(), lok.image.clone()));
        }
        if let Some(reporter) = self.reporter.as_ref().filter(|r| r.enabled) {
            images.push((reporter.host.clone(), reporter.image.clone()));
        }
        images
    }

    /// Points one compose service at an image, by the same names [`Self::stack_images`]
    /// reports.
    ///
    /// The inverse of that method, and it has to stay its inverse: two paths write an
    /// image back into a profile — `rollback`, putting an older one back, and `update
    /// --infra`, advancing a pin — and a service missing from here would be reported as
    /// moved without being moved. `every_service_in_the_stack_can_be_written_back` in
    /// `rollback` walks `stack_images` through this and fails if one is unreachable.
    ///
    /// Written as a chain of comparisons rather than a lookup because the names are the
    /// blocks' own `host`s, and only the services are a map. A service that is switched on
    /// and has no image yet — one outside the catalogue, at the moment it is given the
    /// image it was named by — is reachable too.
    pub fn set_service_image(&mut self, service: &str, image: &str) {
        if service == DB_COMPOSE_SERVICE {
            self.db.image = image.to_string();
            return;
        }
        if service == self.gateway.host {
            self.gateway.image = image.to_string();
            return;
        }
        if service == self.local_redis.host {
            self.local_redis.image = image.to_string();
            return;
        }
        if service == self.minio.host {
            self.minio.image = image.to_string();
            return;
        }
        if service == self.minio.init_container_host {
            self.minio.init_container_image = image.to_string();
            return;
        }
        if let Some(mesh) = self.mesh.as_mut().filter(|m| m.host == service) {
            mesh.image = image.to_string();
            return;
        }
        if let Some(ollama) = self.local_ollama.as_mut().filter(|o| o.host == service) {
            ollama.image = image.to_string();
            return;
        }
        if let Some(lok) = self.lok.as_mut().filter(|l| l.host == service) {
            lok.image = image.to_string();
            return;
        }
        if let Some(reporter) = self.reporter.as_mut().filter(|r| r.host == service) {
            reporter.image = image.to_string();
            return;
        }
        if let Some(livekit) = self.livekit.as_mut().filter(|l| l.host == service) {
            livekit.image = image.to_string();
            return;
        }
        if self.takt_host().as_deref() == Some(service) {
            self.takt_image = Some(image.to_string());
            return;
        }
        if let Some(block) = self
            .services
            .values_mut()
            .find(|block| block.enabled && block.host == service)
        {
            block.image = Some(image.to_string());
        }
    }

    /// The compose services that are switched on and have no image to run: services
    /// outside the catalogue that were named without one.
    pub fn imageless_services(&self) -> Vec<String> {
        self.service_ids()
            .into_iter()
            .map(|id| self.service(id))
            .filter(|block| block.enabled && block.image.is_none())
            .map(|block| block.host.clone())
            .collect()
    }
}

/// Where a service image this build seeded would have to be for the files generated now to
/// be the ones it reads, if it is not there already.
///
/// A service is seeded on a **major** (`jhnnsrs/rekuest:6`), not on `latest`: within a
/// major a service reads the config it always read, so a hub follows the tag freely, and a
/// release that reads something else is a new major, which only arrives with a Konstruktor
/// that generates for it. That is the whole contract between the two, and the catalogue
/// ([`crate::catalog::Known::image`]) is where it is written down — raising a major there
/// goes with a new layout
/// ([`crate::migrate::CURRENT_LAYOUT`]), since older hubs then have to move.
///
/// Behind is the seeded repository at `latest` (what earlier Konstruktors seeded) or at an
/// older major, with or without a digest pinned beside it. Anything else was chosen by
/// somebody — an exact version, another repository, a local build — and is left alone.
pub fn caught_up_image(id: ServiceId, image: &str) -> Option<String> {
    let supported = id.default_image()?;
    let (repository, major) = supported.rsplit_once(':')?;
    let named = image.split('@').next().unwrap_or(image);
    let (found, tag) = named.rsplit_once(':')?;
    if found != repository {
        return None;
    }
    let behind = tag == "latest"
        || matches!(
            (tag.parse::<u32>(), major.parse::<u32>()),
            (Ok(tag), Ok(major)) if tag < major
        );
    behind.then(|| supported.to_string())
}

/// Whether `image` is the one this build generates for: the seeded repository on the
/// seeded major, or an exact version of that major.
pub fn is_supported_image(id: ServiceId, image: &str) -> bool {
    let Some((repository, major)) = id.default_image().and_then(|s| s.rsplit_once(':')) else {
        return true;
    };
    let named = image.split('@').next().unwrap_or(image);
    named
        .rsplit_once(':')
        .is_some_and(|(found, tag)| found == repository && tag.split('.').next() == Some(major))
}

/// The block a service starts with, before its image has been asked anything.
///
/// For a catalogue service that is the catalogue's image, switched on when the catalogue
/// pre-ticks it. For any other there is only the name: no image, switched off until
/// somebody adds it. Either way the conventions every
/// service is held to — it listens on port 80 and is served under its own name — and
/// nothing its image has to say first: no databases, no buckets, no key, no secrets. Those are [`ServiceBlock::provide`]'s, once the image has answered.
fn build_service_block(id: ServiceId, mount_github: bool) -> ServiceBlock {
    let known = id.known();
    let name = id.as_str();

    let mut block = ServiceBlock {
        admin_config: Some(Kinded::global()),
        allowed_hosts: vec!["*".to_string()],
        auth_config: Kinded::local(),
        databases: BTreeMap::new(),
        debug: false,
        enabled: known.is_some_and(|known| known.default),
        source: None,
        source_override: None,
        host: name.into(),
        identifier: None,
        health: None,
        image: known.map(|known| known.image.to_string()),
        internal_port: 80,
        // Whether there is a source to run it from is not known until its image is asked:
        // see `HubConfig::sources_are_known`.
        mount_github,
        path_config: Kinded::local(),
        redis_config: Some(Kinded::local()),
        secret_key: generate_django_secret_key(),
        buckets: Buckets::default(),
        instance_key_pair: None,
        secrets: BTreeMap::new(),
        ollama_config: None,
        ensured_repositories: None,
        provenance_issuer: None,
        provenance_kid: None,
        retained: false,
    };

    // What this build wires for one service in particular. A service it has never heard
    // of passes through untouched.
    match id {
        ServiceId::Rekuest => {
            block.provenance_issuer = Some("rekuest".into());
            block.provenance_kid = Some("rekuest-prov-1".into());
        }
        ServiceId::Kabinet => {
            block.ensured_repositories = Some(vec![
                "jhnnsrs/ome:main".into(),
                "jhnnsrs/renderer:main".into(),
            ]);
        }
        ServiceId::Alpaka => {
            block.ollama_config = Some(Kinded::local());
        }
        _ => {}
    }

    block
}

/// Where the database and the object storage keep their bytes.
///
/// The default is a named Docker volume for each, which lives inside the engine's own
/// VM on macOS and Windows and on the host filesystem on Linux — in every case the
/// fastest storage a container can get. A bind mount into the deployment folder goes
/// through the file-sharing layer on the desktop engines (gRPC-FUSE, virtiofs), which
/// is fine for a config file and very much not fine for Postgres or for a bucket of
/// images: the difference is easily an order of magnitude on writes.
///
/// `DeploymentFolder` is kept as the opt-out for the one thing a named volume is worse
/// at — the data being a folder you can see, move and copy by hand — and the front ends
/// say so before anyone picks it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StorageMode {
    /// Named volumes, managed by the engine. `mount` is left empty on both blocks.
    #[default]
    DockerVolumes,
    /// Bind mounts at `./db_data` and `./rustfs_data` inside the deployment folder.
    DeploymentFolder,
}

impl StorageMode {
    /// Whether the data lives in the engine's own volumes, rather than in a folder.
    pub fn uses_volumes(self) -> bool {
        matches!(self, StorageMode::DockerVolumes)
    }
}

// --- what the infrastructure runs ----------------------------------------------------
//
// Every image follows `latest`. The services have always floated on a channel tag, because
// there is no version field in the profile and the set of tags is the only statement of
// what a hub follows; the infrastructure used to be pinned by digest instead, and now
// follows the same channel.
//
// That puts the one dangerous move — a Postgres major changing under a running hub, which
// leaves a cluster the new binary refuses to open — on `updates::guard`, which reads
// `PG_MAJOR` off the pulled image before an update recreates the database. `jhnnsrs/daten`
// keeps `PGDATA` at `/var/lib/postgresql/data` across majors, so the mount stays put.
//
// These are defaults, so they are what new hubs get. Every generator path afterwards
// reads `config.<service>.image` back out of the stored profile.

/// Postgres with multiple-database support (Postgres 19 at the time of writing).
pub const DB_IMAGE: &str = "jhnnsrs/daten:latest";
/// Caddy 2.11.4.
pub const GATEWAY_IMAGE: &str = "caddy:2.11.4";
/// Redis 8.10.1.
pub const REDIS_IMAGE: &str = "redis:8.10.1";
/// The S3 server. See [`MinioBlock`].
pub const STORAGE_IMAGE: &str = "rustfs/rustfs:latest";
/// The run-once container that creates the buckets and the services' user in RustFS.
pub const STORAGE_INIT_IMAGE: &str = "jhnnsrs/init:latest";

/// The bind mount the database uses when the data lives in the deployment folder.
pub const DB_FOLDER_MOUNT: &str = "./db_data";
/// The bind mount the object storage uses when the data lives in the deployment folder.
pub const STORAGE_FOLDER_MOUNT: &str = "./rustfs_data";

/// Reads a profile back into a [`StorageMode`]: any bind mount on either block means the
/// data is in a folder, an empty `mount` on both means the volumes.
/// `https` when the gateway terminates TLS, `http` otherwise.
///
/// One helper because three places used to decide this independently — the dashboard's
/// gateway URL, the restore's health checks, and the reachability probe — and a hub that
/// disagreed with itself about its own scheme would be diagnosed as unreachable.
pub fn scheme_of(config: &HubConfig) -> &'static str {
    if config.gateway.ssl {
        "https"
    } else {
        "http"
    }
}

/// The origins Django accepts unsafe requests (form POSTs, the admin) from, beyond the one
/// a request names in its own `Host`: every address the hub is reached at, as the browser
/// spells it — `scheme://host[:port]`, the port left off only where it is the scheme's
/// default, because Django compares the whole thing.
///
/// `hosts` are the names and addresses the hub advertises on this machine's networks. To
/// those come `localhost`, the gateway's name on the hub's own network (plugin apps), and
/// the tailnet node's short name. The node's full MagicDNS name is the coordination
/// server's to assign and is not known here.
///
/// The gateway serves plain HTTP on its published HTTP port, and HTTPS only when the hub
/// terminates TLS — so `https://` origins are written only then.
pub fn trusted_origins(config: &HubConfig, hosts: &[String]) -> Vec<String> {
    let gateway = &config.gateway;

    let mut out: Vec<String> = Vec::new();
    let mut push = |value: String| {
        if !out.contains(&value) {
            out.push(value);
        }
    };

    // What the generator always wrote, so nothing that worked before stops working.
    push("http://localhost".into());
    push("https://localhost".into());

    let published: Vec<&str> = std::iter::once("localhost")
        .chain(hosts.iter().map(String::as_str).map(str::trim))
        .filter(|h| !h.is_empty())
        .collect();
    for host in &published {
        if let Some(port) = gateway.exposed_http_port {
            push(origin("http", host, port));
        }
        if gateway.ssl {
            if let Some(port) = gateway.exposed_https_port {
                push(origin("https", host, port));
            }
        }
    }

    // Inside the hub's network and on the tailnet nothing is mapped: the gateway's own port.
    let mut unmapped = vec![gateway.host.as_str()];
    if let Some(mesh) = config
        .mesh
        .as_ref()
        .filter(|m| m.enabled && !m.hostname.is_empty())
    {
        unmapped.push(mesh.hostname.as_str());
    }
    for host in unmapped {
        push(origin("http", host, 80));
        if gateway.ssl {
            push(origin("https", host, 443));
        }
    }

    out
}

pub fn storage_mode_of(config: &HubConfig) -> StorageMode {
    let bound = |mount: &Option<String>| mount.as_deref().is_some_and(|m| !m.is_empty());
    if bound(&config.db.mount) || bound(&config.minio.mount) {
        StorageMode::DeploymentFolder
    } else {
        StorageMode::DockerVolumes
    }
}

#[derive(Debug, Clone)]
pub struct HubConfigOptions {
    /// Stable per-machine id; the registry owns it.
    pub device_id: String,
    /// The coordination server this hub trusts for identity.
    pub coord_server: String,
    /// `"local"` runs Rekuest here; anything else points at a remote provenance authority.
    pub rekuest_server: String,
    /// Which services to switch on. Rekuest is decided by `rekuest_server`, not by this.
    pub services: Option<Vec<ServiceId>>,
    pub http_port: Option<u16>,
    pub https_port: Option<u16>,
    pub ssl: bool,
    pub domain: Option<String>,
    pub global_admin: String,
    pub global_admin_password: Option<String>,
    pub global_admin_email: Option<String>,
    pub global_description: Option<String>,
    pub csrf_trusted_origins: Option<Vec<String>>,
    /// Join a mesh. Left out, the hub gets no `mesh` block and no sidecar — the key is
    /// only known after the authorization, so this is filled in on a second pass.
    pub mesh: Option<MeshOptions>,
    /// The coordination server to run here, on a self-contained hub
    /// (`coord_server: local`). Left out there, the defaults of [`LokOptions`] apply;
    /// ignored on any other hub.
    pub lok: Option<LokOptions>,
    /// Injected by the tests; generated fresh otherwise.
    pub provenance_key_pair: Option<KeyPair>,
    /// A *dev hub*: **every** service's source is checked out on this machine and
    /// mounted over the image's workspace, so `mount_github` is set on each service
    /// block and the generated compose file carries the bind mounts. The branch is not
    /// part of the config — upstream's model has no key for it, and it is only needed at
    /// the moment the checkout happens.
    ///
    /// The CLI's `--dev` still means all of them. The wizard asks per service instead,
    /// through [`Self::service_options`]; the two are a union, never a conflict.
    pub dev_hub: bool,
    /// What was asked of one service in particular. Only the services that were given an
    /// answer appear — everything absent takes the deployment-wide default.
    pub service_options: BTreeMap<ServiceId, ServiceOptions>,
    /// Where the database and object storage live. See [`StorageMode`].
    pub storage: StorageMode,
}

/// What a front end can say about a single service, beyond whether it runs at all.
///
/// Everything here is something the person creating the hub has to decide *per service*
/// and that cannot be derived. Two of the fields apply to one service each, which is why
/// they are `Option` and why nothing complains when they are set on a service that has no
/// use for them — a front end that offers them elsewhere is the thing at fault, and
/// making the type enforce it would mean a variant per service.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceOptions {
    /// Check this service's repository out into `mounts/<service>` and mount it over the
    /// image's workspace. Needs git, which the caller is responsible for having found.
    #[serde(default)]
    pub from_source: bool,
    /// The branch to check out. Absent, the commit the image says it was built from is
    /// checked out, or — when it does not say — the repository's own default branch.
    #[serde(default)]
    pub branch: Option<String>,
    /// The source to run from in place of what the image says: a repository to clone, or
    /// an absolute path of a folder on this machine, which is used where it is. Absent,
    /// the repository the service's image names.
    #[serde(default)]
    pub source: Option<String>,
    /// Django's debug mode for this one service. It reaches the container: the generator
    /// already writes it as `django.debug` in `configs/<service>.yaml`.
    #[serde(default)]
    pub debug: bool,
    /// **Alpaka only.** Where its language models come from.
    #[serde(default)]
    pub ollama: Option<OllamaChoice>,
    /// **Kabinet only.** The app repositories this hub should offer, replacing the
    /// default pair. Absent leaves the default alone.
    #[serde(default)]
    pub repositories: Option<Vec<String>>,
}

/// Where Alpaka's models come from.
///
/// `run_locally` adds an Ollama container to the stack; otherwise `url` names one that
/// already exists. Both empty is the same as not answering, and leaves the profile saying
/// what it says today — a `local` provider that nothing starts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OllamaChoice {
    #[serde(default)]
    pub run_locally: bool,
    #[serde(default)]
    pub url: Option<String>,
}

impl Default for HubConfigOptions {
    fn default() -> Self {
        Self {
            device_id: String::new(),
            coord_server: String::new(),
            rekuest_server: "local".into(),
            services: None,
            http_port: Some(crate::defaults::HTTP_PORT),
            https_port: Some(crate::defaults::HTTPS_PORT),
            ssl: false,
            domain: None,
            global_admin: "admin".into(),
            global_admin_password: None,
            global_admin_email: None,
            global_description: None,
            csrf_trusted_origins: None,
            mesh: None,
            lok: None,
            provenance_key_pair: None,
            dev_hub: false,
            service_options: BTreeMap::new(),
            storage: StorageMode::default(),
        }
    }
}

/// The wizard hands back empty strings for questions that were skipped; upstream stores
/// those as null, and a `domain: ""` would end up in every generated URL.
fn blank(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// Build a complete hub profile.
///
/// `rekuest.enabled` deliberately ignores the service selection and follows
/// `rekuest_server` instead: a hub that trusts a remote Rekuest must not start a second
/// one.
///
/// Every service of the catalogue gets a block, and so does every selected service that
/// is not in it — without an image, which whoever named it has to give it
/// ([`HubConfig::set_service_image`]). No image has been asked anything yet, so no block
/// holds buckets, a key or secrets: [`HubConfig::provide`] adds those from what the
/// images say.
pub fn build_hub_config(options: &HubConfigOptions) -> HubConfig {
    let selected = options.services.as_deref().unwrap_or_default();
    let mut services: BTreeMap<ServiceId, ServiceBlock> = SERVICE_IDS
        .iter()
        .chain(selected)
        .map(|id| (*id, build_service_block(*id, options.dev_hub)))
        .collect();

    if options.services.is_some() {
        for (id, block) in services.iter_mut() {
            block.enabled = selected.contains(id);
        }
    }

    // A dev hub mounts every service's source; asking for one service on its own mounts
    // that one. Both write the same field, so the generator and `git::checkouts` never
    // have to know which of the two answers put it there.
    for (id, block) in services.iter_mut() {
        if let Some(asked) = options.service_options.get(id) {
            block.mount_github = block.mount_github || asked.from_source;
            if let Some(source) = blank(asked.source.as_deref()) {
                block.source_override = Some(source);
            }
            block.debug = block.debug || asked.debug;

            // Kabinet's app repositories: an answer replaces the seeded pair outright
            // rather than adding to it, because "these are the apps this hub offers" is
            // the question, not "these as well".
            if let Some(repositories) = &asked.repositories {
                block.ensured_repositories = Some(repositories.clone());
            }

            // Alpaka's provider. `local` means one runs in this stack, `global` means it
            // is somewhere else.
            if let Some(ollama) = &asked.ollama {
                if ollama.run_locally {
                    block.ollama_config = Some(Kinded::local());
                } else if blank(ollama.url.as_deref()).is_some() {
                    block.ollama_config = Some(Kinded::global());
                }
            }
        }
    }

    let rekuest = services
        .get_mut(&ServiceId::Rekuest)
        .expect("every catalogue service is seeded");
    rekuest.enabled = options.rekuest_server.trim() == "local";
    // Rekuest's instance key is also its provenance key; a caller may pin it (tests).
    // Otherwise it is minted with every other service's, once the images are asked.
    rekuest.instance_key_pair = options.provenance_key_pair.clone();

    let mut config = HubConfig {
        services,
        // Minted once Lovekit is known to run: see `HubConfig::provide`.
        livekit: None,

        coord_server: options.coord_server.clone(),
        csrf_trusted_origins: options.csrf_trusted_origins.clone(),
        db: DbBlock {
            enabled: true,
            github_repo: "https://github.com/arkitektio/daten-server".into(),
            host: "daten".into(),
            image: DB_IMAGE.into(),
            // A named volume by default — see `StorageMode` for why the bind mount
            // into the folder is the opt-out rather than the rule. Either way erasing
            // the data is a separate, confirmed act: `destroy::purge_data`.
            mount: (!options.storage.uses_volumes()).then(|| DB_FOLDER_MOUNT.into()),
            postgres_password: generate_alpha_numeric_string(40),
            postgres_user: generate_name(),
            volume_name: "db_data".into(),
        },
        default_service_grace_period_seconds: 2,
        device_id: Some(options.device_id.clone()),
        domain: blank(options.domain.as_deref()),
        gateway: GatewayBlock {
            auto_https: true,
            enabled: true,
            exposed_http_port: options.http_port,
            exposed_https_port: options.https_port,
            host: "gateway".into(),
            image: GATEWAY_IMAGE.into(),
            internal_port: 80,
            ssl: options.ssl,
            ssl_cert: None,
        },
        global_admin: options.global_admin.clone(),
        global_admin_email: blank(options.global_admin_email.as_deref()),
        // JavaScript's `||` falls through on the empty string, not just on null — so a
        // password of "" regenerates rather than being written blank.
        global_admin_password: blank(options.global_admin_password.as_deref())
            .unwrap_or_else(|| generate_alpha_numeric_string(40)),
        global_description: blank(options.global_description.as_deref()),
        internal_network: generate_name(),
        local_ollama: None,
        lok: None,
        local_redis: RedisBlock {
            enabled: true,
            host: "redis".into(),
            image: REDIS_IMAGE.into(),
            internal_port: 6379,
        },
        mesh: options.mesh.as_ref().map(build_mesh_block),
        // Added when the hub is authorized; a profile that never was has nothing to log
        // in as.
        reporter: None,
        takt_image: None,
        minio: MinioBlock {
            access_key: generate_alpha_numeric_string(40),
            console_port: 9001,
            enabled: true,
            exposed_console_port: None,
            host: "rustfs".into(),
            image: STORAGE_IMAGE.into(),
            // The dashboard recognises a run-once container by its `_init` suffix, where
            // "exited" is success rather than a failure — see `isInitContainer`.
            init_container_host: "rustfs_init".into(),
            init_container_image: STORAGE_INIT_IMAGE.into(),
            internal_port: 9000,
            // Upstream's default is the container-absolute `/data`, which docker turns
            // into an *anonymous* volume — nothing to find again after `down`. Ours is
            // the named volume, or the folder beside the database when that was asked.
            mount: (!options.storage.uses_volumes()).then(|| STORAGE_FOLDER_MOUNT.into()),
            root_password: generate_alpha_numeric_string(40),
            root_user: generate_name(),
            secret_key: generate_alpha_numeric_string(40),
            volume_name: "rustfs_data".into(),
        },
        rekuest_server: options.rekuest_server.clone(),
    };

    // Alpaka's provider, once the blocks are in place. Only when Alpaka actually runs:
    // an Ollama container for a service this hub does not have would be several
    // gigabytes pulled for nothing.
    if config.service(ServiceId::Alpaka).enabled {
        config.local_ollama = options
            .service_options
            .get(&ServiceId::Alpaka)
            .and_then(|asked| asked.ollama.as_ref())
            .and_then(|ollama| {
                if ollama.run_locally {
                    Some(OllamaBlock::local())
                } else {
                    blank(ollama.url.as_deref()).map(|url| OllamaBlock::remote(&url))
                }
            });
    }

    // The coordination server, on a hub that runs its own.
    if options.coord_server.trim() == LOCAL_COORD_SERVER {
        config.lok = Some(build_lok_block(&options.lok.clone().unwrap_or_default()));
    }

    config
}
