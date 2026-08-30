//! What to tell somebody whose container engine is not ready, and what a button can do
//! about it.
//!
//! The probe says *what* is wrong — no binary, no compose, a silent daemon, a version
//! too old. This module says what to do about it on *this* OS, for *this* product, and
//! it says it as data: a list of [`Remedy`]s, each a title, a sentence and some
//! [`Step`]s. The desktop app renders steps as buttons and code blocks; the CLI prints
//! them. Neither invents wording of its own, so the two cannot drift.
//!
//! Open-source engines come first. On macOS that is Colima, on Windows Rancher Desktop,
//! on Linux the distribution's own `docker` — each gives a real `docker` CLI with the
//! compose plugin, which is all Konstruktor asks for. Docker Desktop is listed, because
//! it works, but it is not what we send anybody to install.
//!
//! Everything an installer runs is a fixed string in this file. No step is ever built
//! from user input, and the desktop app refuses to run anything that is not one of the
//! [`InstallerId`]s here.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::docker::{DockerProbe, DockerState};
use crate::engine_probe::{self, EngineBrand, EngineKind};

/// The OS this is running on, as far as installing an engine is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Platform {
    Macos,
    Windows,
    Linux,
    Other,
}

impl Platform {
    pub fn current() -> Platform {
        if cfg!(target_os = "macos") {
            Platform::Macos
        } else if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "linux") {
            Platform::Linux
        } else {
            Platform::Other
        }
    }
}

/// The installers the desktop app knows how to run. A closed set on purpose: the
/// command behind each is a literal in [`InstallerId::plan`], never an argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallerId {
    /// `brew install colima docker docker-compose`, link the compose plugin, start it.
    BrewColima,
    /// `brew install docker-compose` and link it — for a Colima or brew `docker` that
    /// has the CLI but not the plugin.
    BrewComposePlugin,
    /// `winget install SUSE.RancherDesktop`, then launch it.
    WingetRancherDesktop,
}

/// The products the app can start on the user's behalf, when the CLI is there but the
/// daemon is not answering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StartTarget {
    Colima,
    DockerDesktop,
    OrbStack,
    RancherDesktop,
    PodmanMachine,
}

/// One thing the user, or the app on their behalf, can do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Step {
    /// A page to open.
    OpenUrl { label: String, url: String },
    /// A command the user pastes into a terminal — where we cannot, or should not, run it
    /// ourselves. Linux installs need `sudo`; that stays the user's.
    CopyCommand { label: String, command: String },
    /// A fixed installer the app runs, streaming its output.
    RunInstaller { label: String, installer: InstallerId },
    /// A product the app launches.
    StartEngine { label: String, target: StartTarget },
    /// Something to know, with nothing to click.
    Note { text: String },
}

/// A way out of the current state: one product, with its steps in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remedy {
    pub title: String,
    pub body: String,
    pub steps: Vec<Step>,
    /// The one we recommend. The first remedy is always primary; the rest are
    /// alternatives the UI folds away.
    pub primary: bool,
}

/// What is on this machine that an installer could use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prereqs {
    /// Homebrew, resolved to a path because a GUI app's `PATH` may not have it.
    pub brew: Option<PathBuf>,
    pub winget: Option<PathBuf>,
}

impl Prereqs {
    pub fn detect(platform: Platform) -> Prereqs {
        match platform {
            Platform::Macos => Prereqs {
                brew: engine_probe::find_tool("brew"),
                winget: None,
            },
            Platform::Windows => Prereqs {
                brew: None,
                winget: engine_probe::find_tool("winget"),
            },
            _ => Prereqs::default(),
        }
    }
}

/// One thing an installer does, in order. The desktop app executes these; the CLI only
/// ever prints the equivalent `CopyCommand`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallAction {
    /// Run a program. `program` is a bare name the caller resolves, or an absolute path.
    Run {
        title: &'static str,
        program: &'static str,
        args: Vec<&'static str>,
    },
    /// Put Homebrew's `docker-compose` where the `docker` CLI looks for plugins.
    LinkComposePlugin,
    /// Bring the freshly installed product up.
    Launch(StartTarget),
}

impl InstallerId {
    pub fn label(self) -> &'static str {
        match self {
            InstallerId::BrewColima => "Install Colima with Homebrew",
            InstallerId::BrewComposePlugin => "Install the compose plugin with Homebrew",
            InstallerId::WingetRancherDesktop => "Install Rancher Desktop with winget",
        }
    }

    /// The command line a user would run to do the same by hand.
    pub fn command(self) -> &'static str {
        match self {
            InstallerId::BrewColima => {
                "brew install colima docker docker-compose && mkdir -p ~/.docker/cli-plugins && ln -sfn \"$(brew --prefix)/opt/docker-compose/bin/docker-compose\" ~/.docker/cli-plugins/docker-compose && colima start"
            }
            InstallerId::BrewComposePlugin => {
                "brew install docker-compose && mkdir -p ~/.docker/cli-plugins && ln -sfn \"$(brew --prefix)/opt/docker-compose/bin/docker-compose\" ~/.docker/cli-plugins/docker-compose"
            }
            InstallerId::WingetRancherDesktop => {
                "winget install -e --id SUSE.RancherDesktop --accept-package-agreements --accept-source-agreements"
            }
        }
    }

    pub fn plan(self) -> Vec<InstallAction> {
        match self {
            InstallerId::BrewColima => vec![
                InstallAction::Run {
                    title: "Installing Colima, the Docker CLI and Compose",
                    program: "brew",
                    args: vec!["install", "colima", "docker", "docker-compose"],
                },
                InstallAction::LinkComposePlugin,
                InstallAction::Launch(StartTarget::Colima),
            ],
            InstallerId::BrewComposePlugin => vec![
                InstallAction::Run {
                    title: "Installing Compose",
                    program: "brew",
                    args: vec!["install", "docker-compose"],
                },
                InstallAction::LinkComposePlugin,
            ],
            InstallerId::WingetRancherDesktop => vec![
                InstallAction::Run {
                    title: "Installing Rancher Desktop",
                    program: "winget",
                    args: vec![
                        "install",
                        "-e",
                        "--id",
                        "SUSE.RancherDesktop",
                        "--accept-package-agreements",
                        "--accept-source-agreements",
                    ],
                },
                InstallAction::Launch(StartTarget::RancherDesktop),
            ],
        }
    }
}

impl StartTarget {
    pub fn label(self) -> &'static str {
        match self {
            StartTarget::Colima => "Colima",
            StartTarget::DockerDesktop => "Docker Desktop",
            StartTarget::OrbStack => "OrbStack",
            StartTarget::RancherDesktop => "Rancher Desktop",
            StartTarget::PodmanMachine => "the Podman machine",
        }
    }

    /// How to launch it, on this platform. `None` where we have no safe way.
    pub fn launch(self, platform: Platform) -> Option<(String, Vec<String>)> {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        match (self, platform) {
            (StartTarget::Colima, Platform::Macos | Platform::Linux) => {
                Some(("colima".into(), s(&["start"])))
            }
            (StartTarget::PodmanMachine, _) => Some(("podman".into(), s(&["machine", "start"]))),
            (StartTarget::DockerDesktop, Platform::Macos) => Some(("open".into(), s(&["-a", "Docker"]))),
            (StartTarget::OrbStack, Platform::Macos) => Some(("open".into(), s(&["-a", "OrbStack"]))),
            (StartTarget::RancherDesktop, Platform::Macos) => {
                Some(("open".into(), s(&["-a", "Rancher Desktop"])))
            }
            (StartTarget::RancherDesktop, Platform::Windows) => windows_app(
                &["Programs\\Rancher Desktop\\Rancher Desktop.exe"],
                &["Rancher Desktop\\Rancher Desktop.exe"],
            ),
            (StartTarget::DockerDesktop, Platform::Windows) => {
                windows_app(&[], &["Docker\\Docker\\Docker Desktop.exe"])
            }
            _ => None,
        }
    }
}

/// A Windows program to launch, looked for under the user's `LocalAppData` and then
/// `ProgramFiles`. Launched via `explorer.exe`, which detaches it the way a double-click
/// would and never inherits our console.
fn windows_app(local: &[&str], program_files: &[&str]) -> Option<(String, Vec<String>)> {
    let mut paths = Vec::new();
    if let Ok(base) = std::env::var("LOCALAPPDATA") {
        paths.extend(local.iter().map(|rel| PathBuf::from(&base).join(rel)));
    }
    if let Ok(base) = std::env::var("ProgramFiles") {
        paths.extend(program_files.iter().map(|rel| PathBuf::from(&base).join(rel)));
    }
    paths
        .into_iter()
        .find(|p| p.exists())
        .map(|p| ("explorer.exe".to_string(), vec![p.to_string_lossy().into_owned()]))
}

// --- the remedies themselves ------------------------------------------------------------

fn url(label: &str, url: &str) -> Step {
    Step::OpenUrl {
        label: label.into(),
        url: url.into(),
    }
}
fn copy(label: &str, command: &str) -> Step {
    Step::CopyCommand {
        label: label.into(),
        command: command.into(),
    }
}
fn note(text: &str) -> Step {
    Step::Note { text: text.into() }
}
fn run(installer: InstallerId) -> Step {
    Step::RunInstaller {
        label: installer.label().into(),
        installer,
    }
}
fn start(target: StartTarget) -> Step {
    Step::StartEngine {
        label: format!("Start {}", target.label()),
        target,
    }
}
fn remedy(title: &str, body: &str, steps: Vec<Step>) -> Remedy {
    Remedy {
        title: title.into(),
        body: body.into(),
        steps,
        primary: false,
    }
}

const DOCKER_DESKTOP_URL: &str = "https://docs.docker.com/get-started/get-docker/";
const PODMAN_DESKTOP_URL: &str = "https://podman-desktop.io/downloads";
const ORBSTACK_URL: &str = "https://orbstack.dev/download";
const RANCHER_URL: &str = "https://rancherdesktop.io/";
const COLIMA_URL: &str = "https://github.com/abiosoft/colima#installation";
const HOMEBREW_URL: &str = "https://brew.sh";
const COMPOSE_INSTALL_URL: &str = "https://docs.docker.com/compose/install/";
const LINUX_ENGINE_URL: &str = "https://docs.docker.com/engine/install/";
const LINUX_POSTINSTALL_URL: &str = "https://docs.docker.com/engine/install/linux-postinstall/";
const WSL_URL: &str = "https://learn.microsoft.com/windows/wsl/install";

const HOMEBREW_INSTALL: &str =
    "/bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"";
const LINUX_GET_DOCKER: &str = "curl -fsSL https://get.docker.com | sudo sh";
const LINUX_USERMOD: &str = "sudo usermod -aG docker \"$USER\"";
const LINUX_PODMAN_DEBIAN: &str = "sudo apt-get install -y podman podman-docker podman-compose";
const LINUX_PODMAN_FEDORA: &str = "sudo dnf install -y podman podman-docker podman-compose";

/// The remedies for the state the probe found, primary first.
pub fn remedies(
    state: DockerState,
    brand: EngineBrand,
    kind: Option<EngineKind>,
    platform: Platform,
    prereqs: &Prereqs,
) -> Vec<Remedy> {
    let mut out = match state {
        DockerState::Ready => Vec::new(),
        DockerState::Missing => missing(platform, prereqs),
        DockerState::NoCompose => no_compose(brand, kind, platform, prereqs),
        DockerState::NoDaemon => no_daemon(brand, kind, platform),
        DockerState::TooOld => too_old(brand, platform),
    };
    if let Some(first) = out.first_mut() {
        first.primary = true;
    }
    out
}

fn missing(platform: Platform, prereqs: &Prereqs) -> Vec<Remedy> {
    match platform {
        Platform::Macos => {
            let colima_steps = if prereqs.brew.is_some() {
                vec![
                    run(InstallerId::BrewColima),
                    copy("Or run it yourself", InstallerId::BrewColima.command()),
                ]
            } else {
                vec![
                    note("Colima installs through Homebrew, which is not on this machine yet. Install Homebrew first, then check again and this becomes one click."),
                    copy("Install Homebrew", HOMEBREW_INSTALL),
                    url("About Homebrew", HOMEBREW_URL),
                    copy("Then install Colima", InstallerId::BrewColima.command()),
                ]
            };
            vec![
                remedy(
                    "Colima",
                    "Open source, and the lightest way to run Docker on a Mac: a small Linux VM with a real `docker` command line and Compose. Nothing runs until you start it, and it stops when you tell it to.",
                    colima_steps,
                ),
                remedy(
                    "OrbStack",
                    "A fast, polished alternative. Free for personal use; a licence is needed for commercial use.",
                    vec![url("Download OrbStack", ORBSTACK_URL)],
                ),
                remedy(
                    "Podman Desktop",
                    "Open source, from Red Hat. Konstruktor drives Podman the same way it drives Docker, though Compose support is a step behind.",
                    vec![url("Download Podman Desktop", PODMAN_DESKTOP_URL)],
                ),
                remedy(
                    "Docker Desktop",
                    "Docker's own app. It works, but it is not open source and larger organisations need a paid subscription to use it.",
                    vec![url("Download Docker Desktop", DOCKER_DESKTOP_URL)],
                ),
            ]
        }
        Platform::Windows => {
            let mut rancher_steps = Vec::new();
            if prereqs.winget.is_some() {
                rancher_steps.push(run(InstallerId::WingetRancherDesktop));
            }
            rancher_steps.push(copy(
                if prereqs.winget.is_some() { "Or run it yourself" } else { "Install with winget" },
                InstallerId::WingetRancherDesktop.command(),
            ));
            rancher_steps.push(url("Download Rancher Desktop", RANCHER_URL));
            rancher_steps.push(note("Rancher Desktop runs on WSL 2. If Windows has to enable it first, the installer will say so, and a restart may be needed before the engine can start. When it first opens, choose the dockerd (moby) container runtime — Kubernetes can stay off."));
            rancher_steps.push(url("About WSL 2", WSL_URL));
            vec![
                remedy(
                    "Rancher Desktop",
                    "Open source, from SUSE, and gives a real `docker` command line with Compose on Windows.",
                    rancher_steps,
                ),
                remedy(
                    "Podman Desktop",
                    "Open source, from Red Hat. Konstruktor drives Podman the same way it drives Docker, though Compose support is a step behind.",
                    vec![
                        copy("Install with winget", "winget install -e --id RedHat.Podman-Desktop"),
                        url("Download Podman Desktop", PODMAN_DESKTOP_URL),
                    ],
                ),
                remedy(
                    "Docker Desktop",
                    "Docker's own app. It works, but it is not open source and larger organisations need a paid subscription to use it.",
                    vec![url("Download Docker Desktop", DOCKER_DESKTOP_URL)],
                ),
            ]
        }
        Platform::Linux | Platform::Other => vec![
            remedy(
                "Docker Engine",
                "The engine itself, straight from your distribution or Docker's repository. No desktop app, no VM — containers run on this kernel.",
                vec![
                    copy("Install Docker Engine and Compose", LINUX_GET_DOCKER),
                    copy("Let your user use it without sudo", LINUX_USERMOD),
                    note("Log out and back in after adding yourself to the docker group, then check again. Konstruktor runs `docker` as you, so it has to be allowed to reach the daemon without sudo."),
                    url("Install from your distribution's packages instead", LINUX_ENGINE_URL),
                    url("The post-install steps", LINUX_POSTINSTALL_URL),
                ],
            ),
            remedy(
                "Podman",
                "Daemonless and rootless by default. Konstruktor drives it through the same commands; `podman-docker` provides the `docker` name and `podman-compose` the Compose subcommand.",
                vec![
                    copy("Debian and Ubuntu", LINUX_PODMAN_DEBIAN),
                    copy("Fedora", LINUX_PODMAN_FEDORA),
                    note("Compose support is a step behind Docker's; if a stack misbehaves under Podman, Docker Engine is the safer choice."),
                ],
            ),
        ],
    }
}

fn no_compose(
    brand: EngineBrand,
    kind: Option<EngineKind>,
    platform: Platform,
    prereqs: &Prereqs,
) -> Vec<Remedy> {
    if kind == Some(EngineKind::Podman) {
        return vec![remedy(
            "Compose for Podman",
            "Podman answers `compose` by handing it to `podman-compose` or Docker's plugin, and neither is installed.",
            vec![
                copy("Debian and Ubuntu", "sudo apt-get install -y podman-compose"),
                copy("Fedora", "sudo dnf install -y podman-compose"),
                copy("macOS and Windows, with pip", "pip install podman-compose"),
                url("Podman and Compose", "https://docs.podman.io/en/latest/markdown/podman-compose.1.html"),
            ],
        )];
    }
    match (platform, brand) {
        (Platform::Macos, _) => {
            let steps = if prereqs.brew.is_some() {
                vec![
                    run(InstallerId::BrewComposePlugin),
                    copy("Or run it yourself", InstallerId::BrewComposePlugin.command()),
                ]
            } else {
                vec![copy("Install and link the plugin", InstallerId::BrewComposePlugin.command())]
            };
            vec![
                remedy(
                    "The Compose plugin",
                    "Your `docker` command line is there, but the `compose` plugin is not beside it. Homebrew ships it as `docker-compose`; it then has to be linked where the CLI looks for plugins.",
                    steps,
                ),
                remedy(
                    "Update the app instead",
                    "Docker Desktop, OrbStack and Rancher Desktop all bundle Compose — a current version of whichever you use brings it back.",
                    vec![url("Installing Compose", COMPOSE_INSTALL_URL)],
                ),
            ]
        }
        (Platform::Windows, EngineBrand::RancherDesktop) => vec![remedy(
            "Rancher Desktop's container runtime",
            "Rancher Desktop only provides `docker compose` when its container runtime is set to dockerd (moby). Open its Preferences, choose dockerd under Container Engine, and let it restart.",
            vec![url("Rancher Desktop preferences", "https://docs.rancherdesktop.io/ui/preferences/container-engine/general")],
        )],
        (Platform::Windows, _) => vec![remedy(
            "Update the app",
            "Rancher Desktop, Docker Desktop and Podman Desktop all bundle Compose with the `docker` command line; a current version brings it back.",
            vec![url("Installing Compose", COMPOSE_INSTALL_URL)],
        )],
        (_, _) => vec![remedy(
            "The Compose plugin",
            "Your `docker` command line is there, but the `compose` plugin is a separate package.",
            vec![
                copy("Debian and Ubuntu", "sudo apt-get install -y docker-compose-plugin"),
                copy("Fedora", "sudo dnf install -y docker-compose-plugin"),
                copy("Arch", "sudo pacman -S docker-compose"),
                url("Installing Compose", COMPOSE_INSTALL_URL),
            ],
        )],
    }
}

fn no_daemon(brand: EngineBrand, kind: Option<EngineKind>, platform: Platform) -> Vec<Remedy> {
    let started = |target: StartTarget, body: &str| {
        let mut steps = Vec::new();
        if target.launch(platform).is_some() {
            steps.push(start(target));
        }
        remedy(&format!("Start {}", target.label()), body, steps)
    };
    match brand {
        EngineBrand::Colima => vec![remedy(
            "Start Colima",
            "The command line is there, but Colima's VM is not running. Starting it brings the daemon up; this takes a few seconds.",
            vec![start(StartTarget::Colima), copy("Or in a terminal", "colima start")],
        )],
        EngineBrand::DockerDesktop => vec![started(
            StartTarget::DockerDesktop,
            "Docker Desktop is installed but not running. Open it and wait for the whale to settle.",
        )],
        EngineBrand::OrbStack => vec![started(
            StartTarget::OrbStack,
            "OrbStack is installed but not running. Open it; the daemon comes up in a moment.",
        )],
        EngineBrand::RancherDesktop => vec![started(
            StartTarget::RancherDesktop,
            "Rancher Desktop is installed but not running. Open it and wait until it reports the engine is up.",
        )],
        EngineBrand::PodmanDesktop => vec![remedy(
            "Start the Podman machine",
            "Podman on a desktop OS runs inside a machine that has to be started first.",
            vec![
                start(StartTarget::PodmanMachine),
                copy("Or in a terminal", "podman machine start"),
                note("If no machine exists yet: `podman machine init` once, then start it."),
            ],
        )],
        EngineBrand::Native if kind == Some(EngineKind::Podman) => vec![remedy(
            "Start the Podman service",
            "Konstruktor talks to Podman over its API socket, which is served by a systemd unit.",
            vec![
                copy("Start it for your user", "systemctl --user enable --now podman.socket"),
                copy("Or system-wide", "sudo systemctl enable --now podman.socket"),
            ],
        )],
        EngineBrand::Native => vec![remedy(
            "Start the daemon",
            "The `docker` command line is there, but nothing is answering at the socket.",
            vec![
                copy("Start it, and again on boot", "sudo systemctl enable --now docker"),
                note("If it is already running but this still fails, your user is probably not in the docker group."),
                copy("Let your user use it without sudo", LINUX_USERMOD),
                url("The post-install steps", LINUX_POSTINSTALL_URL),
            ],
        )],
        EngineBrand::Unknown => vec![remedy(
            "Start the engine",
            "The command line is there, but its daemon is not answering. Start whichever app provides Docker on this machine and check again.",
            match platform {
                Platform::Linux => vec![copy("If it is a plain daemon", "sudo systemctl enable --now docker")],
                _ => Vec::new(),
            },
        )],
    }
}

fn too_old(brand: EngineBrand, platform: Platform) -> Vec<Remedy> {
    let body = "Konstruktor needs Compose 2.20 or newer and Engine API 1.41 or newer. What is installed is older than that.";
    match (platform, brand) {
        (Platform::Macos, EngineBrand::Colima) | (Platform::Macos, EngineBrand::Native) => vec![remedy(
            "Update with Homebrew",
            body,
            vec![copy("Upgrade", "brew upgrade colima docker docker-compose"), url("Colima", COLIMA_URL)],
        )],
        (_, EngineBrand::DockerDesktop) => vec![remedy(
            "Update Docker Desktop",
            body,
            vec![url("Docker Desktop release notes", "https://docs.docker.com/desktop/release-notes/")],
        )],
        (_, EngineBrand::OrbStack) => vec![remedy("Update OrbStack", body, vec![url("OrbStack", ORBSTACK_URL)])],
        (_, EngineBrand::RancherDesktop) => vec![remedy("Update Rancher Desktop", body, vec![url("Rancher Desktop", RANCHER_URL)])],
        (Platform::Linux, _) => vec![remedy(
            "Update Docker Engine",
            body,
            vec![
                copy("Debian and Ubuntu", "sudo apt-get update && sudo apt-get install -y docker-ce docker-ce-cli docker-compose-plugin"),
                copy("Fedora", "sudo dnf upgrade -y docker-ce docker-ce-cli docker-compose-plugin"),
                url("Installing a current engine", LINUX_ENGINE_URL),
            ],
        )],
        _ => vec![remedy("Update your engine", body, vec![url("Installing Compose", COMPOSE_INSTALL_URL)])],
    }
}

/// The probe's verdict and its primary remedy as text, for the command line and for
/// error messages. Written once here so `konstruktor doctor` and the app agree.
pub fn describe(probe: &DockerProbe) -> String {
    let name = probe.engine_label();
    let headline = match probe.state() {
        DockerState::Ready => return format!("{name} is ready."),
        DockerState::Missing => "No container engine is installed. Konstruktor hands the finished deployment to Docker Compose, so one has to be on this machine.".to_string(),
        DockerState::NoCompose => format!("{name} is installed, but `compose` is not."),
        DockerState::NoDaemon => format!("{name} is installed, but the daemon is not answering."),
        DockerState::TooOld => format!("{name} is installed, but too old."),
    };
    let mut text = headline;
    if let Some(primary) = probe.remedies.first() {
        text.push_str(&format!("\n\n{}: {}", primary.title, primary.body));
        for step in &primary.steps {
            match step {
                Step::OpenUrl { label, url } => text.push_str(&format!("\n  {label}: {url}")),
                Step::CopyCommand { label, command } => text.push_str(&format!("\n  {label}:\n    {command}")),
                Step::RunInstaller { installer, .. } => {
                    text.push_str(&format!("\n  Install:\n    {}", installer.command()))
                }
                Step::StartEngine { label, .. } => text.push_str(&format!("\n  {label}.")),
                Step::Note { text: t } => text.push_str(&format!("\n  {t}")),
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_brew() -> Prereqs {
        Prereqs {
            brew: Some(PathBuf::from("/opt/homebrew/bin/brew")),
            winget: None,
        }
    }

    fn with_winget() -> Prereqs {
        Prereqs {
            brew: None,
            winget: Some(PathBuf::from("winget")),
        }
    }

    fn has_installer(remedy: &Remedy, id: InstallerId) -> bool {
        remedy
            .steps
            .iter()
            .any(|s| matches!(s, Step::RunInstaller { installer, .. } if *installer == id))
    }

    /// The whole point: an open-source engine is what we send people to, on every OS,
    /// and Docker Desktop is never the first card.
    #[test]
    fn recommends_open_source_first() {
        let mac = remedies(DockerState::Missing, EngineBrand::Unknown, None, Platform::Macos, &with_brew());
        assert_eq!(mac[0].title, "Colima");
        assert!(mac[0].primary);
        assert!(has_installer(&mac[0], InstallerId::BrewColima));

        let win = remedies(DockerState::Missing, EngineBrand::Unknown, None, Platform::Windows, &with_winget());
        assert_eq!(win[0].title, "Rancher Desktop");
        assert!(has_installer(&win[0], InstallerId::WingetRancherDesktop));

        let linux = remedies(DockerState::Missing, EngineBrand::Unknown, None, Platform::Linux, &Prereqs::default());
        assert_eq!(linux[0].title, "Docker Engine");

        for set in [&mac, &win, &linux] {
            assert!(set.iter().skip(1).all(|r| !r.primary));
            assert_ne!(set[0].title, "Docker Desktop");
        }
    }

    /// Without Homebrew there is nothing to click: the one-click step must not be
    /// offered, and the Homebrew install must be.
    #[test]
    fn falls_back_to_copyable_commands_without_a_package_manager() {
        let mac = remedies(DockerState::Missing, EngineBrand::Unknown, None, Platform::Macos, &Prereqs::default());
        assert!(!has_installer(&mac[0], InstallerId::BrewColima));
        assert!(mac[0].steps.iter().any(|s| matches!(s, Step::CopyCommand { command, .. } if command.contains("brew.sh") || command.contains("Homebrew"))));

        let win = remedies(DockerState::Missing, EngineBrand::Unknown, None, Platform::Windows, &Prereqs::default());
        assert!(!has_installer(&win[0], InstallerId::WingetRancherDesktop));
        assert!(win[0].steps.iter().any(|s| matches!(s, Step::CopyCommand { .. })));
    }

    /// Linux never gets a button that runs `sudo` for the user.
    #[test]
    fn never_runs_an_installer_on_linux() {
        for state in [DockerState::Missing, DockerState::NoCompose, DockerState::NoDaemon, DockerState::TooOld] {
            for brand in [EngineBrand::Native, EngineBrand::Unknown, EngineBrand::DockerDesktop] {
                let set = remedies(state, brand, Some(EngineKind::Docker), Platform::Linux, &Prereqs::default());
                assert!(set.iter().flat_map(|r| &r.steps).all(|s| !matches!(s, Step::RunInstaller { .. })), "{state:?} {brand:?}");
            }
        }
    }

    /// A stopped daemon names the product that has to be started, not "Docker".
    #[test]
    fn a_silent_daemon_names_its_product() {
        let colima = remedies(DockerState::NoDaemon, EngineBrand::Colima, Some(EngineKind::Docker), Platform::Macos, &with_brew());
        assert_eq!(colima[0].title, "Start Colima");
        assert!(colima[0].steps.iter().any(|s| matches!(s, Step::StartEngine { target: StartTarget::Colima, .. })));

        let orb = remedies(DockerState::NoDaemon, EngineBrand::OrbStack, Some(EngineKind::Docker), Platform::Macos, &with_brew());
        assert_eq!(orb[0].title, "Start OrbStack");

        let native = remedies(DockerState::NoDaemon, EngineBrand::Native, Some(EngineKind::Docker), Platform::Linux, &Prereqs::default());
        assert!(native[0].steps.iter().any(|s| matches!(s, Step::CopyCommand { command, .. } if command.contains("systemctl"))));
    }

    #[test]
    fn ready_needs_no_remedy() {
        assert!(remedies(DockerState::Ready, EngineBrand::Colima, Some(EngineKind::Docker), Platform::Macos, &with_brew()).is_empty());
    }

    /// Nothing an installer runs may come from anywhere but this file.
    #[test]
    fn every_installer_plan_is_literal() {
        for id in [InstallerId::BrewColima, InstallerId::BrewComposePlugin, InstallerId::WingetRancherDesktop] {
            let plan = id.plan();
            assert!(!plan.is_empty());
            assert!(matches!(plan[0], InstallAction::Run { .. }));
            assert!(!id.command().is_empty());
        }
    }
}
