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
    pub image: Option<String>,
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
            status: c.status,
            labels: c.labels,
            state: c.state.map(|state| state.to_string()),
        })
        .collect())
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
