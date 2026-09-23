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
    /// Rancher Desktop is installed on Windows, whether or not its CLI can be found.
    pub rancher_desktop: bool,
}

impl Prereqs {
    pub fn detect(platform: Platform) -> Prereqs {
        match platform {
            Platform::Macos => Prereqs {
                brew: engine_probe::find_tool("brew"),
                ..Prereqs::default()
            },
            Platform::Windows => Prereqs {
                winget: engine_probe::find_tool("winget"),
                rancher_desktop: !engine_probe::rancher_desktop_installs().is_empty(),
                ..Prereqs::default()
            },
            _ => Prereqs::default(),
        }
    }
}

/// One thing an installer does, in order. Both front ends execute these through
/// [`install`]; a step needing sudo is a `CopyCommand` on the remedy instead, and stays
/// the user's to paste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallAction {
    /// Run a program. `program` is a bare name the caller resolves, or an absolute path.
    Run {
        title: &'static str,
        program: &'static str,
        args: Vec<&'static str>,
        /// On Windows, when the program fails saying it needs administrator rights, run
        /// the same command again through a UAC prompt. Only asked for when needed, so a
        /// per-user install never sees the prompt.
        elevate_if_denied: bool,
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

    /// The platform this installer is for. The gate used to be spelled out again in the
    /// desktop command layer; it belongs with the plan it guards, so there is one place
    /// that knows `brew` is macOS and `winget` is Windows.
    pub fn platform(self) -> Platform {
        match self {
            InstallerId::BrewColima | InstallerId::BrewComposePlugin => Platform::Macos,
            InstallerId::WingetRancherDesktop => Platform::Windows,
        }
    }

    pub fn plan(self) -> Vec<InstallAction> {
        match self {
            InstallerId::BrewColima => vec![
                InstallAction::Run {
                    title: "Installing Colima, the Docker CLI and Compose",
                    program: "brew",
                    args: vec!["install", "colima", "docker", "docker-compose"],
                    elevate_if_denied: false,
                },
                InstallAction::LinkComposePlugin,
                InstallAction::Launch(StartTarget::Colima),
            ],
            InstallerId::BrewComposePlugin => vec![
                InstallAction::Run {
                    title: "Installing Compose",
                    program: "brew",
                    args: vec!["install", "docker-compose"],
                    elevate_if_denied: false,
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
                    // Its WSL dependency, and the all-users MSI, need an administrator.
                    elevate_if_denied: true,
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
        // Installed, but no CLI to be found: Rancher Desktop only puts `docker` in place
        // on its first start, or — for a per-user install — in `~/.rd/bin` once set up.
        // Installing it again would not help.
        Platform::Windows if prereqs.rancher_desktop => vec![remedy(
            "Finish setting up Rancher Desktop",
            "Rancher Desktop is installed, but its `docker` command line is not in place yet. Start it once and let it finish its first-run setup; this page picks it up by itself.",
            vec![
                start(StartTarget::RancherDesktop),
                note("When it asks, choose dockerd (moby) as the container engine — Kubernetes can stay off."),
                note("If it says WSL is missing or needs a restart, or it was just installed and nothing happens, restart Windows and open Konstruktor again."),
            ],
        )],
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
        EngineBrand::RancherDesktop if platform == Platform::Windows => {
            let mut steps = Vec::new();
            if StartTarget::RancherDesktop.launch(platform).is_some() {
                steps.push(start(StartTarget::RancherDesktop));
            }
            steps.push(note("The first start sets up its WSL distribution and takes a few minutes. When it asks, choose dockerd (moby) as the container engine — Kubernetes can stay off. This page turns green by itself once the engine answers."));
            steps.push(note("If Rancher Desktop says WSL is missing or needs an update, or it never gets past starting, restart Windows: a fresh WSL install only works after a restart."));
            vec![remedy(
                "Start Rancher Desktop",
                "Rancher Desktop is installed, but its engine is not answering yet. Open it and wait until it reports the engine is up.",
                steps,
            )]
        }
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
            ..Prereqs::default()
        }
    }

    fn with_winget() -> Prereqs {
        Prereqs {
            winget: Some(PathBuf::from("winget")),
            ..Prereqs::default()
        }
    }

    /// Installed but no CLI yet — the state right after winget, before a first start or
    /// a restart. Offering the install again would only repeat it.
    #[test]
    fn installed_rancher_is_not_offered_again() {
        let prereqs = Prereqs {
            rancher_desktop: true,
            ..with_winget()
        };
        let win = remedies(DockerState::Missing, EngineBrand::Unknown, None, Platform::Windows, &prereqs);
        assert_eq!(win[0].title, "Finish setting up Rancher Desktop");
        assert!(!has_installer(&win[0], InstallerId::WingetRancherDesktop));
        assert!(win[0].steps.iter().any(|s| matches!(s, Step::StartEngine { target: StartTarget::RancherDesktop, .. })));
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

    #[test]
    fn winget_admin_failure_is_recognised() {
        let out = "Installer failed with exit code: 0x80073d28 : The package installation failed because administrator privileges are required.";
        assert!(needs_admin(1, out));
        assert!(needs_admin(0x8007_3D28_u32 as i32, ""));
        assert!(!needs_admin(1, "Installer hash does not match"));
    }
}

// --- running a remedy ------------------------------------------------------------------
//
// Everything above *describes* what would fix a machine; everything below runs it. The two
// were split across crates until now — the plans here, the executor in the desktop app —
// which meant `konstruktor doctor` could name a remedy it had no way to apply.
//
// Nothing here decides anything: every program and argument is a literal in `plan()`, and
// this only spawns them.

use tokio_util::sync::CancellationToken;

/// One line of an installer's output, plus the stage markers a front end uses as headings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallLine {
    pub line: String,
    pub stderr: bool,
    /// Set on the line that opens a new stage — "Installing Colima…" — and on nothing
    /// else, so a panel can render those as headings rather than as output.
    pub stage: bool,
}

/// How the installer ended. A failure is an *outcome*, not an `Err`: the output it
/// streamed is the explanation, and an `Err` would only repeat the last line of it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallOutcome {
    pub ok: bool,
    /// The installer said Windows has to restart before the engine can start. Surfaced,
    /// never hidden: the next probe would otherwise keep failing without saying why.
    pub needs_reboot: bool,
    pub cancelled: bool,
    pub message: Option<String>,
}

fn cancelled() -> InstallOutcome {
    InstallOutcome {
        ok: false,
        needs_reboot: false,
        cancelled: true,
        message: Some("cancelled".into()),
    }
}

fn clean(raw: &str) -> String {
    String::from_utf8_lossy(&strip_ansi_escapes::strip(raw)).into_owned()
}

/// A program name from a plan, as something that can be spawned. Bare names are looked
/// up the way the engine binary is; `open` and `explorer.exe` are always on `PATH`.
fn resolve_program(program: &str) -> Result<std::path::PathBuf, String> {
    let path = std::path::PathBuf::from(program);
    if path.is_absolute() || matches!(program, "open" | "explorer.exe") {
        return Ok(path);
    }
    crate::engine_probe::find_tool(program)
        .ok_or_else(|| format!("`{program}` was not found on this machine"))
}

/// Runs one of the fixed installers, streaming its output over `on_line`.
///
/// The program is resolved the way the engine binary is, so a Homebrew that a
/// Finder-launched app cannot see on `PATH` is still found.
pub async fn install(
    installer: InstallerId,
    cancel: &CancellationToken,
    on_line: &(dyn Fn(InstallLine) + Sync),
) -> Result<InstallOutcome, String> {
    let platform = Platform::current();
    if installer.platform() != platform {
        return Err(format!("{installer:?} is not an installer for this platform"));
    }
    run_plan(installer.plan(), platform, cancel, on_line).await
}

/// Launches the product behind a stopped daemon — Colima, OrbStack, Docker Desktop… —
/// and returns without waiting for it. A probe's polling notices when it is up.
pub async fn launch(target: StartTarget) -> Result<(), String> {
    let (program, args) = target
        .launch(Platform::current())
        .ok_or_else(|| format!("{} cannot be started from here", target.label()))?;
    let program = resolve_program(&program)?;
    crate::process::async_command(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Restarts Windows, for an installer that said it needs one before the engine can
/// start. `shutdown` gives running programs a few seconds' notice; a standard user may
/// restart their own workstation, so no elevation is involved.
pub async fn restart_computer() -> Result<(), String> {
    if Platform::current() != Platform::Windows {
        return Err("Restarting from here is only supported on Windows".into());
    }
    // `p:4:2`: planned, application installation — what the event log files it under.
    let status = crate::process::async_command("shutdown.exe")
        .args(["/r", "/t", "5", "/d", "p:4:2", "/c", "Konstruktor: finishing the container engine install"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Windows refused to restart (exit code {})", status.code().unwrap_or(-1)))
    }
}

async fn run_plan(
    plan: Vec<InstallAction>,
    platform: Platform,
    token: &CancellationToken,
    on_line: &(dyn Fn(InstallLine) + Sync),
) -> Result<InstallOutcome, String> {
    let stage = |text: &str| {
        on_line(InstallLine {
            line: text.to_string(),
            stderr: false,
            stage: true,
        });
    };
    let mut needs_reboot = false;

    for action in plan {
        if token.is_cancelled() {
            return Ok(cancelled());
        }
        match action {
            InstallAction::Run {
                title,
                program,
                args,
                elevate_if_denied,
            } => {
                stage(title);
                let program = resolve_program(program)?;
                let mut cmd = crate::process::async_command(&program);
                cmd.args(&args);
                // Homebrew: no prompts, no hints, and no minutes-long `brew update`
                // before the install the user asked for.
                cmd.env("NONINTERACTIVE", "1")
                    .env("HOMEBREW_NO_ENV_HINTS", "1")
                    .env("HOMEBREW_NO_AUTO_UPDATE", "1");
                let (mut status, mut output) = stream(cmd, token, on_line).await?;
                let mut elevated = false;
                if cfg!(windows)
                    && elevate_if_denied
                    && status.is_some_and(|s| !s.success() && needs_admin(s.code().unwrap_or(-1), &output))
                {
                    stage("Asking Windows for administrator rights");
                    (status, output) = stream_elevated(&program, &args, token, on_line).await?;
                    elevated = true;
                }
                let Some(status) = status else {
                    return Ok(cancelled());
                };
                let code = status.code().unwrap_or(-1);
                if elevated && code == ERROR_CANCELLED {
                    return Ok(InstallOutcome {
                        ok: false,
                        needs_reboot,
                        cancelled: false,
                        message: Some(format!(
                            "{title} needs administrator rights, and the prompt was declined"
                        )),
                    });
                }
                // winget's "installed, restart to finish" codes, and the word itself.
                let restart_hinted = program.to_string_lossy().contains("winget")
                    && (code == 3010
                        || code == 1641
                        || output.to_ascii_lowercase().contains("restart"));
                needs_reboot |= restart_hinted;
                if !status.success() && !(restart_hinted && (code == 3010 || code == 1641)) {
                    return Ok(InstallOutcome {
                        ok: false,
                        needs_reboot,
                        cancelled: false,
                        message: Some(format!("{title} failed (exit code {code})")),
                    });
                }
            }
            InstallAction::LinkComposePlugin => {
                stage("Linking Compose where the Docker CLI looks for plugins");
                link_compose_plugin(on_line).await?;
            }
            InstallAction::Launch(target) => {
                stage(&format!("Starting {}", target.label()));
                let Some((program, args)) = target.launch(platform) else {
                    continue;
                };
                let program = resolve_program(&program)?;
                if target == StartTarget::Colima {
                    // `colima start` is the install's last, and longest, step: it
                    // downloads a VM image the first time. Worth watching.
                    let mut cmd = crate::process::async_command(&program);
                    cmd.args(&args);
                    let (status, _) = stream(cmd, token, on_line).await?;
                    match status {
                        None => return Ok(cancelled()),
                        Some(s) if !s.success() => {
                            return Ok(InstallOutcome {
                                ok: false,
                                needs_reboot,
                                cancelled: false,
                                message: Some("Colima did not start".into()),
                            })
                        }
                        Some(_) => {}
                    }
                } else {
                    crate::process::async_command(program)
                        .args(args)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .map_err(|e| e.to_string())?;
                }
            }
        }
    }

    Ok(InstallOutcome {
        ok: true,
        needs_reboot,
        cancelled: false,
        message: None,
    })
}

/// Runs a command, forwarding every line, until it exits or `token` fires. `None` for the
/// status means it was cancelled — and killed, not abandoned.
async fn stream(
    mut cmd: tokio::process::Command,
    token: &CancellationToken,
    on_line: &(dyn Fn(InstallLine) + Sync),
) -> Result<(Option<std::process::ExitStatus>, String), String> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| e.to_string())?;

    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;

    // Both pipes are read here rather than in spawned tasks: `on_line` borrows, so it
    // cannot be moved into a `'static` task the way a Tauri channel could. Reading them
    // concurrently in one future is what keeps a chatty installer from filling a pipe
    // buffer and blocking on the other.
    let mut out_lines = BufReader::new(stdout).lines();
    let mut err_lines = BufReader::new(stderr).lines();
    let mut collected = String::new();
    let mut out_done = false;
    let mut err_done = false;

    let status = loop {
        if out_done && err_done {
            break tokio::select! {
                status = child.wait() => Some(status.map_err(|e| e.to_string())?),
                _ = token.cancelled() => {
                    let _ = child.kill().await;
                    None
                }
            };
        }
        tokio::select! {
            line = out_lines.next_line(), if !out_done => match line {
                Ok(Some(raw)) => {
                    let line = clean(&raw);
                    collected.push_str(&line);
                    collected.push('\n');
                    on_line(InstallLine { line, stderr: false, stage: false });
                }
                _ => out_done = true,
            },
            line = err_lines.next_line(), if !err_done => match line {
                Ok(Some(raw)) => {
                    let line = clean(&raw);
                    if !line.trim().is_empty() {
                        collected.push_str(&line);
                        collected.push('\n');
                        on_line(InstallLine { line, stderr: true, stage: false });
                    }
                }
                _ => err_done = true,
            },
            _ = token.cancelled() => {
                let _ = child.kill().await;
                break None;
            }
        }
    };

    Ok((status, collected))
}

/// `ERROR_CANCELLED`: what the elevation wrapper exits with when the UAC prompt is declined.
const ERROR_CANCELLED: i32 = 1223;

/// Whether a failed Windows installer failed for want of administrator rights. winget
/// prints the package's HRESULT — `0x80073d28` for an MSIX dependency like WSL — along
/// with a sentence saying so; either is enough.
fn needs_admin(code: i32, output: &str) -> bool {
    const APPX_NEEDS_ADMIN: i32 = 0x8007_3D28_u32 as i32;
    let output = output.to_ascii_lowercase();
    code == APPX_NEEDS_ADMIN
        || output.contains("0x80073d28")
        || output.contains("administrator privileges are required")
        || output.contains("requires administrator")
}

/// Asks UAC to elevate `Start-Process -Verb RunAs`, and exits with the elevated command's
/// code — or `ERROR_CANCELLED` when the prompt is declined. The command line comes in
/// through the environment so none of it is parsed as PowerShell.
const ELEVATE_PS: &str = "try { \
    $p = Start-Process -FilePath $env:ComSpec -ArgumentList $env:KONSTRUKTOR_ELEVATED \
         -Verb RunAs -WindowStyle Hidden -Wait -PassThru -ErrorAction Stop; \
    exit $p.ExitCode \
} catch { exit 1223 }";

/// Runs `program` elevated, streaming what it prints the way [`stream`] does.
///
/// An elevated process cannot write to a non-elevated parent's pipes, so its output goes
/// to a log file in the temp directory and is tailed from there. Cancelling stops the
/// wait, not the installer: a non-elevated process has no right to kill an elevated one.
async fn stream_elevated(
    program: &std::path::Path,
    args: &[&str],
    token: &CancellationToken,
    on_line: &(dyn Fn(InstallLine) + Sync),
) -> Result<(Option<std::process::ExitStatus>, String), String> {
    let log = std::env::temp_dir().join(format!("konstruktor-elevated-{}.log", std::process::id()));
    tokio::fs::write(&log, b"").await.map_err(|e| e.to_string())?;
    // `/s` strips the outer pair of quotes and leaves the rest as written; every piece
    // is a literal from `plan()` or a path we resolved, and no Windows path holds a `"`.
    let command_line = format!(
        "/d /s /c \"\"{}\" {} > \"{}\" 2>&1\"",
        program.display(),
        args.join(" "),
        log.display()
    );
    let mut child = crate::process::async_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", ELEVATE_PS])
        .env("KONSTRUKTOR_ELEVATED", command_line)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| e.to_string())?;

    let mut read = 0usize;
    let mut pending = Vec::new();
    let mut collected = String::new();
    let mut emit = |raw: &[u8]| {
        let line = clean(String::from_utf8_lossy(raw).trim_end_matches(['\r', '\n']));
        if !line.trim().is_empty() {
            collected.push_str(&line);
            collected.push('\n');
            on_line(InstallLine { line, stderr: false, stage: false });
        }
    };

    let status = loop {
        let exited = tokio::select! {
            status = child.wait() => Some(status.map_err(|e| e.to_string())?),
            _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => None,
            _ = token.cancelled() => {
                let _ = child.kill().await;
                break None;
            }
        };
        if let Ok(bytes) = tokio::fs::read(&log).await {
            if bytes.len() > read {
                pending.extend_from_slice(&bytes[read..]);
                read = bytes.len();
            }
        }
        while let Some(end) = pending.iter().position(|&b| b == b'\n') {
            let raw: Vec<u8> = pending.drain(..=end).collect();
            emit(&raw);
        }
        if let Some(status) = exited {
            emit(&std::mem::take(&mut pending));
            break Some(status);
        }
    };

    let _ = tokio::fs::remove_file(&log).await;
    Ok((status, collected))
}

/// Homebrew installs `docker-compose` as a standalone binary; the `docker` CLI only finds
/// it as `docker compose` through `~/.docker/cli-plugins`. Done here rather than by a
/// shell one-liner so there is no shell, and no quoting, between us and the path.
async fn link_compose_plugin(on_line: &(dyn Fn(InstallLine) + Sync)) -> Result<(), String> {
    let brew = resolve_program("brew")?;
    let output = crate::process::async_command(brew)
        .args(["--prefix", "docker-compose"])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("`brew --prefix docker-compose` failed — is docker-compose installed?".into());
    }
    let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let binary = std::path::PathBuf::from(prefix)
        .join("bin")
        .join("docker-compose");
    if !binary.is_file() {
        return Err(format!("{} is not there", binary.display()));
    }

    let home = dirs::home_dir().ok_or("no home directory")?;
    let plugins = home.join(".docker").join("cli-plugins");
    std::fs::create_dir_all(&plugins).map_err(|e| e.to_string())?;
    let link = plugins.join("docker-compose");
    if link.exists() || link.symlink_metadata().is_ok() {
        std::fs::remove_file(&link).map_err(|e| e.to_string())?;
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&binary, &link).map_err(|e| e.to_string())?;
    #[cfg(not(unix))]
    std::fs::copy(&binary, &link).map_err(|e| e.to_string())?;

    on_line(InstallLine {
        line: format!("{} -> {}", link.display(), binary.display()),
        stderr: false,
        stage: false,
    });
    Ok(())
}
