use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Everything Konstruktor needs to know about git on this machine.
///
/// Git is optional in a way Docker is not: a hub runs from published images and never
/// needs it. It only becomes load-bearing for a *dev hub*, where the services' source is
/// checked out on this machine and mounted into the containers — so this probe reports,
/// and nothing here ever blocks a plain deployment.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct GitProbe {
    /// The `git` binary is on PATH.
    pub cli: bool,
    /// `git --version`, e.g. "2.43.0".
    pub cli_version: Option<String>,
}

impl GitProbe {
    pub fn is_ready(&self) -> bool {
        self.cli
    }
}

/// Looks for git. Nothing here panics — "git is missing" is an ordinary answer.
pub fn probe() -> GitProbe {
    let mut probe = GitProbe::default();

    if let Some(line) = Command::new("git")
        .arg("--version")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|line| !line.is_empty())
    {
        probe.cli = true;
        // The same banner shape as Docker's — `git version 2.43.0` — so the same parse.
        probe.cli_version = crate::docker::parse_cli_version(&line);
    }

    probe
}

#[derive(Debug, thiserror::Error)]
pub enum CloneError {
    #[error("git is not installed, so the source for {service} cannot be checked out")]
    NoGit { service: String },
    #[error("could not run git for {service}: {source}")]
    Spawn {
        service: String,
        #[source]
        source: std::io::Error,
    },
    #[error("git could not clone {repo}{branch}: {message}", branch = .branch
        .as_deref()
        .map(|b| format!(" at branch `{b}`"))
        .unwrap_or_default())]
    Failed {
        repo: String,
        branch: Option<String>,
        message: String,
    },
}

/// Checks `repo` out into `into`, at `branch` when one is named.
///
/// An ordinary full clone, not a shallow one. The point of a dev hub is that somebody
/// works in these checkouts — switches branches, commits, pushes — and a
/// `--depth 1 --single-branch` clone is exactly the shape that cannot do any of that
/// without being repaired first. The download is the price of the option.
///
/// An existing non-empty destination is left exactly as it is. Re-creating a dev hub over
/// a folder somebody has been working in must never throw their work away, and a checkout
/// is the one part of a deployment Konstruktor does not own.
pub fn clone_service(
    service: &str,
    repo: &str,
    branch: Option<&str>,
    into: &Path,
) -> Result<bool, CloneError> {
    if into.read_dir().map(|mut d| d.next().is_some()).unwrap_or(false) {
        return Ok(false);
    }

    if !probe().is_ready() {
        return Err(CloneError::NoGit {
            service: service.to_string(),
        });
    }

    let mut command = Command::new("git");
    command.arg("clone");
    if let Some(branch) = branch {
        command.args(["--branch", branch]);
    }
    command.arg(repo).arg(into);

    let output = command.output().map_err(|source| CloneError::Spawn {
        service: service.to_string(),
        source,
    })?;

    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(CloneError::Failed {
            repo: repo.to_string(),
            branch: branch.map(str::to_string),
            message: if message.is_empty() {
                "git gave no reason".into()
            } else {
                message
            },
        });
    }

    Ok(true)
}

/// One service's checkout inside a dev hub's `mounts/` folder.
///
/// Every field is answered independently and a failure is a field rather than an error:
/// a checkout somebody deleted, or replaced with a plain folder, must show up in the
/// dashboard saying so — not take the page down with it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Checkout {
    /// The compose service this belongs to, e.g. `rekuest`.
    pub service: String,
    /// Absolute path of the checkout.
    pub path: String,
    /// The repository it was cloned from, as the profile names it.
    pub repo: String,
    /// The branch HEAD is on. `None` on a detached HEAD, or when there is no repository.
    pub branch: Option<String>,
    /// The short commit HEAD points at.
    pub head: Option<String>,
    /// HEAD is not on a branch — switching is still fine, but nothing tracks.
    pub detached: bool,
    /// Tracked files differ from HEAD. Untracked files deliberately do not count: a dev
    /// hub's containers write `__pycache__` and friends into the mount, and treating
    /// those as work-in-progress would leave the switch permanently refused.
    pub dirty: bool,
    /// Why this checkout could not be read, when it could not.
    pub error: Option<String>,
}

/// Runs a git subcommand inside `at`, returning its stdout on success and its stderr on
/// failure — git explains itself far better than any wrapper could, and on a dev hub the
/// explanation is usually "a container wrote this file as root", which only git can say.
fn git_in(at: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(at)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            stderr
        })
    }
}

/// Reads one checkout. Never fails: what went wrong lands in `error`.
pub fn read_checkout(service: &str, repo: &str, path: &Path) -> Checkout {
    let mut checkout = Checkout {
        service: service.to_string(),
        path: path.to_string_lossy().to_string(),
        repo: repo.to_string(),
        branch: None,
        head: None,
        detached: false,
        dirty: false,
        error: None,
    };

    if !path.exists() {
        checkout.error = Some("no checkout in this deployment".into());
        return checkout;
    }
    if !probe().is_ready() {
        checkout.error = Some("git is not installed".into());
        return checkout;
    }

    // `--git-dir` rather than `--is-inside-work-tree`: the latter answers yes for a
    // folder *under* a repository, which a deployment inside one would be.
    if git_in(path, &["rev-parse", "--git-dir"]).is_err() {
        checkout.error = Some("this folder is not a git repository".into());
        return checkout;
    }

    match git_in(path, &["symbolic-ref", "--short", "HEAD"]) {
        Ok(branch) => checkout.branch = Some(branch),
        // Not an error: a detached HEAD is a state, not a failure.
        Err(_) => checkout.detached = true,
    }
    checkout.head = git_in(path, &["rev-parse", "--short", "HEAD"]).ok();

    // Untracked files are excluded on purpose — see `Checkout::dirty`.
    match git_in(path, &["status", "--porcelain", "--untracked-files=no"]) {
        Ok(status) => checkout.dirty = !status.is_empty(),
        Err(message) => checkout.error = Some(message),
    }

    checkout
}

/// The branches this checkout could switch to: everything local, plus everything on
/// `origin` that has no local counterpart yet.
///
/// `fetch --prune` first, so a branch pushed a minute ago is offered and one deleted
/// upstream stops being.
pub fn branches(path: &Path) -> Result<Vec<String>, String> {
    if !probe().is_ready() {
        return Err("git is not installed".into());
    }
    // A fetch failure is not fatal — offline, the local answer is still worth having.
    let _ = git_in(path, &["fetch", "--prune", "--quiet"]);

    let mut names: Vec<String> = git_in(
        path,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )?
    .lines()
    .map(str::to_string)
    .collect();

    for remote in git_in(
        path,
        &["for-each-ref", "--format=%(refname:short)", "refs/remotes/origin"],
    )
    .unwrap_or_default()
    .lines()
    {
        // `origin/HEAD` is a symbolic alias for the default branch, not a branch.
        let Some(name) = remote.strip_prefix("origin/") else {
            continue;
        };
        if name != "HEAD" && !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }

    names.sort();
    Ok(names)
}

#[derive(Debug, thiserror::Error)]
pub enum SwitchError {
    #[error("git is not installed")]
    NoGit,
    #[error("{service} has uncommitted changes. Commit or stash them first — switching \
             branches over them would lose work.")]
    Dirty { service: String },
    #[error("no branch called `{branch}`, here or on origin")]
    NoSuchBranch { branch: String },
    #[error("{0}")]
    Git(String),
}

/// Puts one checkout on `branch`.
///
/// Refuses over uncommitted work rather than forcing: the whole point of a dev hub is
/// that the checkout holds work somebody is doing, and `--force` would throw it away.
///
/// A branch that exists only on the remote is created to track it. `git checkout <name>`
/// alone relies on DWIM, which is off whenever more than one remote matches or
/// `checkout.guess` is unset — so it is spelled out instead of hoped for.
pub fn switch_branch(service: &str, path: &Path, branch: &str) -> Result<(), SwitchError> {
    if !probe().is_ready() {
        return Err(SwitchError::NoGit);
    }

    let state = read_checkout(service, "", path);
    if let Some(error) = state.error {
        return Err(SwitchError::Git(error));
    }
    if state.dirty {
        return Err(SwitchError::Dirty {
            service: service.to_string(),
        });
    }

    let _ = git_in(path, &["fetch", "--prune", "--quiet"]);

    let local = git_in(path, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])
        .is_ok();
    let remote = git_in(
        path,
        &["rev-parse", "--verify", "--quiet", &format!("refs/remotes/origin/{branch}")],
    )
    .is_ok();

    if local {
        git_in(path, &["checkout", branch]).map_err(SwitchError::Git)?;
    } else if remote {
        git_in(
            path,
            &["checkout", "-b", branch, "--track", &format!("origin/{branch}")],
        )
        .map_err(SwitchError::Git)?;
    } else {
        return Err(SwitchError::NoSuchBranch {
            branch: branch.to_string(),
        });
    }

    Ok(())
}

/// Where a dev hub keeps one service's checkout, inside the deployment folder.
///
/// The single definition both the creator and the dashboard use, so the folder that is
/// cloned into is by construction the folder compose bind-mounts and the one a branch is
/// switched in.
pub fn checkout_dir(dir: &Path, service_host: &str) -> std::path::PathBuf {
    dir.join(crate::generate::compose::MOUNTS_DIR).join(service_host)
}

/// Every checkout this deployment has, in the profile's service order.
///
/// Empty means "not a dev hub" — nothing else has to be asked, and no front end needs a
/// separate flag to decide whether to offer branch switching.
pub fn checkouts(dir: &Path, config: &crate::config::hub::HubConfig) -> Vec<Checkout> {
    config
        .enabled_services()
        .into_iter()
        .map(|id| config.service(id))
        .filter(|service| service.mount_github)
        .map(|service| {
            read_checkout(
                &service.host,
                &service.github_repo,
                &checkout_dir(dir, &service.host),
            )
        })
        .collect()
}
