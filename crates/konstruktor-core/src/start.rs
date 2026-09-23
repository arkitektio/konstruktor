//! Starting a hub — the one sequence every front end runs, so none of them can leave a
//! step out.
//!
//! In order:
//!
//! 1. **The database guard.** `compose up` applies whatever the database image reference
//!    resolves to *now*, and Postgres will not open a cluster from another major. A
//!    refusal stops here; a warning is handed back to be shown, never dropped.
//! 2. **The mesh sidecar's networks.** A `compose up` that failed after creating the
//!    containers — a pull that was refused, say — can leave the sidecar attached to only
//!    one of its networks. It then starts, joins the tailnet and looks healthy, while the
//!    gateway inside its namespace cannot resolve a single service. A sidecar with fewer
//!    networks than its compose entry declares is removed together with the gateway, so
//!    the `up` below creates both afresh.
//! 3. **`up`, without the reporter if its image cannot be had** — see
//!    [`crate::compose::up_in`].

use std::path::Path;

use serde::Serialize;

use crate::compose::{self, ComposeLine};
use crate::config::hub::{HubConfig, DB_COMPOSE_SERVICE};
use crate::updates::{guard, Guard};

/// What a start had to say besides compose's own output.
#[derive(Debug, Clone, Default, Serialize)]
pub struct StartReport {
    /// Things worth telling the person, none of which stopped the start.
    pub warnings: Vec<String>,
    /// The sidecar and gateway were recreated because the sidecar had lost a network.
    pub repaired_mesh: bool,
    /// What compose wrote on stdout.
    pub output: String,
}

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    /// The database guard refused: starting would break the database.
    #[error("{0}")]
    Refused(String),
    #[error("{0}")]
    Compose(String),
}

/// Starts the hub in `dir`, narrating over `on_line`. A folder without a hub profile — a
/// plugin engine — skips the hub-specific steps and is simply brought up.
pub async fn start(
    dir: &Path,
    on_line: &(dyn Fn(ComposeLine) + Send + Sync),
) -> Result<StartReport, StartError> {
    let mut report = StartReport::default();
    let say = |line: &str| {
        on_line(ComposeLine {
            line: line.to_string(),
            stderr: true,
        })
    };

    let config = crate::profile::read_profile(dir).ok().map(|p| p.config);

    // --- 0. networks this project joins but does not own ------------------------------
    // An engine attached to a hub joins that hub's network, which only exists while the
    // hub is up. Compose would fail on it with a bare "network not found"; this names the
    // hub to start first.
    if let Some(network) = missing_external_network(dir).await {
        let owner = crate::registry::load()
            .deployments
            .into_iter()
            .find(|record| {
                record.kind == "hub"
                    && crate::profile::read_profile(Path::new(&record.path))
                        .is_ok_and(|p| p.config.internal_network == network)
            })
            .map(|record| record.name);
        return Err(StartError::Refused(match owner {
            Some(hub) => format!(
                "this is attached to the hub {hub}, whose network `{network}` does not exist \
                 until {hub} is started — start {hub} first"
            ),
            None => format!(
                "this joins the network `{network}`, which does not exist — start the hub it \
                 belongs to first, or detach from it"
            ),
        }));
    }

    if let Some(config) = &config {
        // --- 1. the database -------------------------------------------------------
        match guard(dir, config, DB_COMPOSE_SERVICE).await {
            Guard::Refuse(reason) => return Err(StartError::Refused(reason)),
            Guard::Warn(detail) => {
                say(&detail);
                report.warnings.push(detail);
            }
            Guard::Clear => {}
        }

        // --- 2. the mesh sidecar ---------------------------------------------------
        if let Some(stale) = sidecar_missing_a_network(dir, config).await {
            let line = format!(
                "The mesh sidecar is missing a network ({stale}); recreating it and the gateway."
            );
            say(&line);
            report.warnings.push(line);
            let mesh = config.mesh.as_ref().expect("a sidecar implies a mesh");
            let remove = vec![
                "compose".to_string(),
                "rm".to_string(),
                "--stop".to_string(),
                "--force".to_string(),
                mesh.host.clone(),
                config.gateway.host.clone(),
            ];
            compose::run_streamed(dir, remove, on_line)
                .await
                .map_err(StartError::Compose)?;
            report.repaired_mesh = true;
        }
    }

    // --- 3. up -------------------------------------------------------------------
    let (args, left_out) = compose::up_in(dir).await;
    if let Some(reason) = left_out {
        say(&reason);
        report.warnings.push(reason);
    }
    report.output = compose::run_streamed(dir, args, on_line)
        .await
        .map_err(StartError::Compose)?;
    Ok(report)
}

/// When the sidecar container exists but is attached to fewer networks than its compose
/// entry declares: how many it has out of how many. `None` when all is well, when there is
/// no mesh, or when there is no container yet — `up` will create it correctly.
pub async fn sidecar_missing_a_network(dir: &Path, config: &HubConfig) -> Option<String> {
    let mesh = config.mesh.as_ref().filter(|m| m.enabled)?;

    let declared = declared_networks(dir, &mesh.host)?;
    if declared == 0 {
        return None;
    }

    let engine = crate::engine_probe::engine();
    let id = engine
        .async_command()
        .args(["compose", "ps", "--all", "--quiet", &mesh.host])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|id| !id.is_empty())?;

    let networks = engine
        .async_command()
        .args(["inspect", "--format", "{{json .NetworkSettings.Networks}}", &id])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()
        .filter(|o| o.status.success())?;
    let attached = serde_json::from_slice::<serde_json::Value>(&networks.stdout)
        .ok()?
        .as_object()
        .map(|m| m.len())?;

    (attached < declared).then(|| format!("attached to {attached} of {declared}"))
}

/// The first network the compose file declares `external` that the engine does not have.
pub async fn missing_external_network(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("docker-compose.yaml")).ok()?;
    let doc: serde_norway::Value = serde_norway::from_str(&text).ok()?;
    let networks = doc.get("networks")?.as_mapping()?;
    for (key, network) in networks {
        if network.get("external").and_then(|e| e.as_bool()) != Some(true) {
            continue;
        }
        let name = network
            .get("name")
            .and_then(|n| n.as_str())
            .or_else(|| key.as_str())?
            .to_string();
        let exists = crate::engine_probe::engine()
            .async_command()
            .args(["network", "inspect", &name])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .is_ok_and(|s| s.success());
        if !exists {
            return Some(name);
        }
    }
    None
}

/// How many networks the compose file on disk gives one service.
fn declared_networks(dir: &Path, service: &str) -> Option<usize> {
    let text = std::fs::read_to_string(dir.join("docker-compose.yaml")).ok()?;
    let doc: serde_norway::Value = serde_norway::from_str(&text).ok()?;
    let networks = doc.get("services")?.get(service)?.get("networks")?;
    Some(match networks {
        serde_norway::Value::Mapping(m) => m.len(),
        serde_norway::Value::Sequence(s) => s.len(),
        _ => 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_the_networks_a_compose_entry_declares() {
        let dir = std::env::temp_dir().join(format!("konstruktor-start-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("docker-compose.yaml"),
            "services:\n  tailscale:\n    networks:\n      young-dream: {aliases: [gateway]}\n      default: {aliases: [gateway]}\n  gateway:\n    network_mode: service:tailscale\n",
        )
        .unwrap();
        assert_eq!(declared_networks(&dir, "tailscale"), Some(2));
        assert_eq!(declared_networks(&dir, "gateway"), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
