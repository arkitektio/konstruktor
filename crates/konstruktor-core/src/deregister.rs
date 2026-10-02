//! A hub removing itself from its coordination server, as the last thing it does.
//!
//! The server deletes a hub for whoever holds that hub's own access token, so this logs
//! in the way the `reporter` does — see [`crate::hubhealth`] — and asks. What makes it
//! more than one request is where the login lives. The refresh token rotates on every
//! use, and the one that still works is in the reporter's volume, not in
//! `hub_credentials.json`: replaying the stale one makes the server revoke the whole
//! chain and reap the hub's mesh node. So the reporter is stopped, its state is read out
//! of the volume, and the token that comes back from the refresh is written into the
//! volume again **before** it is used — a destroy that is then refused must leave a hub
//! that can still report.
//!
//! Between the refresh and that write the only working token is on this side of the
//! volume. It is kept in [`RESCUE_DIR`] inside the deployment folder for exactly that
//! long, so neither a failed write nor a killed process loses it.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use tokio::io::AsyncWriteExt;

use crate::compose;
use crate::config::hub::ReporterBlock;
use crate::connect::wellknown;
use crate::credentials::{credentials_path, read_credentials, HubCredentials};
use crate::hubhealth::{self, ReportError, ReporterState, STATE_FILENAME};

/// Where the rotated login waits, inside the deployment folder, until the reporter's
/// volume has it.
pub const RESCUE_DIR: &str = ".reporter-rescue";
/// What `hub_credentials.json` is renamed to once the server has let go of the hub, so
/// that a delete which then fails locally can be run again without asking a server that
/// no longer knows this login.
pub const DEREGISTERED_FILENAME: &str = "hub_credentials.deregistered.json";

/// Long enough for a slow server, short enough that a dead one does not look like a hang.
const TIMEOUT: Duration = Duration::from_secs(20);

/// What became of the hub's registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerOutcome {
    /// The server deleted the hub.
    Removed,
    /// The server knows the login but no hub behind it: somebody removed it there first.
    AlreadyGone,
    /// There was never anything to remove — the hub was not authorized, or this is not a hub.
    NotRegistered,
    /// Nobody asked: the caller chose to delete locally only.
    LeftRegistered,
}

#[derive(Debug, thiserror::Error)]
pub enum DeregisterError {
    #[error("the coordination server at {0} did not answer")]
    Unreachable(String),
    #[error("the coordination server declares no token endpoint")]
    NoTokenEndpoint,
    #[error("this coordination server cannot remove hubs yet ({0})")]
    Unsupported(String),
    #[error(
        "the coordination server refused the hub's login ({0}) — it was replaced, revoked, \
         or the hub is already gone there"
    )]
    InvalidGrant(String),
    #[error("the coordination server did not accept the hub's token ({0})")]
    Unauthorized(String),
    #[error("the coordination server answered {status}: {detail}")]
    Server { status: u16, detail: String },
    #[error(
        "the hub's login was renewed but could not be put back into the reporter's volume \
         ({detail}). It is kept in {rescue}, and the reporter was left stopped: started \
         now, it would use a login the server has already replaced. Run the delete again."
    )]
    WriteBack { rescue: String, detail: String },
    #[error("docker could not be used to reach the hub's login: {0}")]
    Docker(String),
    #[error(transparent)]
    Discovery(#[from] wellknown::CoordinationServerError),
    #[error(transparent)]
    Transport(#[from] reqwest::Error),
    #[error("could not keep the hub's login: {0}")]
    State(#[from] std::io::Error),
}

impl DeregisterError {
    /// The failure was on this machine — Docker, or the folder — and the server was never
    /// the one to say no. Deleting locally regardless is no answer to these.
    pub fn is_local(&self) -> bool {
        matches!(
            self,
            Self::Docker(_) | Self::WriteBack { .. } | Self::State(_)
        )
    }
}

impl From<ReportError> for DeregisterError {
    fn from(error: ReportError) -> Self {
        match error {
            ReportError::InvalidGrant(detail) => Self::InvalidGrant(detail),
            ReportError::Unauthorized(detail) => Self::Unauthorized(detail),
            ReportError::Server { status, detail } => Self::Server { status, detail },
            ReportError::NoTokenEndpoint => Self::NoTokenEndpoint,
            ReportError::Discovery(e) => Self::Discovery(e),
            ReportError::Transport(e) => Self::Transport(e),
            ReportError::State(e) => Self::State(e),
            // Neither is reachable from here — the seed was read and has a refresh token
            // before anything that could say so is called — but both mean the same thing.
            other @ (ReportError::NoCredentials(_) | ReportError::NoRefreshToken) => {
                Self::InvalidGrant(other.to_string())
            }
        }
    }
}

// --- the server ----------------------------------------------------------------------

/// Where a hub deletes itself: a declared `hub_deletion_endpoint`, or the sibling of the
/// token endpoint — `…/lok/o/token/` → `…/lok/f/hubdelete/`.
pub fn deletion_endpoint(well_known: &wellknown::WellKnownFakts) -> Option<String> {
    if let Some(declared) = well_known
        .extra
        .get("hub_deletion_endpoint")
        .and_then(|v| v.as_str())
    {
        return Some(declared.to_string());
    }
    let token = well_known.token_endpoint.as_deref()?;
    let base = token.trim_end_matches('/').strip_suffix("o/token")?;
    Some(format!("{base}f/hubdelete/"))
}

/// Asks the server to delete the hub this token belongs to.
///
/// A 404 is two different answers. With `hub_not_found` it is the endpoint saying the
/// hub is already gone; without, it is a server that has no such endpoint — and taking
/// that for "gone" would delete a hub locally that is still listed.
pub async fn remove_hub(
    client: &reqwest::Client,
    endpoint: &str,
    access_token: &str,
) -> Result<ServerOutcome, DeregisterError> {
    let response = client
        .post(endpoint)
        .bearer_auth(access_token)
        .header("Accept", "application/json")
        .send()
        .await?;
    let status = response.status().as_u16();
    let body: serde_json::Value = response.json().await.unwrap_or(serde_json::Value::Null);
    let code = body.get("error").and_then(|v| v.as_str());
    let detail = body
        .get("error_description")
        .and_then(|v| v.as_str())
        .or(code)
        .map(str::to_string)
        .unwrap_or_else(|| status.to_string());

    match status {
        200..=299 => Ok(ServerOutcome::Removed),
        404 if code == Some("hub_not_found") => Ok(ServerOutcome::AlreadyGone),
        404 | 405 => Err(DeregisterError::Unsupported(format!(
            "{endpoint} answered {status}"
        ))),
        401 => Err(DeregisterError::Unauthorized(detail)),
        _ => Err(DeregisterError::Server { status, detail }),
    }
}

/// Renews the hub's login and answers the access token, with the rotated state on disk
/// in `state_dir` before it returns.
pub async fn fresh_access_token(
    client: &reqwest::Client,
    token_endpoint: &str,
    seed: &HubCredentials,
    stored: Option<ReporterState>,
    state_dir: &Path,
) -> Result<(String, ReporterState), DeregisterError> {
    let mut state = hubhealth::reconcile(seed, stored)?;
    let access = hubhealth::refresh(client, token_endpoint, &mut state, state_dir).await?;
    Ok((access.token, state))
}

// --- the reporter's volume -----------------------------------------------------------

async fn compose(
    dir: &Path,
    args: Vec<String>,
    stdin: Option<&[u8]>,
) -> Result<Vec<u8>, DeregisterError> {
    let mut child = crate::engine_probe::engine()
        .async_command()
        .args(&args)
        .current_dir(dir)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| DeregisterError::Docker(format!("could not run docker: {e}")))?;
    if let Some(input) = stdin {
        let mut pipe = child.stdin.take().expect("stdin was piped");
        pipe.write_all(input)
            .await
            .map_err(|e| DeregisterError::Docker(e.to_string()))?;
        // Dropped here, which is what ends `cat` on the other side.
    }
    let output = child
        .wait_with_output()
        .await
        .map_err(|e| DeregisterError::Docker(e.to_string()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        return Err(DeregisterError::Docker(if detail.is_empty() {
            format!("docker {} exited with an error", args.join(" "))
        } else {
            detail.to_string()
        }));
    }
    Ok(output.stdout)
}

/// A one-off container of the reporter's own service, so the volume is found the way
/// compose names it. Every reporter image is alpine underneath, whatever its version.
fn in_reporter(service: &str, script: &str) -> Vec<String> {
    [
        "compose",
        "run",
        "--rm",
        "--no-deps",
        "-T",
        "--entrypoint",
        "sh",
        service,
        "-c",
        script,
    ]
    .map(String::from)
    .to_vec()
}

async fn reporter_is_running(dir: &Path, service: &str) -> Result<bool, DeregisterError> {
    let args = ["compose", "ps", "-q", "--status", "running", service]
        .map(String::from)
        .to_vec();
    Ok(!compose(dir, args, None).await?.trim_ascii().is_empty())
}

async fn stop_reporter(dir: &Path, service: &str) -> Result<(), DeregisterError> {
    let args = ["compose", "stop", service].map(String::from).to_vec();
    compose(dir, args, None).await.map(drop)
}

/// The state in the volume, or `None` when the reporter never got as far as writing one
/// — in which case the seed is still the login that works.
async fn read_volume_state(
    dir: &Path,
    service: &str,
) -> Result<Option<ReporterState>, DeregisterError> {
    let script = format!("cat /state/{STATE_FILENAME} 2>/dev/null || true");
    let raw = compose(dir, in_reporter(service, &script), None).await?;
    Ok(serde_json::from_slice(&raw).ok())
}

/// Through a partial file and a rename, for the reason [`hubhealth::write_state`] does it.
async fn write_volume_state(
    dir: &Path,
    service: &str,
    state: &ReporterState,
) -> Result<(), DeregisterError> {
    let script = format!(
        "cat > /state/{STATE_FILENAME}.partial && mv /state/{STATE_FILENAME}.partial /state/{STATE_FILENAME}"
    );
    let json = serde_json::to_vec_pretty(state).expect("serializes");
    compose(dir, in_reporter(service, &script), Some(&json))
        .await
        .map(drop)
}

/// Owner-only: for as long as it exists it holds a login.
fn create_rescue_dir(rescue: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(rescue)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(rescue, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

// --- the whole of it -----------------------------------------------------------------

/// An access token the server will take, obtained without costing the hub its login.
async fn hub_token(
    dir: &Path,
    client: &reqwest::Client,
    token_endpoint: &str,
    seed: &HubCredentials,
    reporter: Option<&ReporterBlock>,
) -> Result<String, DeregisterError> {
    // A grant without a refresh token has no chain to protect, and nothing to renew:
    // the access token it came with either still works or the server says it does not.
    if seed
        .envelope
        .refresh_token
        .as_deref()
        .is_none_or(str::is_empty)
    {
        return Ok(seed.envelope.access_token.clone());
    }

    let rescue = dir.join(RESCUE_DIR);
    // A state left here is newer than the volume's: an earlier attempt rotated the token
    // and could not put it back.
    let rescued = hubhealth::read_state(&rescue);
    let stored = match (&rescued, reporter) {
        (Some(_), _) => rescued.clone(),
        (None, Some(reporter)) => read_volume_state(dir, &reporter.host).await?,
        (None, None) => None,
    };

    create_rescue_dir(&rescue)?;
    let renewed = fresh_access_token(client, token_endpoint, seed, stored, &rescue).await;
    let (token, state) = match renewed {
        Ok(renewed) => renewed,
        Err(error) => {
            // Nothing was rotated, so there is nothing here worth keeping — unless it
            // was here before, in which case it is still the newest login there is.
            if rescued.is_none() {
                let _ = std::fs::remove_dir_all(&rescue);
            }
            return Err(error);
        }
    };

    // Without a reporter nothing else reads the login, and the folder is its only home.
    if let Some(reporter) = reporter {
        write_volume_state(dir, &reporter.host, &state)
            .await
            .map_err(|error| DeregisterError::WriteBack {
                rescue: rescue.join(STATE_FILENAME).display().to_string(),
                detail: error.to_string(),
            })?;
        let _ = std::fs::remove_dir_all(&rescue);
    }
    Ok(token)
}

/// Removes the hub in `dir` from the coordination server it was authorized against.
///
/// Leaves the hub as it found it when the server does not delete it: the reporter is
/// started again if it was running, with a login that works. The one exception is
/// [`DeregisterError::WriteBack`], where starting it would do harm.
pub async fn deregister(dir: &Path) -> Result<ServerOutcome, DeregisterError> {
    let Some(seed) = read_credentials(dir) else {
        return Ok(ServerOutcome::NotRegistered);
    };
    let reporter = crate::profile::read_profile(dir)
        .ok()
        .and_then(|profile| profile.config.reporter)
        .filter(|reporter| reporter.enabled);

    let client = reqwest::Client::builder().timeout(TIMEOUT).build()?;

    // Asked before the reporter is touched: a server that is not there, or cannot do
    // this, is found out without having stopped anything.
    let well_known = tokio::time::timeout(TIMEOUT, wellknown::discover(&seed.server))
        .await
        .map_err(|_| DeregisterError::Unreachable(wellknown::base_url(&seed.server)))??;
    let token_endpoint = well_known
        .token_endpoint
        .clone()
        .ok_or(DeregisterError::NoTokenEndpoint)?;
    let endpoint = deletion_endpoint(&well_known).ok_or(DeregisterError::NoTokenEndpoint)?;

    // Stopped so it cannot rotate the token between our reading it and our using it.
    let was_running = match &reporter {
        Some(reporter) => {
            let running = reporter_is_running(dir, &reporter.host).await?;
            stop_reporter(dir, &reporter.host).await?;
            running
        }
        None => false,
    };

    let outcome = async {
        let token = hub_token(dir, &client, &token_endpoint, &seed, reporter.as_ref()).await?;
        remove_hub(&client, &endpoint, &token).await
    }
    .await;

    match &outcome {
        Ok(_) => {
            // The login is void now. Moved aside rather than deleted: the folder is about
            // to go anyway, and until it does this is the record of what the hub was.
            let _ = std::fs::rename(credentials_path(dir), dir.join(DEREGISTERED_FILENAME));
        }
        Err(DeregisterError::WriteBack { .. }) => {}
        Err(_) => {
            if let (true, Some(reporter)) = (was_running, &reporter) {
                // Best effort: the error being returned is the one that matters.
                let _ = compose(dir, compose::up_service(&reporter.host), None).await;
            }
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn well_known(json: serde_json::Value) -> wellknown::WellKnownFakts {
        serde_json::from_value(json).expect("a well-known")
    }

    #[test]
    fn a_declared_deletion_endpoint_wins() {
        let declared = well_known(serde_json::json!({
            "token_endpoint": "https://go.example/lok/o/token/",
            "hub_deletion_endpoint": "https://go.example/elsewhere/",
        }));
        assert_eq!(
            deletion_endpoint(&declared).as_deref(),
            Some("https://go.example/elsewhere/")
        );
    }

    #[test]
    fn the_deletion_endpoint_is_otherwise_the_token_endpoints_sibling() {
        let derived = well_known(serde_json::json!({
            "token_endpoint": "https://go.example/lok/o/token/",
        }));
        assert_eq!(
            deletion_endpoint(&derived).as_deref(),
            Some("https://go.example/lok/f/hubdelete/")
        );
        assert_eq!(deletion_endpoint(&well_known(serde_json::json!({}))), None);
    }

    #[test]
    fn the_outcome_is_spelled_the_way_the_front_end_reads_it() {
        assert_eq!(
            serde_json::to_value(ServerOutcome::AlreadyGone).unwrap(),
            serde_json::json!("already_gone")
        );
    }
}
