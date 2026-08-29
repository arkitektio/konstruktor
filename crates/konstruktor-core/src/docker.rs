//! Everything Konstruktor needs to know about the container engine on this machine.
//!
//! Both front ends share it: the wizard's first step and the CLI's `doctor` reach the
//! same verdict from the same probe.
//!
//! Every question here is asked through the engine's own CLI rather than its API socket.
//! A socket path is a guess that goes stale — Docker Desktop 4.13+ serves the user socket
//! from `~/.docker/run/docker.sock` and only creates `/var/run/docker.sock` when a setting
//! is on, and Colima, Rancher, OrbStack and Podman each have their own — whereas the CLI
//! resolves its own endpoint from its contexts. Asking the binary is the only lookup that
//! is correct on all of them, and it is what the rest of this crate already does for
//! `compose`.

use std::collections::HashMap;
use std::fs::canonicalize;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine_probe::{self, Engine, EngineKind};

/// A daemon round trip is the only call here that can block on a socket nobody is serving,
/// so every one of them is bounded. The dashboard polls the probe, so an unbounded call
/// would not hang once — it would pile up.
const DAEMON_TIMEOUT: Duration = Duration::from_secs(5);
/// Listing containers and resolving images walk more data; they get a little longer.
const QUERY_TIMEOUT: Duration = Duration::from_secs(10);
/// A restart is a different kind of wait: the engine sends SIGTERM and then waits out the
/// container's whole stop grace period — 10 seconds by default, and a compose file is free
/// to ask for more — before killing it and starting it again. Anything near the query
/// timeout would abandon perfectly ordinary restarts.
const RESTART_TIMEOUT: Duration = Duration::from_secs(120);

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
    /// The human sentence the engine writes for this container, e.g. "Up 2 hours".
    pub status: Option<String>,
    /// The machine-readable state, e.g. "running".
    pub state: Option<String>,
    /// `com.docker.compose.service` — what the dashboard groups by.
    pub service: Option<String>,
}

/// What we found when we looked for a container engine.
///
/// Every field is answered independently, because the three ways this can go wrong have
/// three different remedies: no CLI means "install Docker", a CLI without the compose
/// plugin means "install a newer Docker", and a CLI whose daemon does not answer means
/// "start Docker". Nothing here panics — "Docker is missing" is the ordinary case this
/// exists to report, not an error.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct DockerProbe {
    /// The engine binary is there.
    pub cli: bool,
    /// `docker --version`, e.g. "27.3.1".
    pub cli_version: Option<String>,
    /// `docker compose` is available — it is a plugin, and the CLI can exist without it.
    pub compose: bool,
    /// `docker compose version --short`, e.g. "2.29.7".
    pub compose_version: Option<String>,
    /// The daemon answered. Required to *run* anything.
    pub daemon: bool,
    /// The Engine API version the daemon reports.
    pub api_version: Option<String>,
    /// Total memory the daemon sees, in bytes.
    pub memory: Option<i64>,
    /// Why the daemon could not be reached, when it could not.
    pub error: Option<String>,
    /// Which engine this is about, so the UI can name it rather than assuming Docker.
    /// `None` when nothing was found at all.
    pub engine: Option<EngineKind>,
}

/// The engine reduced to the one thing a UI has to decide: what to tell the user next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DockerState {
    Ready,
    /// No engine binary is there at all — offer an install.
    Missing,
    /// The CLI is present but `compose` is not — offer a newer Docker.
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

    /// What to call this engine in a sentence, before we know which one it is.
    pub fn engine_label(&self) -> &'static str {
        self.engine.map(EngineKind::label).unwrap_or("Docker")
    }
}

/// Runs an engine command with a deadline, capturing its output.
///
/// `kill_on_drop` is what makes the timeout real: [`tokio::time::timeout`] only drops the
/// future, it does not reap the child, so without it a probe against a hung socket would
/// leave a `docker` process behind — once per poll, forever.
pub(crate) async fn run(
    cmd: &mut tokio::process::Command,
    timeout: Duration,
) -> Result<Output, String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    match tokio::time::timeout(timeout, cmd.output()).await {
        Ok(result) => result.map_err(|e| e.to_string()),
        Err(_) => Err(format!("timed out after {}s", timeout.as_secs())),
    }
}

/// The stderr of a failed command, or a generic line when it said nothing.
fn failure(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        format!("exited with {}", output.status)
    } else {
        stderr
    }
}

/// Runs a command that must produce JSON, and parses it.
pub(crate) async fn json(
    engine: &Engine,
    args: &[&str],
    timeout: Duration,
) -> Result<Value, String> {
    let output = run(engine.async_command().args(args), timeout).await?;
    if !output.status.success() {
        return Err(failure(&output));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())
}

/// Reads a list of JSON objects out of engine output.
///
/// The two engines disagree on the shape: Docker's `--format json` writes one object per
/// line, Podman's writes a single array. `inspect` writes an array on both. Accepting
/// either here means no call site has to care which engine it is talking to.
fn json_list(stdout: &[u8]) -> Vec<Value> {
    let text = String::from_utf8_lossy(stdout);
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }

    if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(text) {
        return items;
    }

    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// The arguments for a throwaway container that gives a tree back to its owner.
///
/// The engine daemon runs as root and creates bind-mount targets as root, so the data
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
///   The engine resolves it through the container's `PATH`, so the bare name avoids
///   guessing between `/bin/chown` and `/usr/bin/chown`.
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
/// `image inspect` rather than reading a `run` failure for "Unable to find image": the
/// exit status is the same answer, and it does not depend on the daemon's locale or on
/// wording that changes between versions.
pub fn image_present(image: &str) -> bool {
    engine_probe::command()
        .args(["image", "inspect", image])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Runs a command that must not touch the daemon, so it stays fast and cannot hang.
/// `None` means the binary could not be executed at all.
fn probe_command(engine: &Engine, args: &[&str]) -> Option<String> {
    let output = engine.command().args(args).output().ok()?;
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

/// The version out of a `… version` banner, wherever in the line it sits.
///
/// The banners put it in different places — `Docker version 27.3.1, build ce122…` against
/// `Docker Compose version v5.1.3` — so this looks for the first token that actually looks
/// like a version rather than counting words. Counting words is what the first cut did,
/// and it reported Compose's version as the literal string "version".
pub(crate) fn parse_cli_version(line: &str) -> Option<String> {
    line.split_whitespace()
        .map(|token| token.trim_end_matches(','))
        .find(|token| {
            let digits = token.strip_prefix('v').unwrap_or(token);
            digits.starts_with(|c: char| c.is_ascii_digit())
        })
        .map(str::to_string)
}

/// The API version out of a `version --format '{{json .}}'` payload.
///
/// Docker reports the daemon's under `Server.ApiVersion`; Podman, which is its own client
/// and server, reports `Client.APIVersion`. Both spellings of the key are in the wild.
pub(crate) fn parse_api_version(version: &Value) -> Option<String> {
    [
        "/Server/ApiVersion",
        "/Server/APIVersion",
        "/Client/APIVersion",
        "/Client/ApiVersion",
    ]
    .into_iter()
    .filter_map(|path| version.pointer(path))
    .find_map(Value::as_str)
    .map(str::to_string)
}

/// Total memory out of an `info --format '{{json .}}'` payload — Docker capitalises its
/// keys, Podman nests the same number under a lowercase `host`.
pub(crate) fn parse_mem_total(info: &Value) -> Option<i64> {
    ["/MemTotal", "/host/memTotal"]
        .into_iter()
        .filter_map(|path| info.pointer(path))
        .find_map(Value::as_i64)
}

pub async fn probe() -> DockerProbe {
    let mut probe = DockerProbe::default();

    // Which engine, and where. This prefers one whose daemon answers, so a machine with
    // both Docker and Podman installed uses the one that is actually running — and it
    // hands back the `info` it had to fetch to find out, so the daemon is asked once.
    let Some(found) = engine_probe::discover(DAEMON_TIMEOUT).await else {
        // Nothing installed. `Missing` is the verdict, and it is not an error.
        return probe;
    };
    let engine = found.engine;

    probe.engine = Some(engine.kind);
    probe.cli = true;
    probe.cli_version = probe_command(&engine, &["--version"])
        .as_deref()
        .and_then(parse_cli_version);

    // `compose version` is answered by the CLI itself and needs no daemon, so a stopped
    // engine still reports it. `--short` is not understood by the earliest Compose v2
    // builds, and a failed parse there would report a working Compose as missing — which
    // is a hard block with a download link attached. Plain `compose version` is the
    // fallback.
    if let Some(version) = probe_command(&engine, &["compose", "version", "--short"]) {
        probe.compose = true;
        probe.compose_version = Some(version);
    } else if let Some(line) = probe_command(&engine, &["compose", "version"]) {
        probe.compose = true;
        probe.compose_version = parse_cli_version(&line);
    }

    match found.info {
        Some(info) => {
            probe.daemon = true;
            probe.memory = parse_mem_total(&info);
            // Only now, and only for the display: the API version is the one thing the
            // daemon check did not already answer, and it is not worth a wait when the
            // daemon is down.
            probe.api_version = json(
                &engine,
                &["version", "--format", "{{json .}}"],
                DAEMON_TIMEOUT,
            )
            .await
            .ok()
            .as_ref()
            .and_then(parse_api_version);
        }
        None => probe.error = found.error,
    }

    probe
}

/// The containers belonging to the compose project in `path`.
///
/// The generated stack carries no `arkitekt.*` labels — it is a plain compose project —
/// so its containers are identified by the directory compose was run in, which stays
/// stable even when two deployments would derive the same project name.
///
/// Two calls, because neither alone has everything: `ps` knows the human status sentence
/// ("Up 2 hours") and nothing else knows it, while `inspect` is the only one that gives
/// the labels as a map and the resolved image id. `ps --format json` flattens labels into
/// a comma-joined string that cannot be parsed back, and omits the image id entirely.
pub async fn list_deployment_containers(path: &str) -> Result<Vec<Container>, String> {
    // The sync accessor, not `discover`: this runs behind a dashboard that has already
    // probed, so the answer is cached. On a cold path it costs one `--version` and no
    // daemon round trip, which is short enough to do on the runtime thread.
    let engine = engine_probe::engine();

    let dir = canonicalize(path).map_err(|e| e.to_string())?;
    let filter = format!(
        "label=com.docker.compose.project.working_dir={}",
        dir.to_string_lossy()
    );

    let listed = run(
        engine
            .async_command()
            .args(["ps", "-a", "--filter", &filter, "--format", "{{json .}}"]),
        QUERY_TIMEOUT,
    )
    .await?;
    if !listed.status.success() {
        return Err(failure(&listed));
    }

    // The status sentence, keyed by the id `ps` prints — which is truncated, so the
    // lookup below matches on prefix against the full id `inspect` gives.
    let summaries: Vec<(String, Option<String>)> = json_list(&listed.stdout)
        .iter()
        .filter_map(|row| {
            let id = ["ID", "Id", "ContainerID"]
                .into_iter()
                .find_map(|key| row.get(key).and_then(Value::as_str))?;
            let status = row
                .get("Status")
                .and_then(Value::as_str)
                .map(str::to_string);
            Some((id.to_string(), status))
        })
        .collect();

    if summaries.is_empty() {
        // `inspect` with no arguments is an error, not an empty answer.
        return Ok(Vec::new());
    }

    let ids: Vec<&str> = summaries.iter().map(|(id, _)| id.as_str()).collect();
    let mut args = vec!["inspect"];
    args.extend_from_slice(&ids);
    let inspected = run(engine.async_command().args(&args), QUERY_TIMEOUT).await?;
    if !inspected.status.success() {
        return Err(failure(&inspected));
    }

    Ok(json_list(&inspected.stdout)
        .iter()
        .map(|c| container_from_inspect(c, &summaries))
        .collect())
}

/// One `inspect` payload as the dashboard wants it.
///
/// The two image fields are easy to swap and fail silently if you do: `Config.Image` is
/// the tag the compose file wrote, `Image` is the id it resolved to when the container was
/// created. The dashboard compares the latter against the tag's *current* id to spot an
/// update that was pulled but never applied.
fn container_from_inspect(c: &Value, summaries: &[(String, Option<String>)]) -> Container {
    let id = c.get("Id").and_then(Value::as_str).map(str::to_string);

    let labels: Option<HashMap<String, String>> = c
        .pointer("/Config/Labels")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                .collect()
        });

    // `inspect` gives the single name, with the leading slash the API's vector also
    // carries. Both are kept as they are: the dashboard renders what it is given.
    let names = c
        .get("Name")
        .and_then(Value::as_str)
        .map(|name| vec![name.to_string()]);

    let status = id.as_ref().and_then(|full| {
        summaries
            .iter()
            .find(|(short, _)| full.starts_with(short.as_str()))
            .and_then(|(_, status)| status.clone())
    });

    Container {
        service: labels
            .as_ref()
            .and_then(|l| l.get("com.docker.compose.service").cloned()),
        id,
        names,
        image: c
            .pointer("/Config/Image")
            .and_then(Value::as_str)
            .map(str::to_string),
        image_id: c.get("Image").and_then(Value::as_str).map(str::to_string),
        status,
        state: c
            .pointer("/State/Status")
            .and_then(Value::as_str)
            .map(str::to_string),
        labels,
    }
}

/// What the local engine currently holds for one image reference.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImageState {
    /// The reference as the compose file spells it, e.g. `jhnnsrs/rekuest:next`.
    pub image: String,
    /// The compose service this image belongs to, so the UI can line it up with the
    /// containers it already lists.
    pub service: String,
    /// Whether the engine has it at all. `false` means nothing has pulled it yet.
    pub present: bool,
    /// The id the tag resolves to *now*. Compared against a running container's
    /// `image_id` to tell "a newer image is pulled but not running yet".
    pub image_id: Option<String>,
    /// When that image was built, as the engine reports it.
    pub created: Option<String>,
}

/// Resolves every image the stack declares against the local engine.
///
/// Nothing here pulls or contacts a registry: this answers "what is on this machine",
/// which is all that is needed to spot an update that was downloaded but never applied.
/// Whether something *newer* exists upstream is a different question, and a registry
/// query this deliberately does not make.
pub async fn image_states(images: &[(String, String)]) -> Result<Vec<ImageState>, String> {
    let engine = engine_probe::engine();

    let mut states = Vec::with_capacity(images.len());
    for (service, image) in images {
        // A missing image is the ordinary case before the first pull, not an error: a
        // non-zero exit here means "not on this machine", which is the answer.
        let inspected = run(
            engine.async_command().args(["image", "inspect", image]),
            QUERY_TIMEOUT,
        )
        .await
        .ok()
        .filter(|output| output.status.success())
        .map(|output| json_list(&output.stdout))
        .and_then(|items| items.into_iter().next());

        states.push(ImageState {
            image: image.clone(),
            service: service.clone(),
            present: inspected.is_some(),
            image_id: inspected
                .as_ref()
                .and_then(|i| i.get("Id"))
                .and_then(Value::as_str)
                .map(str::to_string),
            created: inspected
                .as_ref()
                .and_then(|i| i.get("Created"))
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }
    Ok(states)
}

pub async fn restart_container(container_id: &str) -> Result<(), String> {
    let engine = engine_probe::engine();
    let output = run(
        engine.async_command().args(["restart", container_id]),
        RESTART_TIMEOUT,
    )
    .await?;

    if output.status.success() {
        Ok(())
    } else {
        Err(failure(&output))
    }
}

/// A plain `Command` for the discovered engine — what every call site that shells out to
/// `docker …` uses instead of naming the binary itself.
pub fn command() -> Command {
    engine_probe::command()
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

    /// The timeout has to actually reap the child. `tokio::time::timeout` only drops the
    /// future; without `kill_on_drop` the abandoned process keeps running, and since the
    /// dashboard polls the probe that would leak one process per tick.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_timed_out_command_leaves_no_process_behind() {
        let mut cmd = tokio::process::Command::new("/bin/sleep");
        cmd.arg("30");
        let started = std::time::Instant::now();
        let result = run(&mut cmd, Duration::from_millis(200)).await;

        assert!(result.is_err(), "the deadline should have been hit");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the wait ended with the deadline, not with the command"
        );

        // The child is killed as the future drops, so nothing is left holding the sleep.
        let survivors = std::process::Command::new("pgrep")
            .args(["-f", "^/bin/sleep 30$"])
            .output()
            .expect("pgrep");
        assert!(
            survivors.stdout.is_empty(),
            "an abandoned child outlived its timeout"
        );
    }

    /// Docker writes one JSON object per line, Podman writes an array, and `inspect`
    /// writes an array on both. Every call site takes whichever it is given.
    #[test]
    fn reads_both_json_shapes() {
        let lines = br#"{"ID":"abc"}
{"ID":"def"}"#;
        assert_eq!(json_list(lines).len(), 2);

        let array = br#"[{"Id":"abc"},{"Id":"def"}]"#;
        assert_eq!(json_list(array).len(), 2);

        assert!(json_list(b"").is_empty());
        assert!(json_list(b"   \n ").is_empty());
    }

    /// Docker puts the daemon's API version under `Server`; Podman, which is its own
    /// server, puts it under `Client` and spells the key differently.
    #[test]
    fn reads_the_api_version_from_either_engine() {
        let docker: Value = serde_json::from_str(
            r#"{"Server":{"ApiVersion":"1.47"},"Client":{"ApiVersion":"1.47"}}"#,
        )
        .unwrap();
        assert_eq!(parse_api_version(&docker).as_deref(), Some("1.47"));

        let podman: Value =
            serde_json::from_str(r#"{"Client":{"APIVersion":"5.2.2"},"Server":null}"#).unwrap();
        assert_eq!(parse_api_version(&podman).as_deref(), Some("5.2.2"));

        assert_eq!(parse_api_version(&Value::Null), None);
    }

    #[test]
    fn reads_total_memory_from_either_engine() {
        let docker: Value = serde_json::from_str(r#"{"MemTotal":16777216}"#).unwrap();
        assert_eq!(parse_mem_total(&docker), Some(16777216));

        let podman: Value = serde_json::from_str(r#"{"host":{"memTotal":8388608}}"#).unwrap();
        assert_eq!(parse_mem_total(&podman), Some(8388608));
    }

    /// The two image fields are the ones that break quietly when swapped: `Config.Image`
    /// is the tag, `Image` is the id the dashboard compares to spot a pulled update.
    #[test]
    fn maps_an_inspect_payload_onto_the_dashboards_fields() {
        let payload: Value = serde_json::from_str(
            r#"{
                "Id": "1234567890abcdef1234567890abcdef",
                "Name": "/hub-rekuest-1",
                "Image": "sha256:deadbeef",
                "State": {"Status": "running"},
                "Config": {
                    "Image": "jhnnsrs/rekuest:next",
                    "Labels": {"com.docker.compose.service": "rekuest"}
                }
            }"#,
        )
        .unwrap();

        let summaries = vec![("1234567890ab".to_string(), Some("Up 2 hours".to_string()))];
        let container = container_from_inspect(&payload, &summaries);

        assert_eq!(container.image.as_deref(), Some("jhnnsrs/rekuest:next"));
        assert_eq!(container.image_id.as_deref(), Some("sha256:deadbeef"));
        assert_eq!(container.state.as_deref(), Some("running"));
        // Matched on prefix, because `ps` truncates the id and `inspect` does not.
        assert_eq!(container.status.as_deref(), Some("Up 2 hours"));
        assert_eq!(container.service.as_deref(), Some("rekuest"));
        // The leading slash is what the API's name vector carried too.
        assert_eq!(
            container.names.as_deref(),
            Some(["/hub-rekuest-1".to_string()].as_slice())
        );
    }
}
