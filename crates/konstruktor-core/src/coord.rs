//! A *coordination server*: the third kind of deployment, and the only one that is not a
//! client of another.
//!
//! A hub is a stack of services and a plugin engine is one deployer container; both are
//! authorized *against* a coordination server, which is where users, organizations and
//! permissions live. `go.arkitekt.live` is one. This module is for running your own.
//!
//! That inverts every assumption the other two paths make. A coordination server issues
//! the tokens rather than presenting them, so it has no device-code flow to complete, no
//! identifier inside somebody else's organization, and no manifest to send anywhere — it
//! is the root of trust, not a claimant.
//!
//! # What it is made of
//!
//! A hub's profile with no services ([`crate::config::coord`]), so that it is kept like a
//! hub — pinned, backed up, updated and rolled back by the same code. It runs:
//!
//! * **Lok**, issuing tokens as the one address it is reached at, which is also what hubs
//!   put in `--server`;
//! * **kontrol**, the account frontend, at the root of that address — where somebody logs
//!   in, and accepts a hub or an app into an organization;
//! * optionally **ionskale**, the mesh, at a name of its own on the same gateway;
//! * the stack's Postgres, Redis, object store and gateway.
//!
//! The gateway serves the address with a certificate every client trusts, obtained over
//! ACME or provided ([`crate::config::hub::PublicTls`]) — apps trust a hub's self-signed
//! certificate *because* they got it from here. A coordination server that serves plain
//! HTTP is for a lab or a test, reached on a network one trusts.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::coord::{build_coord_config, CoordOptions, MeshChoice, SignupPolicy, SmtpBlock};
use crate::config::hub::{HubConfig, PublicSite, PublicTls, StorageMode};
use crate::create::CreateEvent;
use crate::profile::{hub_profile, write_profile};
use crate::{docker, registry};

/// The registry's `kind` for a coordination server.
pub const COORD_KIND: &str = "coord";

/// Lok's config inside the deployment folder.
///
/// What told a coordination server's folder apart before it had a profile; one with a
/// profile says so in it ([`crate::profile::profile_is_a_coord`]). A self-contained hub
/// writes this file too, and is recognised as a hub by its profile first.
pub const COORD_CONFIG_FILE: &str = "configs/lok.yaml";

/// How long a new coordination server is waited on: long enough for a CA to be asked.
const READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Whether this folder holds a coordination server.
pub fn holds_a_coord(dir: &Path) -> bool {
    crate::profile::profile_is_a_coord(dir) || dir.join(COORD_CONFIG_FILE).is_file()
}

/// Everything a front end collects for a coordination server. Flat and serde-friendly,
/// like [`crate::create::HubAnswers`] and [`crate::engine::EngineAnswers`].
///
/// Deliberately short of the hub's questions. There is no coordination server to name
/// itself to and no services to pick: what a coordination server needs is a name to be
/// reached at, a certificate for it, and a first administrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordAnswers {
    pub dir: String,
    pub name: String,
    /// The DNS name clients reach it at — what hubs put in `--server`. For one that serves
    /// plain HTTP, any host this machine is reached at (`localhost` for a test).
    pub domain: String,
    /// Where its certificate comes from. `None` serves plain HTTP: a lab's, or a test's.
    #[serde(default)]
    pub tls: Option<PublicTls>,
    pub http_port: u16,
    pub https_port: u16,
    /// The mesh's own name, served by the same gateway. Needs TLS.
    #[serde(default)]
    pub mesh_domain: Option<String>,
    #[serde(default)]
    pub mesh_dns_suffix: Option<String>,
    /// The first account, which everything else is administered through.
    pub admin: String,
    #[serde(default)]
    pub admin_password: Option<String>,
    #[serde(default)]
    pub admin_email: Option<String>,
    /// The organization the administrator starts out in. Left out, the server's name.
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default)]
    pub email: Option<SmtpBlock>,
    #[serde(default)]
    pub signup: SignupPolicy,
    /// The addresses this machine is reached at from the internet, when it cannot see them
    /// itself — behind a NAT or a load balancer. What the DNS check compares against, beside
    /// its own interfaces'.
    #[serde(default)]
    pub public_ips: Vec<IpAddr>,
    #[serde(default)]
    pub skip_dns_check: bool,
    /// Images to run instead of the ones a new coordination server is seeded with, by compose
    /// service: `lok` → `jhnnsrs/lok:8.1.0`.
    #[serde(default)]
    pub images: BTreeMap<String, String>,
    /// The same, for whatever of them this coordination server runs: an entry naming one it
    /// does not is skipped rather than refused. What `$KONSTRUKTOR_IMAGES` fills.
    #[serde(default)]
    pub default_images: BTreeMap<String, String>,
    #[serde(default)]
    pub storage: StorageMode,
    /// Run `docker compose up -d` once everything is written, and wait for it.
    #[serde(default)]
    pub start: bool,
}

pub struct CreatedCoord {
    pub path: PathBuf,
    pub record: registry::DeploymentRecord,
    pub config: HubConfig,
    /// What a provided certificate was found to be.
    pub certificate: Option<crate::tls::CertificateInfo>,
    /// What every endpoint last said, when it was started.
    pub endpoints: Vec<crate::ready::Endpoint>,
}

#[derive(Debug, thiserror::Error)]
pub enum CoordError {
    #[error("{0}")]
    Answers(String),
    #[error("Docker is not ready: {0}")]
    Docker(String),
    #[error("{0}")]
    Folder(String),
    /// What has to be true of this machine first: its names point here, its ports are free.
    #[error("{0}")]
    Preflight(String),
    #[error("{0}")]
    Certificate(String),
    #[error("Could not write the deployment: {0}")]
    Write(#[from] std::io::Error),
    #[error("The coordination server was written but did not start. `konstruktor logs` says why.")]
    StartFailed,
}

/// The origin a coordination server is reached at, from what was answered.
pub fn origin(answers: &CoordAnswers) -> String {
    let domain = answers.domain.trim().to_lowercase();
    match &answers.tls {
        Some(_) => crate::config::hub::origin("https", &domain, answers.https_port),
        None => crate::config::hub::origin("http", &domain, answers.http_port),
    }
}

/// What cannot be created, said before anything is asked or written.
pub fn validate(answers: &CoordAnswers) -> Result<(), CoordError> {
    let refuse = |why: &str| Err(CoordError::Answers(why.to_string()));
    let domain = answers.domain.trim();
    if domain.is_empty() {
        return refuse(
            "A coordination server needs the name it is reached at: `--domain coord.example.org`.",
        );
    }
    if domain.contains("://") || domain.contains('/') || domain.contains(':') {
        return refuse(
            "`--domain` is a name, without a scheme, a path or a port: `coord.example.org`.",
        );
    }
    // A hub's services fetch the coordination server's keys from inside their containers,
    // where `localhost` is the container itself.
    if crate::hosts::classify_host(domain) == crate::hosts::HostCategory::Loopback {
        return refuse(
            "A coordination server is reached by hubs' containers, where `localhost` is the \
             container itself: give `--domain` an address or name of this machine that other \
             containers and machines reach — its LAN address, say.",
        );
    }
    if answers.admin.trim().is_empty() {
        return refuse("A coordination server needs a first administrator: `--admin`.");
    }
    if answers.http_port == answers.https_port {
        return refuse("The HTTP and HTTPS ports have to differ.");
    }

    match &answers.tls {
        Some(PublicTls::Acme { email, .. }) => {
            if !email.contains('@') {
                return refuse(
                    "Let's Encrypt needs an address to write to about the certificate: \
                     `--acme-email`.",
                );
            }
            // The CA asks on 80 (HTTP-01) or 443 (TLS-ALPN-01), and nowhere else.
            if answers.http_port != 80 || answers.https_port != 443 {
                return refuse(
                    "A certificate from Let's Encrypt is obtained on ports 80 and 443, where \
                     the CA asks for it: leave `--http-port` and `--https-port` at 80 and 443, \
                     or provide a certificate with `--tls-cert` and `--tls-key`.",
                );
            }
            if domain.parse::<IpAddr>().is_ok() || !domain.contains('.') {
                return refuse(
                    "Let's Encrypt issues certificates for DNS names only: give `--domain` a \
                     name that resolves to this machine.",
                );
            }
        }
        Some(PublicTls::Provided {
            cert_path,
            key_path,
        }) => {
            for (flag, path) in [("--tls-cert", cert_path), ("--tls-key", key_path)] {
                if !Path::new(path).is_file() {
                    return Err(CoordError::Answers(format!(
                        "{path} (`{flag}`) is not a file."
                    )));
                }
            }
        }
        None => {}
    }

    if let Some(mesh) = answers.mesh_domain.as_deref().map(str::trim) {
        if answers.tls.is_none() {
            return refuse(
                "The mesh is reached over TLS at a name of its own: it needs a coordination \
                 server with a certificate, not `--plain-http`.",
            );
        }
        if mesh.is_empty() || mesh.contains(':') || mesh.contains('/') {
            return refuse("`--mesh-domain` is a name, without a scheme, a path or a port.");
        }
        // Its nodes are told `<name>:443`, and nothing here publishes it elsewhere.
        if answers.https_port != 443 {
            return refuse(
                "The mesh is reached on port 443 of its name: leave `--https-port` at 443 to \
                 run one.",
            );
        }
        if mesh.eq_ignore_ascii_case(domain) {
            return refuse(
                "The mesh needs a name of its own, beside the coordination server's: its \
                 nodes reach it at the root.",
            );
        }
    }
    Ok(())
}

/// The names a coordination server is served at: its domain, and its mesh's.
fn public_names(answers: &CoordAnswers) -> Vec<String> {
    std::iter::once(answers.domain.trim().to_lowercase())
        .chain(
            answers
                .mesh_domain
                .as_deref()
                .map(|m| m.trim().to_lowercase()),
        )
        .collect()
}

/// What has to be true of this machine before anything is written: the public names point
/// here, and the ports the CA asks on are free.
pub async fn preflight(answers: &CoordAnswers) -> Result<(), CoordError> {
    let ports: Vec<u16> = match &answers.tls {
        Some(_) => vec![answers.http_port, answers.https_port],
        None => vec![answers.http_port],
    };
    let busy = crate::preflight::busy_ports(&ports).await;
    if !busy.is_empty() {
        return Err(CoordError::Preflight(format!(
            "Something on this machine already answers on {}: a coordination server is \
             reached there. Stop it, or pick other ports.",
            busy.iter()
                .map(|p| format!("port {p}"))
                .collect::<Vec<_>>()
                .join(" and ")
        )));
    }

    if answers.tls.is_none() || answers.skip_dns_check {
        return Ok(());
    }
    let mut ours = crate::preflight::local_addresses();
    ours.extend(answers.public_ips.iter().copied());
    let findings = crate::preflight::resolve_names(&public_names(answers), &ours).await;
    match crate::preflight::describe_names(&findings) {
        Some(wrong) => Err(CoordError::Preflight(wrong)),
        None => Ok(()),
    }
}

/// Builds a coordination server's profile from what was answered: no image asked yet.
pub fn build(answers: &CoordAnswers, device_id: &str) -> Result<HubConfig, CoordError> {
    let domain = answers.domain.trim().to_lowercase();
    let mesh_domain = answers
        .mesh_domain
        .as_deref()
        .map(|m| m.trim().to_lowercase());
    let extra_roots = crate::trust::extra_roots()
        .map_err(CoordError::Certificate)?
        .iter()
        .map(|root| der_to_pem(root.as_ref()))
        .collect::<String>();

    let mut config = build_coord_config(&CoordOptions {
        device_id: device_id.to_string(),
        name: answers.name.clone(),
        origin: origin(answers),
        public: answers.tls.clone().map(|tls| PublicSite {
            domain: domain.clone(),
            extra_names: mesh_domain.iter().cloned().collect(),
            tls,
        }),
        http_port: Some(answers.http_port),
        // One port: a server that serves plain HTTP listens on nothing else.
        https_port: answers.tls.as_ref().map(|_| answers.https_port),
        admin: answers.admin.trim().to_string(),
        admin_password: answers.admin_password.clone(),
        admin_email: answers.admin_email.clone(),
        organization: answers
            .organization
            .clone()
            .filter(|o| !o.trim().is_empty())
            .unwrap_or_else(|| answers.name.clone()),
        mesh: mesh_domain.map(|domain| MeshChoice {
            domain,
            dns_suffix: answers.mesh_dns_suffix.clone(),
        }),
        email: answers.email.clone(),
        signup: answers.signup,
        storage: answers.storage,
        extra_roots: (!extra_roots.is_empty()).then_some(extra_roots),
        key_pair: None,
    });
    // The self-signed certificate every name but the public ones is served with: the
    // probes', which ask at `localhost`, and the gateway's own on the stack's network.
    if answers.tls.is_some() {
        config
            .place_certificate(&[])
            .map_err(CoordError::Certificate)?;
    }
    Ok(config)
}

/// A DER certificate as PEM.
fn der_to_pem(der: &[u8]) -> String {
    use base64::Engine;
    let body = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = String::from("-----BEGIN CERTIFICATE-----\n");
    for line in body.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(line).expect("base64 is ASCII"));
        out.push('\n');
    }
    out.push_str("-----END CERTIFICATE-----\n");
    out
}

/// Generate, write, register and start a coordination server.
///
/// Nothing is written until everything that can be checked here was: the answers, Docker,
/// the folder, the names and ports, a provided certificate, and what Lok's image says it
/// is. Unlike the other two creators it takes no `CancellationToken` and authorizes against
/// nobody: there is nobody to authorize against.
pub async fn create_coord(
    answers: &CoordAnswers,
    on: &(dyn Fn(CreateEvent) + Sync),
) -> Result<CreatedCoord, CoordError> {
    validate(answers)?;

    on(CreateEvent::CheckingDocker);
    let probe = docker::probe().await;
    if !probe.is_ready() {
        return Err(CoordError::Docker(crate::create::describe_docker(&probe)));
    }

    let dir = PathBuf::from(&answers.dir);
    let mut store = registry::load();
    // A folder that is not there yet is made once everything was checked; one that is has
    // to be one a deployment can go in.
    let verdict = registry::inspect_folder(&store, &dir);
    if !verdict.can_create() && verdict != registry::FolderVerdict::Missing {
        return Err(CoordError::Folder(verdict.describe()));
    }

    on(CreateEvent::Log {
        line: "Checking this machine: its ports, and where its names point…".into(),
    });
    preflight(answers).await?;

    on(CreateEvent::Building);
    let mut config = build(answers, &store.device_id)?;
    // Checked before anything is written, against what clients trust.
    if let (
        Some(PublicTls::Provided {
            cert_path,
            key_path,
        }),
        Some(site),
    ) = (&answers.tls, config.public_site())
    {
        let read = |path: &str| std::fs::read(path).map_err(|e| format!("{path}: {e}"));
        let cert = read(cert_path).map_err(CoordError::Certificate)?;
        let key = read(key_path).map_err(CoordError::Certificate)?;
        crate::tls::inspect(
            &cert,
            Some(&key),
            &site.names(),
            crate::trust::root_store().map_err(CoordError::Certificate)?,
            std::time::SystemTime::now(),
        )
        .map_err(|why| {
            CoordError::Certificate(format!("The certificate cannot be used: {why}."))
        })?;
        // Trusted here only because of a root named for Konstruktor alone: a hub's services,
        // its apps and every browser elsewhere check it against their own machine's roots.
        let public = crate::trust::root_store_with(Vec::new()).map_err(CoordError::Certificate)?;
        if crate::tls::inspect(
            &cert,
            None,
            &site.names(),
            public,
            std::time::SystemTime::now(),
        )
        .is_err()
        {
            on(CreateEvent::Log {
                line: format!(
                    "The certificate chains to a root only {} names, not one this machine \
                     trusts. Hubs, apps and browsers elsewhere refuse it unless their own \
                     machines trust that root.",
                    crate::trust::CA_FILE_ENV
                ),
            });
        }
    }

    let answer = |e: crate::create::CreateError| CoordError::Answers(e.to_string());
    let defaults = crate::create::images_this_hub_runs(&config, &answers.default_images);
    crate::create::apply_images(&mut config, &defaults).map_err(answer)?;

    on(CreateEvent::Log {
        line: "Asking Lok's image what it is…".into(),
    });
    let said = crate::contract::describe_all(&config, &BTreeMap::new()).await;
    let silent = crate::contract::undescribed(&config, &said);
    if !silent.is_empty() {
        return Err(CoordError::Answers(format!(
            "{} carries no description: the image cannot be found here or in its registry, \
             or it is not a release Konstruktor runs (no `{}` label). Nothing was written.",
            silent.join(", "),
            crate::contract::LABEL
        )));
    }
    // The mesh control server is the one Lok's release says it runs beside.
    sidecar_images(&mut config, &said);
    crate::create::apply_images(&mut config, &answers.images).map_err(answer)?;
    config.provide(&said);

    // --- write --------------------------------------------------------------
    let files = crate::generate::generate_hub_files(&config, &Default::default(), &said);
    for name in files.keys() {
        on(CreateEvent::Writing { file: name.clone() });
    }
    std::fs::create_dir_all(&dir)?;
    write_profile(&dir, &hub_profile(config.clone()))
        .map_err(|e| CoordError::Write(std::io::Error::other(e.to_string())))?;
    crate::migrate::write_hub(&dir, &config, &files)
        .map_err(|e| CoordError::Write(std::io::Error::other(e.to_string())))?;
    crate::contract::remember(&dir, &config, &said)?;
    let certificate =
        crate::tls::install_public_files(&dir, &config).map_err(CoordError::Certificate)?;

    let record = registry::register_kind(
        &mut store,
        COORD_KIND,
        &answers.name,
        &dir.to_string_lossy(),
        None,
        None,
        crate::create::now_rfc3339(),
    );
    let _ = registry::save(&store);

    // --- start --------------------------------------------------------------
    let mut endpoints = Vec::new();
    if answers.start {
        on(CreateEvent::Starting);
        let log = |line: crate::compose::ComposeLine| on(CreateEvent::Log { line: line.line });
        if let Err(error) = crate::start::start(&dir, &log).await {
            on(CreateEvent::Log {
                line: error.to_string(),
            });
            return Err(CoordError::StartFailed);
        }
        on(CreateEvent::Log {
            line: match config.public_site().map(|site| &site.tls) {
                Some(PublicTls::Acme { .. }) => {
                    "Waiting for it to answer, with a certificate from the CA…".into()
                }
                _ => "Waiting for it to answer…".into(),
            },
        });
        endpoints = crate::ready::wait(&config, READY_TIMEOUT, &|_| {})
            .await
            .unwrap_or_default();
    }

    on(CreateEvent::Done {
        path: dir.to_string_lossy().to_string(),
    });
    Ok(CreatedCoord {
        path: dir,
        record,
        config,
        certificate,
        endpoints,
    })
}

/// Points the mesh at the image Lok's release names for it, when it names one.
fn sidecar_images(config: &mut HubConfig, said: &crate::contract::Said) {
    let Some(lok) = config.running_lok() else {
        return;
    };
    let lok_image = lok.image.clone();
    let Some(description) = said.get(&lok.host) else {
        return;
    };
    let named = description
        .sidecars
        .iter()
        .find(|sidecar| sidecar.name == "ionskale")
        .map(|sidecar| sidecar.image_beside(&lok_image));
    if let (Some(image), Some(mesh)) = (
        named,
        config.coord.as_mut().and_then(|coord| coord.mesh.as_mut()),
    ) {
        mesh.image = image;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answers() -> CoordAnswers {
        CoordAnswers {
            dir: "/tmp/coord".into(),
            name: "Lab".into(),
            domain: "coord.example.org".into(),
            tls: Some(PublicTls::Acme {
                email: "ops@example.org".into(),
                ca: None,
                ca_root: None,
            }),
            http_port: 80,
            https_port: 443,
            mesh_domain: Some("mesh.coord.example.org".into()),
            mesh_dns_suffix: None,
            admin: "admin".into(),
            admin_password: None,
            admin_email: None,
            organization: None,
            email: None,
            signup: SignupPolicy::Closed,
            public_ips: Vec::new(),
            skip_dns_check: false,
            images: BTreeMap::new(),
            default_images: BTreeMap::new(),
            storage: StorageMode::default(),
            start: false,
        }
    }

    #[test]
    fn a_lok_config_marks_the_folder() {
        let dir = std::env::temp_dir().join(format!("konstruktor-coord-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("configs")).unwrap();
        assert!(!holds_a_coord(&dir));
        std::fs::write(dir.join(COORD_CONFIG_FILE), "lok: {}").unwrap();
        assert!(holds_a_coord(&dir));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn its_origin_is_its_name_on_the_port_it_is_served_at() {
        assert_eq!(origin(&answers()), "https://coord.example.org");
        let mut lab = answers();
        lab.tls = None;
        lab.mesh_domain = None;
        lab.domain = "192.168.1.20".into();
        lab.http_port = 7180;
        assert_eq!(origin(&lab), "http://192.168.1.20:7180");
    }

    #[test]
    fn lets_encrypt_is_only_asked_on_80_and_443_for_a_name() {
        let mut moved = answers();
        moved.https_port = 8443;
        assert!(
            matches!(validate(&moved), Err(CoordError::Answers(why)) if why.contains("80 and 443"))
        );
        let mut address = answers();
        address.domain = "203.0.113.7".into();
        assert!(validate(&address).is_err());
        assert!(validate(&answers()).is_ok());
    }

    #[test]
    fn the_mesh_needs_tls_and_a_name_of_its_own() {
        let mut plain = answers();
        plain.tls = None;
        assert!(validate(&plain).is_err());
        let mut same = answers();
        same.mesh_domain = Some("COORD.example.org".into());
        assert!(validate(&same).is_err());
    }

    #[test]
    fn the_mesh_is_only_run_on_443() {
        let dir =
            std::env::temp_dir().join(format!("konstruktor-mesh-port-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (cert, key) = (dir.join("cert.pem"), dir.join("key.pem"));
        std::fs::write(&cert, "").unwrap();
        std::fs::write(&key, "").unwrap();
        let mut answers = answers();
        answers.tls = Some(PublicTls::Provided {
            cert_path: cert.to_string_lossy().to_string(),
            key_path: key.to_string_lossy().to_string(),
        });
        answers.https_port = 8443;
        assert!(matches!(validate(&answers), Err(CoordError::Answers(why)) if why.contains("443")));
        answers.mesh_domain = None;
        assert!(validate(&answers).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_domain_is_a_name_hubs_containers_reach() {
        for wrong in [
            "https://coord.example.org",
            "coord.example.org:443",
            "",
            "localhost",
            "127.0.0.1",
        ] {
            let mut answers = answers();
            answers.domain = wrong.into();
            assert!(validate(&answers).is_err(), "{wrong}");
        }
    }

    #[test]
    fn it_is_built_with_its_public_names_and_a_fallback_certificate() {
        let config = build(&answers(), "device").unwrap();
        let site = config.public_site().unwrap();
        assert_eq!(
            site.names(),
            ["coord.example.org", "mesh.coord.example.org"]
        );
        let fallback = config.served_certificate().unwrap();
        assert!(fallback.hosts.contains(&"localhost".to_string()));
        assert_eq!(config.gateway.exposed_https_port, Some(443));
    }

    #[test]
    fn a_der_certificate_reads_back_from_its_pem() {
        let certificate = crate::tls::generate(&["lab.local".into()], 2026).unwrap();
        let der = crate::trust::parse_certificates(certificate.cert_pem.as_bytes()).unwrap();
        let again =
            crate::trust::parse_certificates(der_to_pem(der[0].as_ref()).as_bytes()).unwrap();
        assert_eq!(again, der);
    }
}
