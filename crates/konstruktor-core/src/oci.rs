//! Reading what an image says of itself without pulling it: its labels, from the image's
//! config in the registry — a few kilobytes, where pulling is the whole image.
//!
//! A service's release build stamps its description into the image as a label
//! (`live.arkitekt.service`, see [`crate::contract::LABEL`]), so an installer can know what a
//! service is before anybody has agreed to download gigabytes of it. An image already on
//! this machine is read where it is; one that is not is read from its registry, anonymously,
//! the way [`crate::updates`] looks up tags. What was read is kept by digest, so asking again
//! costs one `HEAD` — which a registry does not count against its pull limits.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

use crate::updates::{parse, Reference};

const TIMEOUT: Duration = Duration::from_secs(15);

/// A manifest of either kind: one image, or the index of one per platform.
const ACCEPT: &str = "application/vnd.oci.image.index.v1+json, \
application/vnd.docker.distribution.manifest.list.v2+json, \
application/vnd.oci.image.manifest.v1+json, \
application/vnd.docker.distribution.manifest.v2+json";

/// The labels of `image`: from the engine when the image is on this machine, else from its
/// registry. `None` when neither can say — the image is not here and the registry cannot be
/// reached, or does not have it.
pub async fn labels(image: &str) -> Option<BTreeMap<String, String>> {
    if let Some(local) = local_labels(image).await {
        return Some(local);
    }
    remote_labels(image).await.ok()
}

/// The labels of an image on this machine, as the engine reads them. `None` when it is not
/// here.
async fn local_labels(image: &str) -> Option<BTreeMap<String, String>> {
    let output = crate::engine_probe::engine()
        .async_command()
        .args([
            "image",
            "inspect",
            "--format",
            "{{json .Config.Labels}}",
            image,
        ])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()
        .filter(|output| output.status.success())?;
    // An image without labels says `null`.
    let labels: Option<BTreeMap<String, String>> = serde_json::from_slice(&output.stdout).ok()?;
    Some(labels.unwrap_or_default())
}

/// The labels of `image` as its registry has them: the manifest the reference resolves to,
/// the one for this machine's platform if that is an index, and the config it names.
pub async fn remote_labels(image: &str) -> Result<BTreeMap<String, String>, String> {
    let mut registry = Registry::new(image)?;
    let digest = match pinned(image) {
        Some(digest) => digest.to_string(),
        None => registry.digest(&registry.reference.tag.clone()).await?,
    };
    if let Some(cached) = cached(&digest) {
        return Ok(cached);
    }

    let mut manifest: Manifest = registry.manifest(&digest).await?;
    if let Some(entries) = manifest.manifests.take() {
        let (os, architecture) = host_platform();
        let chosen = entries
            .iter()
            .find(|entry| {
                entry.platform.as_ref().is_some_and(|platform| {
                    platform.os == os && platform.architecture == architecture
                })
            })
            .ok_or_else(|| format!("{image} is not built for {os}/{architecture}"))?;
        manifest = registry.manifest(&chosen.digest).await?;
    }
    let config = manifest
        .config
        .ok_or_else(|| format!("the manifest of {image} names no config"))?;

    #[derive(Deserialize)]
    struct ImageConfig {
        #[serde(default)]
        config: Option<Inner>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Inner {
        #[serde(default)]
        labels: Option<BTreeMap<String, String>>,
    }
    let blob = registry.blob(&config.digest).await?;
    let read: ImageConfig = serde_json::from_slice(&blob).map_err(|e| e.to_string())?;
    let labels = read.config.and_then(|c| c.labels).unwrap_or_default();
    remember(&digest, &labels);
    Ok(labels)
}

/// A manifest, of either kind, as far as reading labels needs it.
#[derive(Debug, Deserialize)]
pub(crate) struct Manifest {
    /// An index: one entry per platform.
    #[serde(default)]
    pub manifests: Option<Vec<Entry>>,
    /// An image: its config.
    #[serde(default)]
    pub config: Option<Descriptor>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Entry {
    pub digest: String,
    #[serde(default)]
    pub platform: Option<Platform>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Platform {
    pub os: String,
    pub architecture: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Descriptor {
    pub digest: String,
}

/// One repository of a registry, asked anonymously: the first answer that wants a token
/// says where to get one, and every later request carries it.
pub(crate) struct Registry {
    client: reqwest::Client,
    pub reference: Reference,
    token: Option<String>,
}

impl Registry {
    pub fn new(image: &str) -> Result<Self, String> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(TIMEOUT)
                .build()
                .map_err(|e| e.to_string())?,
            reference: parse(image),
            token: None,
        })
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}://{}/v2/{}/{path}",
            scheme(&self.reference.host),
            self.reference.host,
            self.reference.repository
        )
    }

    async fn send(
        &mut self,
        method: reqwest::Method,
        path: &str,
        accept: Option<&str>,
    ) -> Result<reqwest::Response, String> {
        let url = self.url(path);
        let request = |token: Option<&str>| {
            let mut request = self.client.request(method.clone(), &url);
            if let Some(accept) = accept {
                request = request.header("Accept", accept);
            }
            if let Some(token) = token {
                request = request.bearer_auth(token);
            }
            request
        };
        let first = request(self.token.as_deref())
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let response = if first.status() == reqwest::StatusCode::UNAUTHORIZED {
            let challenge = first
                .headers()
                .get("www-authenticate")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| "registry asked for auth without saying how".to_string())?
                .to_string();
            let token = crate::updates::token(&self.client, &challenge, &self.reference).await?;
            let response = request(Some(&token))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            self.token = Some(token);
            response
        } else {
            first
        };
        if !response.status().is_success() {
            return Err(format!(
                "registry answered {} for {path}",
                response.status()
            ));
        }
        Ok(response)
    }

    /// The digest `tag` resolves to, by `HEAD`: no registry counts that as a pull.
    pub async fn digest(&mut self, tag: &str) -> Result<String, String> {
        let response = self
            .send(
                reqwest::Method::HEAD,
                &format!("manifests/{tag}"),
                Some(ACCEPT),
            )
            .await?;
        response
            .headers()
            .get("docker-content-digest")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .ok_or_else(|| "registry did not report a digest".to_string())
    }

    /// The manifest at `reference` (a tag or a digest), of either kind.
    pub async fn manifest<T: serde::de::DeserializeOwned>(
        &mut self,
        reference: &str,
    ) -> Result<T, String> {
        self.send(
            reqwest::Method::GET,
            &format!("manifests/{reference}"),
            Some(ACCEPT),
        )
        .await?
        .json()
        .await
        .map_err(|e| e.to_string())
    }

    /// A blob, by digest: an image's config, or a document published as one.
    pub async fn blob(&mut self, digest: &str) -> Result<Vec<u8>, String> {
        let bytes = self
            .send(reqwest::Method::GET, &format!("blobs/{digest}"), None)
            .await?
            .bytes()
            .await
            .map_err(|e| e.to_string())?;
        Ok(bytes.to_vec())
    }
}

/// How a registry is spoken to: plain HTTP on this machine, as the engine does it, and TLS
/// everywhere else.
fn scheme(host: &str) -> &'static str {
    let name = host.rsplit_once(':').map_or(host, |(name, _)| name);
    if name == "localhost" || name == "127.0.0.1" || name == "[::1]" {
        "http"
    } else {
        "https"
    }
}

/// The digest an image reference pins itself to, if it names one.
fn pinned(image: &str) -> Option<&str> {
    image.split_once('@').map(|(_, digest)| digest)
}

/// This machine's platform, as an image index names platforms.
pub(crate) fn host_platform() -> (&'static str, &'static str) {
    let architecture = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "arm" => "arm",
        other => other,
    };
    ("linux", architecture)
}

/// Where labels read from a registry are kept, by the digest they were read at: a digest
/// names one manifest forever, so what was read at it never goes stale.
fn cache_path(digest: &str) -> Option<PathBuf> {
    let name: String = digest
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    crate::registry::registry_dir().map(|dir| dir.join("image-labels").join(format!("{name}.json")))
}

fn cached(digest: &str) -> Option<BTreeMap<String, String>> {
    let text = std::fs::read(cache_path(digest)?).ok()?;
    serde_json::from_slice(&text).ok()
}

fn remember(digest: &str, labels: &BTreeMap<String, String>) {
    let Some(path) = cache_path(digest) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_vec(labels) {
        let _ = std::fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_registry_on_this_machine_is_spoken_to_over_plain_http() {
        assert_eq!(scheme("localhost:5000"), "http");
        assert_eq!(scheme("127.0.0.1:41233"), "http");
        assert_eq!(scheme("registry-1.docker.io"), "https");
        assert_eq!(scheme("ghcr.io"), "https");
    }

    #[test]
    fn a_digest_is_a_file_name() {
        let path = cache_path("sha256:abc").expect("a data dir");
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("sha256-abc.json")
        );
    }
}
