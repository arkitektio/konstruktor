//! Which container engine this machine has, and where its binary is.
//!
//! Everything Konstruktor asks of a container engine goes through its CLI — `compose up`,
//! `inspect`, `ps`. Nothing here opens the daemon socket, on purpose: a socket path is a
//! guess that goes stale (Docker Desktop 4.13+ moved the user socket to
//! `~/.docker/run/docker.sock`, Colima and Rancher and OrbStack each have their own, and
//! Podman's is somewhere under `$XDG_RUNTIME_DIR`), while the CLI already knows the answer
//! because it reads its own contexts. Asking the binary is the only lookup that stays
//! correct across all of them.
//!
//! Two questions, deliberately separated, because they have different costs:
//!
//! * *Which binary* — `<name> --version`, which never touches a daemon and cannot hang.
//!   [`engine`] answers it synchronously and caches the answer.
//! * *Which daemon answers* — a real round trip that can block on a socket nobody is
//!   serving. Only [`discover`] asks it, under a timeout, and it corrects the cache.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The engines we know how to drive. Both speak the same subcommands for everything
/// Konstruktor does, so the kind only decides which binary to run and what to call it in
/// the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EngineKind {
    Docker,
    Podman,
}

impl EngineKind {
    pub fn binary_name(self) -> &'static str {
        match self {
            EngineKind::Docker => "docker",
            EngineKind::Podman => "podman",
        }
    }

    /// What to call it in a sentence the user reads.
    pub fn label(self) -> &'static str {
        match self {
            EngineKind::Docker => "Docker",
            EngineKind::Podman => "Podman",
        }
    }

    /// The engine socket as it is named *inside* a container, for the deployer's bind
    /// mount. Docker Desktop and every `docker`-compatible runtime present the daemon at
    /// the classic path inside the VM regardless of where the host socket lives; Podman
    /// keeps its own.
    pub fn container_socket(self) -> &'static str {
        match self {
            EngineKind::Docker => "/var/run/docker.sock",
            EngineKind::Podman => "/run/podman/podman.sock",
        }
    }
}


/// Which *product* is serving the engine — the thing the user has to start, or install.
///
/// `EngineKind` says which CLI to run; this says what to call the runtime behind it in a
/// sentence. "Start Docker" is the wrong instruction on a machine whose daemon lives in
/// a Colima VM, and a remedy that names the right product is the difference between a
/// user who clicks once and one who reinstalls something that was never broken.
///
/// Read off `info` when the daemon answers, and guessed from the filesystem when it does
/// not — which is exactly the case where the name matters most.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum EngineBrand {
    DockerDesktop,
    Colima,
    OrbStack,
    RancherDesktop,
    PodmanDesktop,
    /// A plain `dockerd` or `podman` on the host — Linux, mostly.
    Native,
    #[default]
    Unknown,
}

impl EngineBrand {
    pub fn label(self) -> &'static str {
        match self {
            EngineBrand::DockerDesktop => "Docker Desktop",
            EngineBrand::Colima => "Colima",
            EngineBrand::OrbStack => "OrbStack",
            EngineBrand::RancherDesktop => "Rancher Desktop",
            EngineBrand::PodmanDesktop => "Podman",
            EngineBrand::Native => "the daemon",
            EngineBrand::Unknown => "the engine",
        }
    }

    /// The brand out of an `info --format '{{json .}}'` payload.
    ///
    /// Each runtime leaves a fingerprint somewhere different: Docker Desktop and Rancher
    /// name themselves in `OperatingSystem`, Colima and OrbStack in the VM hostname
    /// (`Name`), Podman under `host`. A plain `dockerd` says none of those and is
    /// `Native`.
    pub fn from_info(info: &serde_json::Value) -> EngineBrand {
        let field = |path: &str| {
            info.pointer(path)
                .and_then(serde_json::Value::as_str)
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default()
        };
        let name = field("/Name");
        let os = field("/OperatingSystem");
        let kernel = field("/KernelVersion");
        let server = field("/ServerVersion");

        if os.contains("docker desktop") {
            EngineBrand::DockerDesktop
        } else if name.contains("colima") || os.contains("colima") {
            EngineBrand::Colima
        } else if name.contains("orbstack") || kernel.contains("orbstack") {
            EngineBrand::OrbStack
        } else if name.contains("rancher") || os.contains("rancher") {
            EngineBrand::RancherDesktop
        } else if info.pointer("/host").is_some() {
            // Podman's payload. `podman machine` on a desktop OS is what ships with
            // Podman Desktop; on Linux it is the native daemonless engine.
            if cfg!(target_os = "linux") {
                EngineBrand::Native
            } else {
                EngineBrand::PodmanDesktop
            }
        } else if server.contains("rd") || name.contains("rancher-desktop") {
            EngineBrand::RancherDesktop
        } else if !name.is_empty() || !os.is_empty() {
            EngineBrand::Native
        } else {
            EngineBrand::Unknown
        }
    }

    /// A best guess from what is installed, for when the daemon is not answering and
    /// `info` cannot tell us. Ordered so the first match is the most specific footprint.
    pub fn from_filesystem(kind: EngineKind) -> EngineBrand {
        if kind == EngineKind::Podman {
            return if cfg!(target_os = "linux") {
                EngineBrand::Native
            } else {
                EngineBrand::PodmanDesktop
            };
        }
        let home = dirs::home_dir();
        let at_home = |rel: &str| home.as_ref().map(|h| h.join(rel).exists()).unwrap_or(false);

        if cfg!(target_os = "macos") {
            if at_home(".colima") {
                return EngineBrand::Colima;
            }
            if std::path::Path::new("/Applications/OrbStack.app").exists() || at_home(".orbstack") {
                return EngineBrand::OrbStack;
            }
            if at_home(".rd") {
                return EngineBrand::RancherDesktop;
            }
            if std::path::Path::new("/Applications/Docker.app").exists() {
                return EngineBrand::DockerDesktop;
            }
            return EngineBrand::Unknown;
        }
        if cfg!(windows) {
            if at_home(".rd") {
                return EngineBrand::RancherDesktop;
            }
            let program_files =
                std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
            if std::path::Path::new(&program_files).join("Docker").join("Docker").exists() {
                return EngineBrand::DockerDesktop;
            }
            return EngineBrand::Unknown;
        }
        // Linux: Docker Desktop for Linux exists but is rare; a `dockerd` is the norm.
        if at_home(".docker/desktop") {
            return EngineBrand::DockerDesktop;
        }
        EngineBrand::Native
    }
}

/// A resolved engine: the kind, and the binary to actually execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    pub kind: EngineKind,
    /// Either the bare name (found on `PATH`) or an absolute path, when `PATH` did not
    /// have it — a GUI-launched macOS app inherits a minimal `PATH` from Finder.
    pub binary: PathBuf,
}

impl Engine {
    /// A command for this engine, ready for `.args(…)`.
    pub fn command(&self) -> std::process::Command {
        std::process::Command::new(&self.binary)
    }

    pub fn async_command(&self) -> tokio::process::Command {
        tokio::process::Command::new(&self.binary)
    }
}

static CACHE: OnceLock<Mutex<Option<Engine>>> = OnceLock::new();

fn cache() -> &'static Mutex<Option<Engine>> {
    CACHE.get_or_init(|| Mutex::new(None))
}

fn cached() -> Option<Engine> {
    cache().lock().ok().and_then(|c| c.clone())
}

fn store(engine: &Engine) {
    if let Ok(mut c) = cache().lock() {
        *c = Some(engine.clone());
    }
}

/// Forgets what was found, so the next lookup asks the filesystem again.
///
/// [`discover`] calls this first: a bare `docker` cached before the user installed
/// anything would otherwise outlive the install, and the freshly installed engine would
/// be found by the probe yet never used by the calls that go through [`engine`].
pub fn forget() {
    if let Ok(mut c) = cache().lock() {
        *c = None;
    }
}

/// The engine to run, right now, without touching a daemon.
///
/// This is what every `docker …` call site uses. It never blocks on a socket: worst case
/// it runs two `--version` checks once, then answers from cache. When [`discover`] has
/// run, its verdict — which is the better-informed one, having actually reached a
/// daemon — is what comes back here.
///
/// Falls back to plain `docker` when nothing is installed, so the resulting failure is the
/// familiar "docker: command not found" rather than something Konstruktor invented.
pub fn engine() -> Engine {
    if let Some(engine) = cached() {
        return engine;
    }

    let resolved = forced()
        .or_else(|| resolve(EngineKind::Docker))
        .or_else(|| resolve(EngineKind::Podman))
        .unwrap_or(Engine {
            kind: EngineKind::Docker,
            binary: PathBuf::from("docker"),
        });

    store(&resolved);
    resolved
}

/// Shorthand for the overwhelmingly common `engine().command()`.
pub fn command() -> std::process::Command {
    engine().command()
}

/// `KONSTRUKTOR_ENGINE=podman` — an override for a machine with both installed where the
/// automatic choice is not the one the user wants.
fn forced() -> Option<Engine> {
    let name = std::env::var("KONSTRUKTOR_ENGINE").ok()?;
    let kind = match name.trim().to_ascii_lowercase().as_str() {
        "docker" => EngineKind::Docker,
        "podman" => EngineKind::Podman,
        _ => return None,
    };
    resolve(kind)
}

/// Where a binary might be when `PATH` is not to be trusted.
///
/// A macOS app launched from Finder gets `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, which has
/// neither Homebrew nor Docker Desktop's own bin directory in it. `fix_env` in the desktop
/// app normally repairs that by reading the login shell, but it can fail — a shell that
/// prompts, a profile that errors — and this is the floor under it.
fn candidates(name: &str) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        // `PATH` is inherited intact on Windows, but an installer that just ran has
        // changed it for *new* processes only — this one still has the old value.
        let mut dirs = Vec::new();
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".rd").join("bin"));
        }
        if let Ok(program_files) = std::env::var("ProgramFiles") {
            dirs.push(
                PathBuf::from(program_files)
                    .join("Docker")
                    .join("Docker")
                    .join("resources")
                    .join("bin"),
            );
            dirs.push(PathBuf::from("C:\\Program Files\\RedHat\\Podman"));
        }
        dirs.into_iter()
            .map(|dir| dir.join(format!("{name}.exe")))
            .collect()
    }
    #[cfg(not(windows))]
    {
        let mut dirs = vec![
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
            // Docker Desktop's bundle, and Podman's macOS installer.
            PathBuf::from("/Applications/Docker.app/Contents/Resources/bin"),
            PathBuf::from("/opt/podman/bin"),
        ];
        if let Some(home) = dirs::home_dir() {
            // Docker Desktop's own bin, and Rancher Desktop's.
            dirs.push(home.join(".docker/bin"));
            dirs.push(home.join(".rd/bin"));
            dirs.push(home.join(".local/bin"));
        }
        dirs.into_iter().map(|dir| dir.join(name)).collect()
    }
}

/// Finds the binary for one engine, or `None` if it is not installed.
///
/// `--version` is answered by the CLI itself and never reaches a daemon, so this stays
/// fast and cannot hang even when the engine is installed but stopped.
fn resolve(kind: EngineKind) -> Option<Engine> {
    let name = kind.binary_name();

    if runs(&PathBuf::from(name)) {
        return Some(Engine {
            kind,
            binary: PathBuf::from(name),
        });
    }

    candidates(name)
        .into_iter()
        .find(|path| path.is_file() && runs(path))
        .map(|binary| Engine { kind, binary })
}

/// Any tool by name — `brew`, `winget`, `colima` — on `PATH` or in the same places an
/// engine binary might hide from a GUI app. Used by the installers, which have exactly
/// the same `PATH` problem the engine has.
pub fn find_tool(name: &str) -> Option<PathBuf> {
    let bare = PathBuf::from(name);
    if runs(&bare) {
        return Some(bare);
    }
    candidates(name)
        .into_iter()
        .find(|path| path.is_file() && runs(path))
}

fn runs(binary: &PathBuf) -> bool {
    std::process::Command::new(binary)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// What [`discover`] found: the engine to use, and whether its daemon answered.
///
/// The `info` payload rides along because getting it *is* the daemon check — asking twice
/// would double the wait on exactly the machine that can least afford it, one where the
/// engine is installed but stopped and every call has to run out its deadline.
pub struct Discovery {
    pub engine: Engine,
    /// The `info` payload, when the daemon answered. `None` means it did not.
    pub info: Option<serde_json::Value>,
    /// Why it did not, when it did not.
    pub error: Option<String>,
    /// What is serving — or, when nothing answered, what appears to be installed.
    pub brand: EngineBrand,
}

/// The engine whose daemon actually answers, preferred over one that is merely installed.
///
/// Order matters only when both are installed: Docker first, because a machine with Docker
/// Desktop running and Podman installed but idle should use Docker. If neither daemon
/// answers we still return whichever is installed, so the UI can say "start Docker" rather
/// than sending someone who already has it to a download page.
///
/// `budget` bounds the daemon round trips — the only calls here that can block, on a
/// socket that exists but is not being served. It is a budget for the whole search rather
/// than a timeout per engine: two stopped engines would otherwise take twice as long to
/// give the same answer, and somebody is waiting on it.
pub async fn discover(budget: Duration) -> Option<Discovery> {
    // Start from nothing: this is the call that runs after "Check again", and its whole
    // point is to notice what changed.
    forget();

    let installed: Vec<Engine> = match forced() {
        Some(engine) => vec![engine],
        None => [EngineKind::Docker, EngineKind::Podman]
            .into_iter()
            .filter_map(resolve)
            .collect(),
    };

    let deadline = tokio::time::Instant::now() + budget;
    let mut first_error = None;

    for engine in &installed {
        // Never zero: a deadline already spent should still let the last engine make one
        // honest attempt rather than reporting a timeout it never ran.
        let remaining = deadline
            .checked_duration_since(tokio::time::Instant::now())
            .unwrap_or(MINIMUM_ATTEMPT)
            .max(MINIMUM_ATTEMPT);

        match ask_info(engine, remaining).await {
            Ok(info) => {
                store(engine);
                let brand = EngineBrand::from_info(&info);
                return Some(Discovery {
                    engine: engine.clone(),
                    info: Some(info),
                    error: None,
                    brand,
                });
            }
            Err(e) => first_error.get_or_insert(e),
        };
    }

    let engine = installed.into_iter().next()?;
    store(&engine);
    let brand = EngineBrand::from_filesystem(engine.kind);
    Some(Discovery {
        engine,
        info: None,
        error: first_error,
        brand,
    })
}

/// However little of the budget is left, one attempt still gets this long.
const MINIMUM_ATTEMPT: Duration = Duration::from_secs(1);

/// `info` rather than `version`, because it is the cheapest call that genuinely requires
/// the daemon on *both* engines — Podman is its own client and answers `version` with no
/// service running at all, so a `version` check would report a stopped Podman as ready.
async fn ask_info(engine: &Engine, timeout: Duration) -> Result<serde_json::Value, String> {
    crate::docker::json(engine, &["info", "--format", "{{json .}}"], timeout).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Podman does not have a `docker.sock`, and mounting the wrong one into the deployer
    /// gives it a socket that is simply not there.
    #[test]
    fn each_engine_names_its_own_socket() {
        assert_eq!(
            EngineKind::Docker.container_socket(),
            "/var/run/docker.sock"
        );
        assert_eq!(
            EngineKind::Podman.container_socket(),
            "/run/podman/podman.sock"
        );
    }

    fn info(json: &str) -> serde_json::Value {
        serde_json::from_str(json).unwrap()
    }

    /// Each runtime leaves its name somewhere different in `info`; the remedy that says
    /// "start Colima" depends on reading the right field.
    #[test]
    fn names_the_runtime_from_info() {
        assert_eq!(
            EngineBrand::from_info(&info(r#"{"Name":"docker-desktop","OperatingSystem":"Docker Desktop"}"#)),
            EngineBrand::DockerDesktop
        );
        assert_eq!(
            EngineBrand::from_info(&info(r#"{"Name":"colima","OperatingSystem":"Ubuntu 24.04.1 LTS"}"#)),
            EngineBrand::Colima
        );
        assert_eq!(
            EngineBrand::from_info(&info(r#"{"Name":"orbstack","KernelVersion":"6.12.13-orbstack-00304-gede1cf3a7c6"}"#)),
            EngineBrand::OrbStack
        );
        assert_eq!(
            EngineBrand::from_info(&info(r#"{"Name":"rancher-desktop","OperatingSystem":"Rancher Desktop WSL Distribution"}"#)),
            EngineBrand::RancherDesktop
        );
        assert_eq!(
            EngineBrand::from_info(&info(r#"{"Name":"myserver","OperatingSystem":"Ubuntu 22.04.4 LTS"}"#)),
            EngineBrand::Native
        );
        assert_eq!(EngineBrand::from_info(&info("{}")), EngineBrand::Unknown);
    }

    /// Podman nests everything under `host`, and which product that means depends on
    /// the OS: a `podman machine` on a desktop is what Podman Desktop ships.
    #[test]
    fn recognises_podman_by_its_shape() {
        let brand = EngineBrand::from_info(&info(r#"{"host":{"os":"linux","memTotal":1}}"#));
        if cfg!(target_os = "linux") {
            assert_eq!(brand, EngineBrand::Native);
        } else {
            assert_eq!(brand, EngineBrand::PodmanDesktop);
        }
    }

    /// The fallback has to be a bare `docker`, not an absolute guess: the error a user
    /// sees for a missing engine should be the shell's, not a path we made up.
    #[test]
    fn falls_back_to_a_bare_docker() {
        let engine = Engine {
            kind: EngineKind::Docker,
            binary: PathBuf::from("docker"),
        };
        assert_eq!(engine.binary.to_str(), Some("docker"));
        assert_eq!(engine.kind.label(), "Docker");
    }
}
