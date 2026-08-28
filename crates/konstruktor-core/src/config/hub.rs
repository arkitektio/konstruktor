use serde::{Deserialize, Serialize};

use crate::catalog::{ServiceId, SERVICE_IDS};
use crate::config::mesh::{build_mesh_block, MeshBlock, MeshOptions};
use crate::secrets::{
    generate_alpha_numeric_string, generate_django_secret_key,
    generate_name, KeyPair,
};

/// The `hub_config.yaml` Konstruktor writes.
///
/// A faithful port of `arkitekt_next/server/config/hub.py` and the defaults it pulls in
/// from `config/infrastructure.py` and `services/*.py`.
///
/// A hub runs data and compute services and trusts a remote coordination server for
/// identity, so there is deliberately no `lok`, no users and no organizations.
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
        Self { kind: "local".into() }
    }
    fn global() -> Self {
        Self { kind: "global".into() }
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
    /// Lovekit is not a container in the generated stack and declares no image.
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ollama_config: Option<Kinded>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ensured_repositories: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance_issuer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance_kid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance_key_pair: Option<KeyPair>,
}

impl ServiceBlock {
    /// The bucket declared for a purpose, if this service declares one.
    pub fn bucket(&self, purpose: &str) -> Option<&LocalBucket> {
        match purpose {
            "media" => Some(&self.media_bucket),
            "zarr" => self.zarr_bucket.as_ref(),
            "parquet" => self.parquet_bucket.as_ref(),
            "bigfile" => self.bigfile_bucket.as_ref(),
            _ => None,
        }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubConfig {
    pub alpaka: ServiceBlock,
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
    pub local_redis: RedisBlock,
    /// Present only on a hub that joined a mesh. Upstream's config model does not know
    /// this key, so it is omitted entirely rather than written as `enabled: false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh: Option<MeshBlock>,
    pub mikro: ServiceBlock,
    pub minio: MinioBlock,
    pub rekuest: ServiceBlock,
    pub rekuest_server: String,
    pub lovekit: ServiceBlock,
}

impl HubConfig {
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
        }
    }

    fn service_mut(&mut self, id: ServiceId) -> &mut ServiceBlock {
        match id {
            ServiceId::Rekuest => &mut self.rekuest,
            ServiceId::Mikro => &mut self.mikro,
            ServiceId::Fluss => &mut self.fluss,
            ServiceId::Kabinet => &mut self.kabinet,
            ServiceId::Kraph => &mut self.kraph,
            ServiceId::Elektro => &mut self.elektro,
            ServiceId::Alpaka => &mut self.alpaka,
            ServiceId::Lovekit => &mut self.lovekit,
        }
    }

    /// Enabled services, in the order the generator feeds them.
    pub fn enabled_services(&self) -> Vec<ServiceId> {
        crate::catalog::HUB_SERVICE_ORDER
            .into_iter()
            .filter(|id| self.service(*id).enabled)
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

        images.push((DB_COMPOSE_SERVICE.to_string(), self.db.image.clone()));
        images.push((self.local_redis.host.clone(), self.local_redis.image.clone()));
        images.push((self.minio.host.clone(), self.minio.image.clone()));
        images.push((
            self.minio.init_container_host.clone(),
            self.minio.init_container_image.clone(),
        ));
        images.push((self.gateway.host.clone(), self.gateway.image.clone()));

        if let Some(mesh) = self.mesh.as_ref().filter(|m| m.enabled) {
            images.push((mesh.host.clone(), mesh.image.clone()));
        }
        images
    }
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
            image: Some("jhnnsrs/rekuest:next"),
            db: "rekuest",
            github_repo: "https://github.com/arkitektio/rekuest-server-next",
        },
        ServiceId::Mikro => ServiceSeed {
            enabled: true,
            image: Some("jhnnsrs/mikro:next"),
            db: "mikro",
            github_repo: "https://github.com/arkitektio/mikro-server-next",
        },
        ServiceId::Fluss => ServiceSeed {
            enabled: true,
            image: Some("jhnnsrs/fluss:next"),
            db: "fluss",
            github_repo: "https://github.com/arkitektio/fluss-server-next",
        },
        ServiceId::Kabinet => ServiceSeed {
            enabled: true,
            image: Some("jhnnsrs/kabinet:next"),
            db: "kabinet",
            github_repo: "https://github.com/arkitektio/kabinet-server",
        },
        ServiceId::Kraph => ServiceSeed {
            enabled: true,
            image: Some("jhnnsrs/kraph:dev"),
            db: "kraph",
            github_repo: "https://github.com/arkitektio/kraph-server",
        },
        ServiceId::Elektro => ServiceSeed {
            enabled: false,
            image: Some("jhnnsrs/elektro:next"),
            db: "elektro",
            github_repo: "https://github.com/arkitektio/elektro-server",
        },
        ServiceId::Alpaka => ServiceSeed {
            enabled: false,
            image: Some("jhnnsrs/alpaka:next"),
            db: "alpaka",
            github_repo: "https://github.com/arkitektio/alpaka-server",
        },
        // No image: lovekit is a LiveKit service, not a Django app, and is never emitted.
        ServiceId::Lovekit => ServiceSeed {
            enabled: true,
            image: None,
            db: "lovekit",
            github_repo: "https://github.com/arkitektio/lovekit-server",
        },
    }
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
        ollama_config: None,
        ensured_repositories: None,
        provenance_issuer: None,
        provenance_kid: None,
        provenance_key_pair: None,
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
        }
        ServiceId::Elektro => {
            block.zarr_bucket = Some(LocalBucket::new("elektrozarr"));
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
    /// Injected by the tests; generated fresh otherwise.
    pub provenance_key_pair: Option<KeyPair>,
    /// A *dev hub*: every service's source is checked out on this machine and mounted
    /// over the image's workspace, so `mount_github` is set on each service block and
    /// the generated compose file carries the bind mounts. The branch is not part of the
    /// config — upstream's model has no key for it, and it is only needed at the moment
    /// the checkout happens.
    pub dev_hub: bool,
}

impl Default for HubConfigOptions {
    fn default() -> Self {
        Self {
            device_id: String::new(),
            coord_server: String::new(),
            rekuest_server: "local".into(),
            services: None,
            http_port: Some(80),
            https_port: Some(443),
            ssl: false,
            domain: None,
            global_admin: "admin".into(),
            global_admin_password: None,
            global_admin_email: None,
            global_description: None,
            csrf_trusted_origins: None,
            mesh: None,
            provenance_key_pair: None,
            dev_hub: false,
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

    let take = |blocks: &mut Vec<(ServiceId, ServiceBlock)>, id: ServiceId| {
        let index = blocks.iter().position(|(i, _)| *i == id).expect("every id is seeded");
        blocks.remove(index).1
    };

    let mut rekuest = take(&mut blocks, ServiceId::Rekuest);
    rekuest.enabled = options.rekuest_server.trim() == "local";
    rekuest.provenance_key_pair = Some(
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

        coord_server: options.coord_server.clone(),
        csrf_trusted_origins: options.csrf_trusted_origins.clone(),
        db: DbBlock {
            enabled: true,
            github_repo: "https://github.com/arkitektio/daten-server".into(),
            host: "daten".into(),
            image: "jhnnsrs/daten:dev".into(),
            // Bind-mounted inside the deployment folder, so a `docker compose down -v`
            // cannot take the database with it.
            mount: Some("./db_data".into()),
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
            image: "caddy:latest".into(),
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
        local_redis: RedisBlock {
            enabled: true,
            host: "redis".into(),
            image: "redis:latest".into(),
            internal_port: 6379,
        },
        mesh: options.mesh.as_ref().map(build_mesh_block),
        minio: MinioBlock {
            access_key: generate_alpha_numeric_string(40),
            console_port: 9001,
            enabled: true,
            exposed_console_port: None,
            host: "minio".into(),
            image: "minio/minio:RELEASE.2025-02-18T16-25-55Z".into(),
            init_container_host: "minio_init".into(),
            init_container_image: "jhnnsrs/init:dev".into(),
            internal_port: 9000,
            // Upstream's default is the container-absolute `/data`, which docker turns
            // into an anonymous volume. Keeping it beside the database makes the whole
            // deployment one movable folder.
            mount: Some("./minio_data".into()),
            root_password: generate_alpha_numeric_string(40),
            root_user: generate_name(),
            secret_key: generate_alpha_numeric_string(40),
            volume_name: "minio_data".into(),
        },
        rekuest_server: options.rekuest_server.clone(),
    };

    // Every service keeps the host it was seeded with; `service_mut` exists for the
    // orchestration that folds a mesh key in after the fact.
    let _ = config.service_mut(ServiceId::Rekuest);
    config
}
