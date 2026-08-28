use std::collections::HashMap;
use std::fs::canonicalize;
use std::process::Command;
use std::time::Duration;

use bollard::query_parameters::ListContainersOptionsBuilder;
use bollard::Docker;
use serde::{Deserialize, Serialize};

/// Everything Konstruktor needs to know about Docker on this machine.
///
/// Both front ends share it: the wizard's first step and the CLI's `doctor` reach the
/// same verdict from the same probe.
fn docker_command() -> String {
    "docker".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Container {
    pub id: Option<String>,
    pub names: Option<Vec<String>>,
    /// The image reference the container was created from, as written in the compose
    /// file — a tag, e.g. `jhnnsrs/rekuest:next`.
    pub image: Option<String>,
    /// The resolved id of that image *at the moment the container was created*. It stops
    /// matching the tag's current id as soon as a newer image is pulled over the tag,
    /// which is how the dashboard knows an update is waiting to be applied.
    pub image_id: Option<String>,
    pub labels: Option<HashMap<String, String>>,
    pub status: Option<String>,
    pub state: Option<String>,
    /// `com.docker.compose.service` — what the dashboard groups by.
    pub service: Option<String>,
}

/// What we found when we looked for Docker.
///
/// Every field is answered independently, because the three ways this can go wrong have
/// three different remedies: no CLI means "install Docker", a CLI without the compose
/// plugin means "install a newer Docker", and a CLI whose daemon does not answer means
/// "start Docker". Nothing here panics — "Docker is missing" is the ordinary case this
/// exists to report, not an error.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct DockerProbe {
    /// The `docker` binary is on PATH.
    pub cli: bool,
    /// `docker --version`, e.g. "27.3.1".
    pub cli_version: Option<String>,
    /// `docker compose` is available — it is a plugin, and the CLI can exist without it.
    pub compose: bool,
    /// `docker compose version --short`, e.g. "2.29.7".
    pub compose_version: Option<String>,
    /// The daemon answered over the local socket. Required to *run* anything.
    pub daemon: bool,
    /// The Engine API version the daemon reports.
    pub api_version: Option<String>,
    /// Total memory the daemon sees, in bytes.
    pub memory: Option<i64>,
    /// Why the daemon could not be reached, when it could not.
    pub error: Option<String>,
}

/// Docker reduced to the one thing a UI has to decide: what to tell the user next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DockerState {
    Ready,
    /// The `docker` binary is not there at all — offer an install.
    Missing,
    /// The CLI is present but `docker compose` is not — offer a newer Docker.
    NoCompose,
    /// Everything is installed but the daemon is silent — say "start Docker".
    NoDaemon,
}

impl DockerProbe {
    pub fn state(&self) -> DockerState {
        if !self.cli {
            DockerState::Missing
        } else if !self.compose {
            DockerState::NoCompose
        } else if !self.daemon {
            DockerState::NoDaemon
        } else {
            DockerState::Ready
        }
    }

    pub fn is_ready(&self) -> bool {
        self.state() == DockerState::Ready
    }
}

/// The arguments for a throwaway container that gives a tree back to its owner.
///
/// The Docker daemon runs as root and creates bind-mount targets as root, so the data
/// directories a hub keeps inside its folder end up owned by root and by whatever uid the
/// container wrote as. A desktop user cannot then delete them. The daemon is the only
/// thing on this machine with the authority to undo that, so we borrow it: a container
/// that does nothing but `chown`, over a mount of the tree in question.
///
/// It chowns rather than deletes, on purpose. The removal stays on the host, where every
/// guard in `destroy` still applies; a container running `rm -rf` as root over a
/// caller-supplied path is a far worse thing to get wrong.
///
/// `None` when the path cannot be expressed as a `--mount` argument — see below. The
/// caller reports that rather than inventing an escaping scheme.
///
/// Why each flag:
///
/// * `--mount` rather than `-v`, because `-v host:/target` splits on colons and a
///   deployment folder is named by the user. `--mount` has its own unquotable character,
///   the comma, which is what the `None` is for.
/// * `--entrypoint chown`, because every candidate image has an entrypoint of its own.
///   Docker resolves it through the container's `PATH`, so the bare name avoids guessing
///   between `/bin/chown` and `/usr/bin/chown`.
/// * `-Rh`: `-R` for the tree, `-h` to act on a symlink itself. Both GNU and busybox
///   `chown -R` default to `-P`, so it will not follow a link out of `/target`.
/// * `--user 0:0`, because an image is free to declare a non-root `USER`, and the whole
///   point is to act as root.
/// * `--network none` and `--read-only`, because nothing here needs either. A bind mount
///   stays writable under `--read-only`, which covers only the container's own layer.
/// * `--pull=never`, so a destructive action the user is watching can never turn into a
///   silent image download over a slow link.
pub fn chown_args(host_path: &str, image: &str, uid: u32, gid: u32) -> Option<Vec<String>> {
    // `--mount` takes comma-separated key=value pairs and offers no quoting, so a comma in
    // the path would be read as the start of another option. A newline is refused for the
    // same reason: it cannot survive the round trip intact.
    if host_path.contains(',') || host_path.contains('\n') {
        return None;
    }

    Some(
        [
            "run",
            "--rm",
            "--network",
            "none",
            "--read-only",
            "--pull=never",
            "--user",
            "0:0",
            "--entrypoint",
            "chown",
            "--mount",
            &format!("type=bind,source={host_path},target=/target"),
            image,
            "-Rh",
            &format!("{uid}:{gid}"),
            "/target",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    )
}

/// Whether an image is already on this machine.
///
/// `docker image inspect` rather than reading a `docker run` failure for "Unable to find
/// image": the exit status is the same answer, and it does not depend on the daemon's
/// locale or on wording that changes between versions.
pub fn image_present(image: &str) -> bool {
    Command::new(docker_command())
        .args(["image", "inspect", image])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Runs a command that must not touch the Docker daemon, so it stays fast and cannot
/// hang. `None` means the binary could not be executed at all.
fn probe_command(args: &[&str]) -> Option<String> {
    let output = Command::new(docker_command()).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// The version out of a `docker … version` banner, wherever in the line it sits.
///
/// The two banners put it in different places — `Docker version 27.3.1, build ce122…`
/// against `Docker Compose version v5.1.3` — so this looks for the first token that
/// actually looks like a version rather than counting words. Counting words is what the
/// first cut did, and it reported Compose's version as the literal string "version".
pub(crate) fn parse_cli_version(line: &str) -> Option<String> {
    line.split_whitespace()
        .map(|token| token.trim_end_matches(','))
        .find(|token| {
            let digits = token.strip_prefix('v').unwrap_or(token);
            digits.starts_with(|c: char| c.is_ascii_digit())
        })
        .map(str::to_string)
}

pub async fn probe() -> DockerProbe {
    let mut probe = DockerProbe::default();

    // `docker --version` and `docker compose version` are answered by the CLI itself;
    // neither needs a running daemon, so a stopped Docker Desktop still reports both.
    if let Some(line) = probe_command(&["--version"]) {
        probe.cli = true;
        probe.cli_version = parse_cli_version(&line);
    }

    if probe.cli {
        // `--short` is not understood by the earliest Compose v2 builds, and a failed
        // parse there would report a working Compose as missing — which is a hard block
        // with a download link attached. Plain `compose version` is the fallback.
        if let Some(version) = probe_command(&["compose", "version", "--short"]) {
            probe.compose = true;
            probe.compose_version = Some(version);
        } else if let Some(line) = probe_command(&["compose", "version"]) {
            probe.compose = true;
            probe.compose_version = parse_cli_version(&line);
        }
    }

    // The daemon is a separate question, and the only one that can hang: a socket that
    // exists but is not being served makes bollard wait. The timeout keeps a broken
    // Docker installation from freezing the check.
    match Docker::connect_with_local_defaults() {
        Ok(docker) => {
            let docker = docker.with_timeout(Duration::from_secs(5));
            match docker.version().await {
                Ok(version) => {
                    probe.daemon = true;
                    probe.api_version = version.api_version;
                    probe.memory = docker.info().await.ok().and_then(|info| info.mem_total);
                }
                Err(e) => probe.error = Some(e.to_string()),
            }
        }
        Err(e) => probe.error = Some(e.to_string()),
    }

    probe
}

/// The containers belonging to the compose project in `path`.
///
/// The generated stack carries no `arkitekt.*` labels — it is a plain compose project —
/// so its containers are identified by the directory compose was run in, which stays
/// stable even when two deployments would derive the same project name.
pub async fn list_deployment_containers(path: &str) -> Result<Vec<Container>, String> {
    let docker = Docker::connect_with_local_defaults().map_err(|e| e.to_string())?;

    let dir = canonicalize(path).map_err(|e| e.to_string())?;
    let working_dir = format!(
        "com.docker.compose.project.working_dir={}",
        dir.to_string_lossy()
    );

    let mut filters = HashMap::new();
    filters.insert("label".to_string(), vec![working_dir]);

    let options = ListContainersOptionsBuilder::new()
        .all(true)
        .filters(&filters)
        .build();

    let containers = docker
        .list_containers(Some(options))
        .await
        .map_err(|e| e.to_string())?;

    Ok(containers
        .into_iter()
        .map(|c| Container {
            service: c
                .labels
                .as_ref()
                .and_then(|l| l.get("com.docker.compose.service").cloned()),
            id: c.id,
            names: c.names,
            image: c.image,
            image_id: c.image_id,
            status: c.status,
            labels: c.labels,
            state: c.state.map(|state| state.to_string()),
        })
        .collect())
}

/// What the local Docker daemon currently holds for one image reference.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImageState {
    /// The reference as the compose file spells it, e.g. `jhnnsrs/rekuest:next`.
    pub image: String,
    /// The compose service this image belongs to, so the UI can line it up with the
    /// containers it already lists.
    pub service: String,
    /// Whether the daemon has it at all. `false` means nothing has pulled it yet.
    pub present: bool,
    /// The id the tag resolves to *now*. Compared against a running container's
    /// `image_id` to tell "a newer image is pulled but not running yet".
    pub image_id: Option<String>,
    /// When that image was built, as the daemon reports it.
    pub created: Option<String>,
}

/// Resolves every image the stack declares against the local daemon.
///
/// Nothing here pulls or contacts a registry: this answers "what is on this machine",
/// which is all that is needed to spot an update that was downloaded but never applied.
/// Whether something *newer* exists upstream is a different question, and a registry
/// query this deliberately does not make.
pub async fn image_states(images: &[(String, String)]) -> Result<Vec<ImageState>, String> {
    let docker = Docker::connect_with_local_defaults().map_err(|e| e.to_string())?;
    let docker = docker.with_timeout(Duration::from_secs(10));

    let mut states = Vec::with_capacity(images.len());
    for (service, image) in images {
        // A missing image is the ordinary case before the first pull, not an error.
        let inspected = docker.inspect_image(image).await.ok();
        states.push(ImageState {
            image: image.clone(),
            service: service.clone(),
            present: inspected.is_some(),
            image_id: inspected.as_ref().and_then(|i| i.id.clone()),
            created: inspected.as_ref().and_then(|i| i.created.clone()),
        });
    }
    Ok(states)
}

pub async fn restart_container(container_id: &str) -> Result<(), String> {
    let docker = Docker::connect_with_local_defaults().map_err(|e| e.to_string())?;
    docker
        .restart_container(container_id, None)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both banners, because they put the version in different positions — the reason
    /// this looks for a version-shaped token instead of counting words.
    #[test]
    fn reads_the_version_out_of_either_docker_banner() {
        assert_eq!(
            parse_cli_version("Docker version 27.3.1, build ce1223035a").as_deref(),
            Some("27.3.1")
        );
        assert_eq!(
            parse_cli_version("Docker Compose version v5.1.3").as_deref(),
            Some("v5.1.3")
        );
        assert_eq!(parse_cli_version("nothing version-shaped here"), None);
    }

    #[test]
    fn each_failure_gets_its_own_verdict() {
        let ready = DockerProbe {
            cli: true,
            compose: true,
            daemon: true,
            ..Default::default()
        };
        assert_eq!(ready.state(), DockerState::Ready);

        assert_eq!(DockerProbe::default().state(), DockerState::Missing);
        assert_eq!(
            DockerProbe {
                cli: true,
                ..Default::default()
            }
            .state(),
            DockerState::NoCompose
        );
        assert_eq!(
            DockerProbe {
                cli: true,
                compose: true,
                ..Default::default()
            }
            .state(),
            DockerState::NoDaemon
        );
    }

    /// A missing binary is reported ahead of a silent daemon: sending somebody whose
    /// Docker is merely stopped to a download page wastes their time.
    #[test]
    fn a_missing_binary_outranks_a_silent_daemon() {
        assert_eq!(DockerProbe::default().state(), DockerState::Missing);
    }
}
