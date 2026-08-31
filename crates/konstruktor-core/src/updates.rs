//! "Is there something newer upstream?" — answered without pulling.
//!
//! A pull is the honest way to find out, and the expensive one: it downloads the layers
//! whether or not anything changed. A registry answers the same question for free — the
//! digest a tag resolves to is in one response header — so the dashboard asks the
//! registry when it opens and only suggests a pull when the answer differs from what the
//! engine already holds.
//!
//! Only the digest is compared. Every registry that speaks the distribution API (Docker
//! Hub, GHCR, Quay, a private one) returns `Docker-Content-Digest` for a manifest request,
//! and that digest is exactly what the engine records in `RepoDigests` after a pull.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;

use crate::config::hub::{HubConfig, DB_COMPOSE_SERVICE};
use crate::docker::ImageState;

const TIMEOUT: Duration = Duration::from_secs(10);

const ACCEPT: &str = "application/vnd.docker.distribution.manifest.list.v2+json, \
application/vnd.oci.image.index.v1+json, \
application/vnd.docker.distribution.manifest.v2+json, \
application/vnd.oci.image.manifest.v1+json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpstreamState {
    /// The tag upstream points at what the engine already has.
    Current,
    /// The tag has moved on; a pull would bring something new.
    Newer,
    /// Nothing pulled yet, so there is nothing to compare — a pull is due regardless.
    Missing,
    /// The registry could not be asked, or did not say. `error` explains.
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamCheck {
    pub service: String,
    pub image: String,
    pub state: UpstreamState,
    pub remote_digest: Option<String>,
    pub error: Option<String>,
}

/// One image reference, taken apart the way the engine does it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Reference {
    pub host: String,
    pub repository: String,
    pub tag: String,
}

/// `[host/]path[:tag]` — Docker's rules: the first component is a host only if it has a
/// dot or a port, or is `localhost`; a bare Hub name lives under `library/`.
pub(crate) fn parse(image: &str) -> Reference {
    // A `@sha256:…` pin cannot move, but it is not what the stack files write; strip it
    // so the tag lookup still works if someone does.
    let image = image.split('@').next().unwrap_or(image);

    let (mut path, tag) = match image.rsplit_once(':') {
        // `host:5000/repo` has a colon before a slash — that is a port, not a tag.
        Some((path, tag)) if !tag.contains('/') => (path.to_string(), tag.to_string()),
        _ => (image.to_string(), "latest".to_string()),
    };

    let mut host = "registry-1.docker.io".to_string();
    if let Some((first, rest)) = path.split_once('/') {
        if first.contains('.') || first.contains(':') || first == "localhost" {
            host = if first == "docker.io" {
                "registry-1.docker.io".to_string()
            } else {
                first.to_string()
            };
            path = rest.to_string();
        }
    }
    if host == "registry-1.docker.io" && !path.contains('/') {
        path = format!("library/{path}");
    }

    Reference {
        host,
        repository: path,
        tag,
    }
}

/// The digest `image` resolves to at its registry right now.
pub async fn remote_digest(image: &str) -> Result<String, String> {
    let reference = parse(image);
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!(
        "https://{}/v2/{}/manifests/{}",
        reference.host, reference.repository, reference.tag
    );

    let first = client
        .head(&url)
        .header("Accept", ACCEPT)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let response = if first.status() == reqwest::StatusCode::UNAUTHORIZED {
        let challenge = first
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| "registry asked for auth without saying how".to_string())?;
        let token = token(&client, challenge, &reference).await?;
        client
            .head(&url)
            .header("Accept", ACCEPT)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| e.to_string())?
    } else {
        first
    };

    if !response.status().is_success() {
        return Err(format!("registry answered {}", response.status()));
    }
    response
        .headers()
        .get("docker-content-digest")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| "registry did not report a digest".to_string())
}

/// Trades a `Bearer realm=…,service=…,scope=…` challenge for an anonymous pull token.
async fn token(
    client: &reqwest::Client,
    challenge: &str,
    reference: &Reference,
) -> Result<String, String> {
    let params = challenge
        .trim_start_matches("Bearer ")
        .split(',')
        .filter_map(|part| {
            let (key, value) = part.trim().split_once('=')?;
            Some((key.to_string(), value.trim_matches('"').to_string()))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let realm = params
        .get("realm")
        .ok_or_else(|| "auth challenge without a realm".to_string())?;
    let scope = params
        .get("scope")
        .cloned()
        .unwrap_or_else(|| format!("repository:{}:pull", reference.repository));

    let mut query = vec![("scope", scope)];
    if let Some(service) = params.get("service") {
        query.push(("service", service.clone()));
    }

    #[derive(Deserialize)]
    struct Token {
        token: Option<String>,
        access_token: Option<String>,
    }
    let issued: Token = client
        .get(realm)
        .query(&query)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    issued
        .token
        .or(issued.access_token)
        .ok_or_else(|| "auth server issued no token".to_string())
}

/// The digest an image reference pins itself to, if it names one.
fn pinned_digest(image: &str) -> Option<&str> {
    image.split_once('@').map(|(_, digest)| digest)
}

fn digest_of(repo_digest: &str) -> &str {
    repo_digest.rsplit('@').next().unwrap_or(repo_digest)
}

/// Every image the stack declares, checked against its registry, concurrently.
pub async fn check(images: &[ImageState]) -> Vec<UpstreamCheck> {
    let mut set = JoinSet::new();
    for (index, state) in images.iter().cloned().enumerate() {
        set.spawn(async move { (index, check_one(state).await) });
    }
    let mut results = Vec::new();
    while let Some(joined) = set.join_next().await {
        if let Ok(item) = joined {
            results.push(item);
        }
    }
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, check)| check).collect()
}

/// The local state of every image a deployment's stack declares.
///
/// The engine only — no network. Separate from [`for_deployment`] because a front end
/// showing which images are present refreshes far more often than it asks a registry
/// anything.
pub async fn images_for_deployment(
    dir: &std::path::Path,
) -> Result<Vec<ImageState>, String> {
    let profile = crate::profile::read_profile(dir).map_err(|e| e.to_string())?;
    crate::docker::image_states(&profile.config.stack_images()).await
}

/// Every image a deployment declares, checked against its registry.
///
/// One call because the two halves were being spelled out separately in each front end,
/// and the order matters: the local state is what the remote digests are compared to.
pub async fn for_deployment(dir: &std::path::Path) -> Result<Vec<UpstreamCheck>, String> {
    let images = images_for_deployment(dir).await?;
    Ok(check(&images).await)
}

// --- advancing a pin -------------------------------------------------------------------
//
// An immutable reference never moves on its own — that is the whole point of it — so a
// pinned hub sits on the version it was created with until something deliberately moves
// it. `update` alone cannot: it pulls the reference the profile names, which resolves to
// what it always did. Advancing the pin means asking the repository what versions exist,
// picking one, and writing it into the profile.
//
// The rule for picking is deliberately narrow. Only the same major, only the same variant,
// and only forwards. A major is a migration — Postgres will not even open the old cluster
// — and it must be a decision somebody makes, not something an update offers because the
// number was bigger.

/// A tag read as a version: `16.13-1` is `[16, 13, 1]` with no variant, `8.11.0-alpine` is
/// `[8, 11, 0]` with the variant `alpine`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Version {
    parts: Vec<u64>,
    variant: String,
}

/// Reads a tag as a version, or decides it is not one.
///
/// A tag is a version if it *starts* with numbers: everything up to the first non-numeric
/// component is the version, and the rest is the variant that has to match for two tags to
/// be comparable at all. `latest`, `dev` and MinIO's `RELEASE.2025-02-18T16-25-55Z` are not
/// versions by this rule, which is correct — nothing here can order them, and offering to
/// move between them would be a guess.
pub(crate) fn version_of(tag: &str) -> Option<Version> {
    let mut parts = Vec::new();
    let mut rest = Vec::new();
    for component in tag.split(['.', '-']) {
        match component.parse::<u64>() {
            Ok(number) if rest.is_empty() => parts.push(number),
            _ => rest.push(component),
        }
    }
    (!parts.is_empty()).then(|| Version {
        parts,
        variant: rest.join("-"),
    })
}

/// The newest tag worth moving `current` to, out of everything the repository publishes.
///
/// `None` when there is nothing to move to, which is the ordinary answer: the hub is on
/// the newest, or its tag is not a version, or the only newer tags cross a major.
pub(crate) fn newer_version_tag<'a>(current: &str, available: &'a [String]) -> Option<&'a str> {
    let now = version_of(current)?;
    let major = *now.parts.first()?;

    available
        .iter()
        .filter_map(|tag| Some((tag, version_of(tag)?)))
        // Same variant, or a `16.13-1` hub would be offered `16.14-1-alpine`.
        .filter(|(_, candidate)| candidate.variant == now.variant)
        // Same major. Crossing one is a migration, not an update — see `guard`.
        .filter(|(_, candidate)| candidate.parts.first() == Some(&major))
        .filter(|(_, candidate)| candidate.parts > now.parts)
        .max_by(|(_, a), (_, b)| a.parts.cmp(&b.parts))
        .map(|(tag, _)| tag.as_str())
}

/// Every tag a repository publishes.
///
/// The same anonymous-token flow [`remote_digest`] uses, against the endpoint beside it.
pub async fn tags(image: &str) -> Result<Vec<String>, String> {
    let reference = parse(image);
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!(
        "https://{}/v2/{}/tags/list",
        reference.host, reference.repository
    );

    let first = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let response = if first.status() == reqwest::StatusCode::UNAUTHORIZED {
        let challenge = first
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| "registry asked for auth without saying how".to_string())?;
        let token = token(&client, challenge, &reference).await?;
        client
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| e.to_string())?
    } else {
        first
    };

    if !response.status().is_success() {
        return Err(format!("registry answered {}", response.status()));
    }

    #[derive(Deserialize)]
    struct Tags {
        #[serde(default)]
        tags: Vec<String>,
    }
    let listed: Tags = response.json().await.map_err(|e| e.to_string())?;
    Ok(listed.tags)
}

/// One service's pin, and the version it could be moved to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Advance {
    pub service: String,
    /// The reference the profile names now.
    pub from: String,
    /// The reference it would name instead.
    pub to: String,
}

/// What advancing this image would move it to, asked of its registry.
///
/// The digest is dropped on the way: a hub moving from `daten:16.13-1@sha256:…` to
/// `daten:16.14-1` is moving to a version, and re-pinning it to a digest here would record
/// what today's registry happens to serve rather than what was chosen.
pub async fn advance_for(service: &str, image: &str) -> Option<Advance> {
    let bare = image.split('@').next().unwrap_or(image);
    let (repository, tag) = bare.rsplit_once(':')?;
    let available = tags(bare).await.ok()?;
    let newer = newer_version_tag(tag, &available)?;
    Some(Advance {
        service: service.to_string(),
        from: image.to_string(),
        to: format!("{repository}:{newer}"),
    })
}

/// Every infrastructure image with a newer version of itself published.
pub async fn advances(config: &HubConfig) -> Vec<Advance> {
    let mut found = Vec::new();
    for (service, image) in config.stack_images() {
        if !is_infrastructure(config, &service) {
            continue;
        }
        if let Some(advance) = advance_for(&service, &image).await {
            found.push(advance);
        }
    }
    found
}

/// Whether a compose service is infrastructure rather than one of the hub's own services.
///
/// The distinction matters because the two carry different risk. An Arkitekt service is a
/// Django application that migrates its own schema forward on start; the infrastructure is
/// the database, the object store, the gateway and the cache, where a moved image can mean
/// a cluster the new binary refuses to open. So `update` moves services by default and
/// takes `--infra` to be told to move the rest.
///
/// Derived from the profile rather than a list of names: everything `stack_images` emits
/// that is not an enabled service's host is infrastructure, so a service added upstream is
/// classified correctly without this having to be edited.
pub fn is_infrastructure(config: &HubConfig, service: &str) -> bool {
    !config
        .enabled_services()
        .into_iter()
        .any(|id| config.service(id).host == service)
}

/// What has to be said before one service's image is allowed to move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "verdict", content = "detail")]
pub enum Guard {
    /// Nothing in the way.
    Clear,
    /// Recreating this service on the image now on disk would break it.
    Refuse(String),
    /// Something could not be checked. Worth saying, not worth stopping for.
    Warn(String),
}

/// Whether recreating `service` on the image currently on disk is safe.
///
/// Only the database has an answer other than [`Guard::Clear`], and it is the one that
/// matters: Postgres will not open a cluster written by a different major — there is no
/// in-place upgrade — so a `db` image that moved from 16 to 17 leaves the container
/// crash-looping and every service behind it unable to connect, with the previous image
/// already replaced.
///
/// **This must be asked after the pull and before the recreate.** The major it reads is
/// the one the *local* image declares, and before a pull that is still the image already
/// running, which agrees with the data by construction and would make the check inert.
/// Pulling first is harmless — a fetched image changes nothing until a container is
/// recreated on it — so the safe order is pull, ask, then recreate or refuse.
pub async fn guard(dir: &std::path::Path, config: &HubConfig, service: &str) -> Guard {
    if service != DB_COMPOSE_SERVICE {
        return Guard::Clear;
    }
    let data = crate::backup::live_pgdata_major(dir, config).await;
    let server = crate::docker::image_pg_major(&config.db.image).await;
    match crate::backup::major_move(data, server) {
        crate::backup::MajorMove::Same(_) => Guard::Clear,
        crate::backup::MajorMove::Across { data, server } => Guard::Refuse(format!(
            "the database image is now Postgres {server}, and this hub's data was written \
             by Postgres {data}. Postgres will not open a cluster from another major, so \
             recreating `{service}` would leave it crash-looping. Moving majors is a \
             migration: back the hub up, then restore the dump into the new version."
        )),
        crate::backup::MajorMove::Unknown => Guard::Warn(
            "the Postgres major on one of the two sides could not be read — this hub's own \
             data, or the image's `PG_MAJOR` — so whether the new image can open the \
             existing cluster is unverified"
                .to_string(),
        ),
    }
}

async fn check_one(local: ImageState) -> UpstreamCheck {
    let base = |state, remote_digest, error| UpstreamCheck {
        service: local.service.clone(),
        image: local.image.clone(),
        state,
        remote_digest,
        error,
    };

    if !local.present {
        return base(UpstreamState::Missing, None, None);
    }

    // A reference that names its own digest cannot move: `repo:tag@sha256:…` resolves to
    // that manifest whatever the tag now points at. So the comparison is against the pin
    // rather than against the registry's answer for the tag — otherwise a pinned image
    // whose channel had moved on would report `Newer` forever, and every update would pull
    // and recreate it to arrive at exactly the image it already had.
    if let Some(pinned) = pinned_digest(&local.image) {
        let held = local.repo_digests.iter().any(|d| digest_of(d) == pinned);
        return base(
            if held { UpstreamState::Current } else { UpstreamState::Newer },
            Some(pinned.to_string()),
            None,
        );
    }

    match remote_digest(&local.image).await {
        Ok(remote) => {
            let known = local
                .repo_digests
                .iter()
                .any(|d| digest_of(d) == remote);
            if local.repo_digests.is_empty() {
                // Built locally, or loaded from a tarball: there is no digest to compare.
                base(
                    UpstreamState::Unknown,
                    Some(remote),
                    Some("local image carries no registry digest".to_string()),
                )
            } else if known {
                base(UpstreamState::Current, Some(remote), None)
            } else {
                base(UpstreamState::Newer, Some(remote), None)
            }
        }
        Err(error) => base(UpstreamState::Unknown, None, Some(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hub_references() {
        assert_eq!(
            parse("jhnnsrs/rekuest:next"),
            Reference {
                host: "registry-1.docker.io".into(),
                repository: "jhnnsrs/rekuest".into(),
                tag: "next".into()
            }
        );
        assert_eq!(parse("postgres").repository, "library/postgres");
        assert_eq!(parse("postgres").tag, "latest");
        assert_eq!(parse("docker.io/library/redis:7").host, "registry-1.docker.io");
    }

    #[test]
    fn other_registries() {
        let ghcr = parse("ghcr.io/arkitektio/kabinet:dev");
        assert_eq!(ghcr.host, "ghcr.io");
        assert_eq!(ghcr.repository, "arkitektio/kabinet");
        let ported = parse("localhost:5000/thing");
        assert_eq!(ported.host, "localhost:5000");
        assert_eq!(ported.repository, "thing");
        assert_eq!(ported.tag, "latest");
        assert_eq!(parse("quay.io/minio/minio@sha256:abc").tag, "latest");
    }

    /// The rule that keeps an advance from becoming a migration. A hub on `16.13-1` may be
    /// offered `16.14-1`; it may not be offered `17.0-1`, whatever the registry publishes,
    /// because Postgres will not open the old cluster and no update should propose that
    /// silently. Variants have to match too, or a plain image would be offered an alpine
    /// one.
    #[test]
    fn only_a_newer_version_of_the_same_major_and_variant_is_offered() {
        let available: Vec<String> = [
            "dev",
            "latest",
            "16.13-1",
            "16.14-1",
            "16.14-1-alpine",
            "17.0-1",
            "16.9-1",
        ]
        .iter()
        .map(|t| t.to_string())
        .collect();

        assert_eq!(newer_version_tag("16.13-1", &available), Some("16.14-1"));
        // Already on the newest of its major: nothing to offer, and 17 is not an offer.
        assert_eq!(newer_version_tag("16.14-1", &available), None);
        // An alpine hub stays on alpine.
        assert_eq!(
            newer_version_tag("16.13-1-alpine", &available),
            Some("16.14-1-alpine")
        );
        // A channel is not a version, so there is nothing to order it against.
        assert_eq!(newer_version_tag("dev", &available), None);
        // Nor is MinIO's release stamp — and being unable to say so is the right answer.
        assert_eq!(
            newer_version_tag("RELEASE.2025-02-18T16-25-55Z", &available),
            None
        );
    }

    #[test]
    fn a_version_is_the_numbers_a_tag_starts_with() {
        assert_eq!(version_of("16.13-1").expect("a version").parts, vec![16, 13, 1]);
        assert_eq!(version_of("8.11.0-alpine").expect("a version").variant, "alpine");
        assert!(version_of("dev").is_none());
        assert!(version_of("RELEASE.2025-02-18T16-25-55Z").is_none());
    }

    /// The split has to come from the profile rather than a list of names, so that a
    /// service added upstream is classified without this being edited.
    #[test]
    fn services_are_not_infrastructure_and_everything_else_is() {
        use crate::config::hub::{build_hub_config, HubConfigOptions};
        let config = build_hub_config(&HubConfigOptions {
            device_id: "device".into(),
            coord_server: "go.arkitekt.live".into(),
            ..Default::default()
        });

        for id in config.enabled_services() {
            let host = config.service(id).host.clone();
            assert!(
                !is_infrastructure(&config, &host),
                "{host} is one of the hub's own services"
            );
        }
        for infra in ["db", "redis", "minio", "minio_init", "gateway"] {
            assert!(is_infrastructure(&config, infra), "{infra} is infrastructure");
        }
    }

    /// Everything but the database is waved through, and the database is never waved
    /// through on a version nobody could read — an unverifiable major is a warning, not a
    /// pass.
    #[tokio::test]
    async fn only_the_database_is_guarded_and_never_silently() {
        use crate::config::hub::{build_hub_config, HubConfigOptions};
        let mut config = build_hub_config(&HubConfigOptions {
            device_id: "device".into(),
            coord_server: "go.arkitekt.live".into(),
            ..Default::default()
        });
        let dir = std::env::temp_dir().join(format!("konstruktor-guard-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).expect("a scratch folder");

        assert_eq!(guard(&dir, &config, "rekuest").await, Guard::Clear);
        assert_eq!(guard(&dir, &config, "gateway").await, Guard::Clear);

        // A folder-mode database with no cluster on disk yet: nothing to read, so nothing
        // to claim.
        config.db.mount = Some("./db_data".into());
        assert!(
            matches!(guard(&dir, &config, "db").await, Guard::Warn(_)),
            "an unreadable major must not report as clear"
        );
    }

    #[test]
    fn a_digest_pin_is_read_off_the_reference() {
        assert_eq!(
            pinned_digest("jhnnsrs/daten:dev@sha256:abc"),
            Some("sha256:abc")
        );
        assert_eq!(pinned_digest("caddy:2.11.4"), None);
    }

    #[test]
    fn digest_strips_repo() {
        assert_eq!(digest_of("jhnnsrs/rekuest@sha256:abc"), "sha256:abc");
    }
}
