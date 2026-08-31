use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::catalog::{ServiceId, SERVICE_IDS};
use crate::config::mesh::{build_mesh_block, MeshBlock, MeshOptions};
use crate::secrets::{
    generate_alpha_numeric_string, generate_django_secret_key, generate_name, KeyPair,
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
    /// Present only when the hub runs its own Ollama. See [`OllamaBlock`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_ollama: Option<OllamaBlock>,
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
        if let Some(ollama) = self.local_ollama.as_ref().filter(|o| o.enabled) {
            images.push((ollama.host.clone(), ollama.image.clone()));
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
    /// Bind mounts at `./db_data` and `./minio_data` inside the deployment folder.
    DeploymentFolder,
}

impl StorageMode {
    /// Whether the data lives in the engine's own volumes, rather than in a folder.
    pub fn uses_volumes(self) -> bool {
        matches!(self, StorageMode::DockerVolumes)
    }
}

/// The bind mount the database uses when the data lives in the deployment folder.
pub const DB_FOLDER_MOUNT: &str = "./db_data";
/// The bind mount the object storage uses when the data lives in the deployment folder.
pub const MINIO_FOLDER_MOUNT: &str = "./minio_data";

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
            http_port: Some(7080),
            https_port: Some(7443),
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
        local_ollama: None,
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
            // The dashboard mirrors this name to recognise a run-once container, where
            // "exited" is success rather than a failure — see `isInitContainer`.
            init_container_host: "minio_init".into(),
            init_container_image: "jhnnsrs/init:dev".into(),
            internal_port: 9000,
            // Upstream's default is the container-absolute `/data`, which docker turns
            // into an *anonymous* volume — nothing to find again after `down`. Ours is
            // the named volume, or the folder beside the database when that was asked.
            mount: (!options.storage.uses_volumes()).then(|| MINIO_FOLDER_MOUNT.into()),
            root_password: generate_alpha_numeric_string(40),
            root_user: generate_name(),
            secret_key: generate_alpha_numeric_string(40),
            volume_name: "minio_data".into(),
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

    // Every service keeps the host it was seeded with; `service_mut` exists for the
    // orchestration that folds a mesh key in after the fact.
    let _ = config.service_mut(ServiceId::Rekuest);
    config
}
