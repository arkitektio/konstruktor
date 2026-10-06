use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::catalog::{ServiceId, SERVICE_IDS};
use crate::config::mesh::{build_mesh_block, MeshBlock, MeshOptions};
use crate::connect::manifest::AdvertisedHost;
use crate::hosts::HostCategory;
use crate::secrets::{
    generate_alpha_numeric_string, generate_django_secret_key, generate_name, KeyPair,
};

/// The `hub_config.yaml` Konstruktor writes.
///
/// A faithful port of `arkitekt_next/server/config/hub.py` and the defaults it pulls in
/// from `config/infrastructure.py` and `services/*.py`.
///
/// A hub runs data and compute services and trusts a remote coordination server for
/// identity, so by default there is no `lok`, no users and no organizations. The one
/// exception is a *self-contained* hub (`coord_server: local`), which runs its own — see
/// [`LokBlock`].
///
/// **Every optional field below is `skip_serializing_if`.** Upstream's pydantic models
/// use `extra="forbid"`, so a key present-but-null is a hard failure where an absent key
/// is fine. This is the single easiest way to break a generated profile.

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalDb {
    pub kind: String,
    pub db: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceBlock {
    pub admin_config: Kinded,
    pub allowed_hosts: Vec<String>,
    pub auth_config: Kinded,
    pub db_config: LocalDb,
    pub debug: bool,
    pub enabled: bool,
    pub github_repo: String,
    pub host: String,
    /// Absent only on the Lovekit block of a profile written before Lovekit had an image
    /// (upstream still writes it so): such a block runs nothing. See [`Self::runs`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    pub internal_port: u16,
    pub media_bucket: LocalBucket,
    pub mount_github: bool,
    pub path_config: Kinded,
    pub redis_config: Kinded,
    pub secret_key: String,

    // Service-specific extras, present only on the services that declare them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zarr_bucket: Option<LocalBucket>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parquet_bucket: Option<LocalBucket>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bigfile_bucket: Option<LocalBucket>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fabriks_bucket: Option<LocalBucket>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub konnektion_bucket: Option<LocalBucket>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ollama_config: Option<Kinded>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ensured_repositories: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance_issuer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance_kid: Option<String>,
    /// Read from profiles written before instance keys; migrated into Rekuest's
    /// [`Self::instance_key_pair`] by [`HubConfig::ensure_instance_keys`], never written again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance_key_pair: Option<KeyPair>,
    /// This instance's Ed25519 key: the only secret it holds for talking to the hub's other
    /// services. The private half goes into its config; the public half into the hub
    /// manifest (`challenge_key`), and the coordination server vouches for it from there.
    /// Rekuest's also signs its provenance tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_key_pair: Option<KeyPair>,
    /// **Kuvert only.** The Fernet key its mailbox credentials are encrypted with, written
    /// to `secrets/<host>.fernet` and mounted read-only. Minted once and kept: a new key
    /// would make every linked mailbox unreadable. See [`HubConfig::ensure_service_secrets`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fernet_key: Option<String>,
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
    /// For `skip_serializing_if` on the services upstream does not know: one that is disabled
    /// and holds nothing worth keeping is left out of the profile, since the Python CLI's
    /// model forbids the key. A disabled Kuvert that already has its Fernet key is kept:
    /// dropping the key would make its mailboxes unreadable once it is switched back on.
    pub fn is_disposable(&self) -> bool {
        !self.enabled && self.fernet_key.is_none() && !self.retained
    }

    /// Whether this service is part of the stack: switched on, and with an image to run.
    ///
    /// Not just `enabled`: upstream seeds `lovekit: enabled: true` without an image, and
    /// every profile written before Lovekit had one says the same — a switch that never ran
    /// anything, and must not start running something now.
    pub fn runs(&self) -> bool {
        self.enabled && self.image.is_some()
    }

    /// The bucket declared for a purpose, if this service declares one.
    pub fn bucket(&self, purpose: &str) -> Option<&LocalBucket> {
        match purpose {
            "media" => Some(&self.media_bucket),
            "zarr" => self.zarr_bucket.as_ref(),
            "parquet" => self.parquet_bucket.as_ref(),
            "bigfile" => self.bigfile_bucket.as_ref(),
            "fabriks" => self.fabriks_bucket.as_ref(),
            "konnektion" => self.konnektion_bucket.as_ref(),
            _ => None,
        }
    }

    /// The bucket name for one of `id`'s purposes, falling back to `<service><purpose>`
    /// when the profile does not name one.
    ///
    /// The fallback is what keeps an older hub working: a profile written before a service
    /// gained a bucket has no entry for it, and a service whose settings dereference the
    /// bucket crashes on boot without it. The name matches what a new profile is seeded
    /// with, so regenerating an old hub gives the same stack a new one gets.
    pub fn bucket_name(&self, id: ServiceId, purpose: &str) -> String {
        self.bucket(purpose)
            .map(|b| b.bucket_name.clone())
            .unwrap_or_else(|| format!("{}{purpose}", id.as_str()))
    }

    /// Every bucket `id` declares, in `bucket_purposes()` order.
    pub fn bucket_names(&self, id: ServiceId) -> Vec<(&'static str, String)> {
        id.bucket_purposes()
            .iter()
            .map(|purpose| (*purpose, self.bucket_name(id, purpose)))
            .collect()
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
/// [`HubConfig::ensure_service_secrets`] and kept when Lovekit is taken out, like Kuvert's
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
        db: "lok".into(),
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
    pub alpaka: ServiceBlock,
    /// Experimental, and unknown upstream: defaulted to a disabled block when a profile has
    /// none, and written only while enabled — so older profiles load and a hub without it
    /// stays readable by the Python CLI.
    #[serde(
        default = "disabled_bank",
        skip_serializing_if = "ServiceBlock::is_disposable"
    )]
    pub bank: ServiceBlock,
    pub coord_server: String,
    pub csrf_trusted_origins: Option<Vec<String>>,
    pub db: DbBlock,
    pub default_service_grace_period_seconds: u32,
    pub device_id: Option<String>,
    pub domain: Option<String>,
    pub elektro: ServiceBlock,
    pub fluss: ServiceBlock,
    pub gateway: GatewayBlock,
    pub global_admin: String,
    pub global_admin_email: Option<String>,
    pub global_admin_password: String,
    pub global_description: Option<String>,
    pub internal_network: String,
    pub kabinet: ServiceBlock,
    pub kraph: ServiceBlock,
    /// Experimental, and unknown upstream. See [`Self::bank`].
    #[serde(
        default = "disabled_dokuments",
        skip_serializing_if = "ServiceBlock::is_disposable"
    )]
    pub dokuments: ServiceBlock,
    /// Experimental, and unknown upstream. See [`Self::bank`].
    #[serde(
        default = "disabled_lokate",
        skip_serializing_if = "ServiceBlock::is_disposable"
    )]
    pub lokate: ServiceBlock,
    /// Lovekit's media server, once Lovekit has run here. See [`LivekitBlock`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub livekit: Option<LivekitBlock>,
    /// Experimental, and unknown upstream. See [`Self::bank`].
    #[serde(
        default = "disabled_kuvert",
        skip_serializing_if = "ServiceBlock::is_disposable"
    )]
    pub kuvert: ServiceBlock,
    pub local_redis: RedisBlock,
    /// Present only when the hub runs its own Ollama. See [`OllamaBlock`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_ollama: Option<OllamaBlock>,
    /// Present only on a self-contained hub. See [`LokBlock`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lok: Option<LokBlock>,
    /// Present only on a hub that joined a mesh. Upstream's config model does not know
    /// this key, so it is omitted entirely rather than written as `enabled: false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh: Option<MeshBlock>,
    pub mikro: ServiceBlock,
    pub minio: MinioBlock,
    pub rekuest: ServiceBlock,
    pub rekuest_server: String,
    /// The image takt runs, once something pinned it (a rollback, an advanced pin). Unset,
    /// it follows Rekuest's image — see [`Self::takt_image`]. Unknown upstream, so it is
    /// written only when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takt_image: Option<String>,
    /// Present once the hub is authorized. See [`ReporterBlock`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reporter: Option<ReporterBlock>,
    pub lovekit: ServiceBlock,
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
    /// The compose service of takt, when this hub runs a Rekuest of its own.
    ///
    /// takt is Rekuest's other half (its own image, the same `rekuest.yaml`): every agent
    /// socket and hook intake, every deadline, schedule and trigger, and the clock of the
    /// server's upkeep jobs. A Rekuest without it serves GraphQL and nothing else — no agent
    /// connects, nothing is assigned, and its health check fails.
    pub fn takt_host(&self) -> Option<String> {
        self.rekuest
            .runs()
            .then(|| format!("{}-takt", self.rekuest.host))
    }

    /// The image takt runs: the pinned one, else the one that belongs to Rekuest's.
    pub fn takt_image(&self) -> Option<String> {
        let rekuest = self
            .rekuest
            .image
            .as_deref()
            .filter(|_| self.rekuest.runs())?;
        Some(
            self.takt_image
                .clone()
                .unwrap_or_else(|| takt_image_for(rekuest)),
        )
    }

    /// Where the stack's own containers reach takt, with Rekuest's path: what the hooked
    /// services report to. Rekuest itself asks takt through [`TAKT_SOCKET_PATH`] and takes
    /// only the path from this; a Rekuest image from before the socket still uses it whole.
    pub fn takt_url(&self) -> Option<String> {
        self.takt_host()
            .map(|host| format!("http://{host}:{TAKT_INTERNAL_PORT}/{}", self.rekuest.host))
    }

    pub fn service(&self, id: ServiceId) -> &ServiceBlock {
        match id {
            ServiceId::Rekuest => &self.rekuest,
            ServiceId::Mikro => &self.mikro,
            ServiceId::Fluss => &self.fluss,
            ServiceId::Kabinet => &self.kabinet,
            ServiceId::Kraph => &self.kraph,
            ServiceId::Elektro => &self.elektro,
            ServiceId::Alpaka => &self.alpaka,
            ServiceId::Lovekit => &self.lovekit,
            ServiceId::Bank => &self.bank,
            ServiceId::Kuvert => &self.kuvert,
            ServiceId::Dokuments => &self.dokuments,
            ServiceId::Lokate => &self.lokate,
        }
    }

    /// Give every service an instance key it does not have yet; true when any was added.
    ///
    /// Idempotent: a key, once minted, is kept — rotating it is re-authorizing with a new one
    /// on purpose, not a side effect of loading a profile. Rekuest's pre-instance-key
    /// provenance pair becomes its instance key, so its provenance tokens keep verifying.
    pub fn ensure_instance_keys(&mut self) -> bool {
        let mut changed = false;
        for id in crate::catalog::SERVICE_IDS {
            let block = self.service_mut(id);
            if block.instance_key_pair.is_none() {
                block.instance_key_pair = Some(
                    block
                        .provenance_key_pair
                        .take()
                        .unwrap_or_else(crate::secrets::generate_ed25519_key_pair),
                );
                changed = true;
            } else if block.provenance_key_pair.take().is_some() {
                changed = true;
            }
        }
        changed
    }

    /// Give an enabled Kuvert the Fernet key it encrypts mailbox credentials with, if it has
    /// none yet; true when one was added.
    ///
    /// Idempotent like [`Self::ensure_instance_keys`]: once minted the key is kept, because
    /// a new one cannot decrypt what the old one encrypted. Only an *enabled* Kuvert gets
    /// one, so a hub without it never writes a `kuvert` block. A Kuvert disabled later keeps
    /// its block and key in the profile (see [`ServiceBlock::is_disposable`]).
    ///
    /// Lovekit's LiveKit credentials are minted the same way, once Lovekit runs, and kept
    /// with the rest of its [`LivekitBlock`] when it is taken out again.
    pub fn ensure_service_secrets(&mut self) -> bool {
        let mut changed = false;
        let kuvert = &mut self.kuvert;
        if kuvert.enabled && kuvert.fernet_key.is_none() {
            kuvert.fernet_key = Some(crate::secrets::generate_fernet_key());
            changed = true;
        }
        if self.lovekit.runs() && self.livekit.is_none() {
            self.livekit = Some(LivekitBlock::minted());
            changed = true;
        }
        changed
    }

    /// The LiveKit this stack runs, if it runs one: only while Lovekit does, like
    /// [`Self::running_ollama`].
    pub fn running_livekit(&self) -> Option<&LivekitBlock> {
        self.livekit.as_ref().filter(|_| self.lovekit.runs())
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

    /// The mutable half of [`Self::service`], used by [`Self::set_service_image`].
    pub(crate) fn service_mut(&mut self, id: ServiceId) -> &mut ServiceBlock {
        match id {
            ServiceId::Rekuest => &mut self.rekuest,
            ServiceId::Mikro => &mut self.mikro,
            ServiceId::Fluss => &mut self.fluss,
            ServiceId::Kabinet => &mut self.kabinet,
            ServiceId::Kraph => &mut self.kraph,
            ServiceId::Elektro => &mut self.elektro,
            ServiceId::Alpaka => &mut self.alpaka,
            ServiceId::Lovekit => &mut self.lovekit,
            ServiceId::Bank => &mut self.bank,
            ServiceId::Kuvert => &mut self.kuvert,
            ServiceId::Dokuments => &mut self.dokuments,
            ServiceId::Lokate => &mut self.lokate,
        }
    }

    /// The services whose database and buckets the stack provisions: the enabled ones,
    /// and the ones taken out that keep their data (see [`ServiceBlock::retained`]), in
    /// generation order.
    pub fn provisioned_services(&self) -> Vec<ServiceId> {
        crate::catalog::HUB_SERVICE_ORDER
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
            .filter(|o| o.enabled && self.alpaka.enabled)
    }

    /// Switch a service on in an existing profile. Its block is reused as it stands — the
    /// instance key, Django secret and Kuvert's Fernet key it had before are what its data
    /// was written with — and only an image it never had is seeded. Rekuest is decided by
    /// `rekuest_server`, so adding it makes this hub run its own, as the wizard does.
    pub fn add_service(&mut self, id: ServiceId) {
        if id == ServiceId::Rekuest {
            self.rekuest_server = "local".into();
        }
        let block = self.service_mut(id);
        block.enabled = true;
        block.retained = false;
        if block.image.is_none() {
            block.image = seed(id).image.map(str::to_string);
        }
    }

    /// Switch a service off in an existing profile, keeping its data provisioned — see
    /// [`ServiceBlock::retained`]. Taking Rekuest out leaves the hub without one
    /// (`rekuest_server: none`), not trusting some other.
    pub fn remove_service(&mut self, id: ServiceId) {
        if id == ServiceId::Rekuest {
            self.rekuest_server = "none".into();
        }
        let block = self.service_mut(id);
        block.enabled = false;
        block.retained = true;
    }

    /// The services the stack runs, in the order the generator feeds them: enabled, with
    /// an image (see [`ServiceBlock::runs`]).
    pub fn enabled_services(&self) -> Vec<ServiceId> {
        crate::catalog::HUB_SERVICE_ORDER
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
    /// Written as a chain of comparisons rather than a lookup because the profile is a
    /// struct: there is no map to write into.
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
        for id in self.enabled_services() {
            if self.service(id).host == service {
                self.service_mut(id).image = Some(image.to_string());
                return;
            }
        }
    }
}

/// Where a service image this build seeded would have to be for the files generated now to
/// be the ones it reads, if it is not there already.
///
/// A service is seeded on a **major** (`jhnnsrs/rekuest:6`), not on `latest`: within a
/// major a service reads the config it always read, so a hub follows the tag freely, and a
/// release that reads something else is a new major, which only arrives with a Konstruktor
/// that generates for it. That is the whole contract between the two, and [`seed`] is
/// where it is written down — raising a major there goes with a new layout
/// ([`crate::migrate::CURRENT_LAYOUT`]), since older hubs then have to move.
///
/// Behind is the seeded repository at `latest` (what earlier Konstruktors seeded) or at an
/// older major, with or without a digest pinned beside it. Anything else was chosen by
/// somebody — an exact version, another repository, a local build — and is left alone.
pub fn caught_up_image(id: ServiceId, image: &str) -> Option<String> {
    let supported = seed(id).image?;
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
    let Some((repository, major)) = seed(id).image.and_then(|s| s.rsplit_once(':')) else {
        return true;
    };
    let named = image.split('@').next().unwrap_or(image);
    named
        .rsplit_once(':')
        .is_some_and(|(found, tag)| found == repository && tag.split('.').next() == Some(major))
}

/// Everything a service block needs beyond the shared defaults.
struct ServiceSeed {
    enabled: bool,
    image: Option<&'static str>,
    db: &'static str,
    github_repo: &'static str,
}

fn seed(id: ServiceId) -> ServiceSeed {
    match id {
        ServiceId::Rekuest => ServiceSeed {
            enabled: true,
            image: Some("jhnnsrs/rekuest:7"),
            db: "rekuest",
            github_repo: "https://github.com/arkitektio/rekuest-server-next",
        },
        ServiceId::Mikro => ServiceSeed {
            enabled: true,
            image: Some("jhnnsrs/mikro:7"),
            db: "mikro",
            github_repo: "https://github.com/arkitektio/mikro-server-next",
        },
        ServiceId::Fluss => ServiceSeed {
            enabled: true,
            image: Some("jhnnsrs/fluss:4"),
            db: "fluss",
            github_repo: "https://github.com/arkitektio/fluss-server-next",
        },
        ServiceId::Kabinet => ServiceSeed {
            enabled: true,
            image: Some("jhnnsrs/kabinet:6"),
            db: "kabinet",
            github_repo: "https://github.com/arkitektio/kabinet-server",
        },
        ServiceId::Kraph => ServiceSeed {
            enabled: true,
            image: Some("jhnnsrs/kraph:2"),
            db: "kraph",
            github_repo: "https://github.com/arkitektio/kraph-server",
        },
        ServiceId::Elektro => ServiceSeed {
            enabled: false,
            image: Some("jhnnsrs/elektro:5"),
            db: "elektro",
            github_repo: "https://github.com/arkitektio/elektro-server",
        },
        ServiceId::Alpaka => ServiceSeed {
            enabled: false,
            image: Some("jhnnsrs/alpaka:5"),
            db: "alpaka",
            github_repo: "https://github.com/arkitektio/alpaka-server",
        },
        // Experimental, like the ones below. Upstream still seeds it enabled and without
        // an image, which is what older profiles say; see `ServiceBlock::runs`.
        ServiceId::Lovekit => ServiceSeed {
            enabled: false,
            image: Some("jhnnsrs/lovekit:3"),
            db: "lovekit",
            github_repo: "https://github.com/arkitektio/lovekit-server",
        },
        // Experimental: offered, never switched on unless asked for.
        ServiceId::Bank => ServiceSeed {
            enabled: false,
            image: Some("jhnnsrs/bank:4"),
            db: "bank",
            github_repo: "https://github.com/jhnnsrs/bank",
        },
        ServiceId::Kuvert => ServiceSeed {
            enabled: false,
            image: Some("jhnnsrs/kuvert:4"),
            db: "kuvert",
            github_repo: "https://github.com/jhnnsrs/kuvert",
        },
        ServiceId::Dokuments => ServiceSeed {
            enabled: false,
            image: Some("jhnnsrs/dokuments:2"),
            db: "dokuments",
            github_repo: "https://github.com/jhnnsrs/dokuments-server",
        },
        ServiceId::Lokate => ServiceSeed {
            enabled: false,
            image: Some("jhnnsrs/lokate:3"),
            db: "lokate",
            github_repo: "https://github.com/arkitektio/lokate-server",
        },
    }
}

/// What a profile written before Bank existed reads as: the seeded, disabled block.
fn disabled_bank() -> ServiceBlock {
    build_service_block(ServiceId::Bank, false)
}

/// What a profile written before Kuvert existed reads as. See [`disabled_bank`].
fn disabled_kuvert() -> ServiceBlock {
    build_service_block(ServiceId::Kuvert, false)
}

/// What a profile written before Dokuments existed reads as. See [`disabled_bank`].
fn disabled_dokuments() -> ServiceBlock {
    build_service_block(ServiceId::Dokuments, false)
}

/// What a profile written before Lokate existed reads as. See [`disabled_bank`].
fn disabled_lokate() -> ServiceBlock {
    build_service_block(ServiceId::Lokate, false)
}

fn build_service_block(id: ServiceId, mount_github: bool) -> ServiceBlock {
    let seed = seed(id);
    let name = id.as_str();

    let mut block = ServiceBlock {
        admin_config: Kinded::global(),
        allowed_hosts: vec!["*".to_string()],
        auth_config: Kinded::local(),
        db_config: LocalDb {
            kind: "local".into(),
            db: seed.db.into(),
        },
        debug: false,
        enabled: seed.enabled,
        github_repo: seed.github_repo.into(),
        host: name.into(),
        image: seed.image.map(str::to_string),
        internal_port: 80,
        media_bucket: LocalBucket::new(&format!("{name}media")),
        mount_github,
        path_config: Kinded::local(),
        redis_config: Kinded::local(),
        secret_key: generate_django_secret_key(),
        zarr_bucket: None,
        parquet_bucket: None,
        bigfile_bucket: None,
        fabriks_bucket: None,
        konnektion_bucket: None,
        ollama_config: None,
        ensured_repositories: None,
        provenance_issuer: None,
        provenance_kid: None,
        provenance_key_pair: None,
        instance_key_pair: None,
        fernet_key: None,
        retained: false,
    };

    match id {
        ServiceId::Rekuest => {
            block.provenance_issuer = Some("rekuest".into());
            block.provenance_kid = Some("rekuest-prov-1".into());
        }
        ServiceId::Mikro => {
            block.zarr_bucket = Some(LocalBucket::new("mikrozarr"));
            block.parquet_bucket = Some(LocalBucket::new("mikroparquet"));
            block.bigfile_bucket = Some(LocalBucket::new("mikrobigfile"));
            block.fabriks_bucket = Some(LocalBucket::new("mikrofabriks"));
            block.konnektion_bucket = Some(LocalBucket::new("mikrokonnektion"));
        }
        ServiceId::Elektro => {
            block.zarr_bucket = Some(LocalBucket::new("elektrozarr"));
            block.parquet_bucket = Some(LocalBucket::new("elektroparquet"));
            block.bigfile_bucket = Some(LocalBucket::new("elektrobigfile"));
        }
        ServiceId::Kraph => {
            block.zarr_bucket = Some(LocalBucket::new("kraphzarr"));
            block.bigfile_bucket = Some(LocalBucket::new("kraphbigfile"));
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
        ServiceId::Bank => {
            block.bigfile_bucket = Some(LocalBucket::new("bankbigfile"));
        }
        // The Fernet key is minted by `ensure_service_secrets`, once Kuvert is enabled.
        ServiceId::Kuvert => {
            block.bigfile_bucket = Some(LocalBucket::new("kuvertbigfile"));
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
    /// The branch to check out. Absent, the repository's own default branch is used —
    /// they do not all agree on what it is called.
    #[serde(default)]
    pub branch: Option<String>,
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
/// `rekuest_server` instead — the Python CLI applies the same rule after its own picker,
/// and a hub that trusts a remote Rekuest must not start a second one.
pub fn build_hub_config(options: &HubConfigOptions) -> HubConfig {
    let mut blocks: Vec<(ServiceId, ServiceBlock)> = SERVICE_IDS
        .into_iter()
        .map(|id| (id, build_service_block(id, options.dev_hub)))
        .collect();

    if let Some(selected) = &options.services {
        for (id, block) in blocks.iter_mut() {
            block.enabled = selected.contains(id);
        }
    }

    // A dev hub mounts every service's source; asking for one service on its own mounts
    // that one. Both write the same field, so the generator and `git::checkouts` never
    // have to know which of the two answers put it there.
    for (id, block) in blocks.iter_mut() {
        if let Some(asked) = options.service_options.get(id) {
            block.mount_github = block.mount_github || asked.from_source;
            block.debug = block.debug || asked.debug;

            // Kabinet's app repositories: an answer replaces the seeded pair outright
            // rather than adding to it, because "these are the apps this hub offers" is
            // the question, not "these as well".
            if let Some(repositories) = &asked.repositories {
                block.ensured_repositories = Some(repositories.clone());
            }

            // Alpaka's provider. `local` means one runs in this stack, `global` means it
            // is somewhere else — the same two words upstream's model already uses.
            if let Some(ollama) = &asked.ollama {
                if ollama.run_locally {
                    block.ollama_config = Some(Kinded::local());
                } else if blank(ollama.url.as_deref()).is_some() {
                    block.ollama_config = Some(Kinded::global());
                }
            }
        }
    }

    let take = |blocks: &mut Vec<(ServiceId, ServiceBlock)>, id: ServiceId| {
        let index = blocks
            .iter()
            .position(|(i, _)| *i == id)
            .expect("every id is seeded");
        blocks.remove(index).1
    };

    let mut rekuest = take(&mut blocks, ServiceId::Rekuest);
    rekuest.enabled = options.rekuest_server.trim() == "local";
    // Rekuest's instance key is also its provenance key; a caller may pin it (tests).
    rekuest.instance_key_pair = Some(
        options
            .provenance_key_pair
            .clone()
            .unwrap_or_else(crate::secrets::generate_ed25519_key_pair),
    );

    let mut config = HubConfig {
        rekuest,
        mikro: take(&mut blocks, ServiceId::Mikro),
        fluss: take(&mut blocks, ServiceId::Fluss),
        kabinet: take(&mut blocks, ServiceId::Kabinet),
        kraph: take(&mut blocks, ServiceId::Kraph),
        elektro: take(&mut blocks, ServiceId::Elektro),
        alpaka: take(&mut blocks, ServiceId::Alpaka),
        lovekit: take(&mut blocks, ServiceId::Lovekit),
        bank: take(&mut blocks, ServiceId::Bank),
        kuvert: take(&mut blocks, ServiceId::Kuvert),
        dokuments: take(&mut blocks, ServiceId::Dokuments),
        lokate: take(&mut blocks, ServiceId::Lokate),
        // Minted below, once Lovekit is known to run.
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
    if config.alpaka.enabled {
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

    // Every service keeps the host it was seeded with; `service_mut` exists for the
    // orchestration that folds a mesh key in after the fact.
    let _ = config.service_mut(ServiceId::Rekuest);
    config.ensure_instance_keys();
    config.ensure_service_secrets();
    config
}
