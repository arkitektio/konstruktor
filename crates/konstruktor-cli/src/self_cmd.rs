//! `konstruktor self install`: put the folder this binary is in on `PATH`, for good.
//!
//! The installer drops the binary in `~/.local/bin`, which plenty of machines do not have
//! on `PATH` — and "add this line to your shell's startup file" is the step people skip,
//! or do for the wrong shell. So the binary does it: every shell this machine is set up
//! for gets one marked block, and running it again changes nothing.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};

use crate::ui;

#[derive(Subcommand)]
pub enum SelfCommand {
    /// Put konstruktor on your PATH, in every shell this machine is set up for.
    Install(InstallArgs),
}

#[derive(Args)]
pub struct InstallArgs {
    /// The folder to put on PATH. Defaults to the one this binary is in.
    #[arg(long)]
    pub dir: Option<PathBuf>,
}

pub fn run(command: SelfCommand) -> Result<()> {
    match command {
        SelfCommand::Install(args) => install(args),
    }
}

/// What fences the block, so a second run finds the first one's instead of adding another.
const BEGIN: &str = "# >>> konstruktor >>>";
const END: &str = "# <<< konstruktor <<<";

fn install(args: InstallArgs) -> Result<()> {
    let dir = match args.dir {
        Some(dir) => std::path::absolute(&dir).with_context(|| format!("{}", dir.display()))?,
        None => std::env::current_exe()
            .context("finding this binary")?
            .parent()
            .context("this binary has no folder")?
            .to_path_buf(),
    };

    if cfg!(windows) {
        return install_windows(&dir);
    }

    let home = dirs::home_dir().context("no home directory to find the shells' files in")?;
    let machine = Machine::detect(home.clone());

    let mut changed = false;
    for target in machine.targets() {
        let block = target.syntax.block(&dir, &home)?;
        let shown = tilde(&target.path, &home);
        match apply(&target.path, &block)? {
            Change::Added => ui::ok(&shown),
            Change::Updated => ui::ok(&format!("{shown} {}", ui::dim("(updated)"))),
            Change::Unchanged => {
                ui::step(&format!(
                    "{} {shown} {}",
                    ui::dim("·"),
                    ui::dim("(already there)")
                ));
                continue;
            }
        }
        changed = true;
    }

    let on_path = std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|entry| entry == dir));
    if changed {
        ui::say("");
        ui::step("New terminals have konstruktor on their PATH.");
    }
    if !on_path {
        ui::step(&format!(
            "For this one: {}",
            ui::bold(&machine.session_line(&dir, &home)?)
        ));
    }
    Ok(())
}

/// The same edit `install.ps1` makes: the user's `Path`, which Windows keeps in the
/// registry rather than in a file a shell reads.
fn install_windows(dir: &Path) -> Result<()> {
    const ALREADY: i32 = 3;
    // The folder travels through the environment, and the script has no double quotes in
    // it, so nothing has to survive being quoted on the way to powershell.
    let script = "$d = $env:KONSTRUKTOR_SELF_DIR; \
        $p = [Environment]::GetEnvironmentVariable('Path', 'User'); \
        if (($p -split ';') -contains $d) { exit 3 }; \
        $n = if ($p) { $p + ';' + $d } else { $d }; \
        [Environment]::SetEnvironmentVariable('Path', $n, 'User')";
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("KONSTRUKTOR_SELF_DIR", dir)
        .status()
        .context("running powershell")?;

    match status.code() {
        Some(0) => ui::ok(&format!(
            "Added {} to your PATH (new terminals pick it up).",
            dir.display()
        )),
        Some(ALREADY) => ui::step(&format!("{} is already on your PATH.", dir.display())),
        _ => bail!("could not change the user PATH ({status})"),
    }
    Ok(())
}

/// Everything about this machine that decides which files get the block.
struct Machine {
    home: PathBuf,
    /// The login shell's name — `zsh`, `bash`, `fish` — or empty when `$SHELL` is unset.
    shell: String,
    /// Where zsh keeps `.zshrc`: `$ZDOTDIR`, or home.
    zsh_dir: PathBuf,
    /// fish's configuration folder: `$XDG_CONFIG_HOME/fish`, or `~/.config/fish`.
    fish_dir: PathBuf,
    macos: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Syntax {
    Posix,
    Fish,
}

struct Target {
    path: PathBuf,
    syntax: Syntax,
}

impl Machine {
    fn detect(home: PathBuf) -> Self {
        let set = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        let shell = set("SHELL")
            .and_then(|shell| Some(Path::new(&shell).file_name()?.to_str()?.to_string()))
            .unwrap_or_default();
        Machine {
            shell,
            zsh_dir: set("ZDOTDIR").map_or_else(|| home.clone(), PathBuf::from),
            fish_dir: set("XDG_CONFIG_HOME")
                .map_or_else(|| home.join(".config"), PathBuf::from)
                .join("fish"),
            macos: cfg!(target_os = "macos"),
            home,
        }
    }

    /// A file that is already there belongs to a shell somebody uses, so it gets the
    /// block. One that is missing is created only for the login shell — nobody wants a
    /// `.zshrc` appearing on a machine that has never run zsh.
    fn targets(&self) -> Vec<Target> {
        let shell = self.shell.as_str();
        let mut targets = Vec::new();
        let mut want = |path: PathBuf, wanted: bool, syntax| {
            if wanted {
                targets.push(Target { path, syntax });
            }
        };

        let zshrc = self.zsh_dir.join(".zshrc");
        want(
            zshrc.clone(),
            zshrc.exists() || shell == "zsh",
            Syntax::Posix,
        );

        let bashrc = self.home.join(".bashrc");
        want(
            bashrc.clone(),
            bashrc.exists() || shell == "bash",
            Syntax::Posix,
        );

        // A login bash reads the first of `.bash_profile`, `.bash_login` and `.profile`
        // that exists, and none of the others. Terminals on macOS open login shells, which
        // skip `.bashrc` too, so there bash needs one of the three — but making a
        // `.bash_profile` next to an existing `.profile` would switch that one off.
        let bash_profile = self.home.join(".bash_profile");
        let bash_login = self.home.join(".bash_login");
        let profile = self.home.join(".profile");
        let has_login_file = bash_profile.exists() || bash_login.exists() || profile.exists();
        want(
            bash_profile.clone(),
            bash_profile.exists() || (shell == "bash" && self.macos && !has_login_file),
            Syntax::Posix,
        );
        want(bash_login.clone(), bash_login.exists(), Syntax::Posix);

        // What every other sh reads at login, and so the one to make for a shell that is
        // none of the above.
        want(
            profile.clone(),
            profile.exists() || !matches!(shell, "zsh" | "bash" | "fish"),
            Syntax::Posix,
        );

        // fish reads every file in `conf.d`, so it gets one of its own, not an edit.
        want(
            self.fish_dir.join("conf.d").join("konstruktor.fish"),
            self.fish_dir.exists() || shell == "fish",
            Syntax::Fish,
        );

        targets
    }

    /// What to paste to get it in the terminal that is already open.
    fn session_line(&self, dir: &Path, home: &Path) -> Result<String> {
        let syntax = if self.shell == "fish" {
            Syntax::Fish
        } else {
            Syntax::Posix
        };
        let dir = syntax.escaped(dir, home)?;
        Ok(match syntax {
            Syntax::Posix => format!("export PATH=\"{dir}:$PATH\""),
            Syntax::Fish => format!("set -gx PATH \"{dir}\" $PATH"),
        })
    }
}

impl Syntax {
    /// The folder, escaped to sit inside double quotes. Under home it is spelled
    /// `$HOME/…`, so the line still holds when the dotfiles are carried to a machine with
    /// another user name.
    fn escaped(self, dir: &Path, home: &Path) -> Result<String> {
        let special: &[char] = match self {
            Syntax::Posix => &['\\', '"', '$', '`'],
            Syntax::Fish => &['\\', '"', '$'],
        };
        let escape = |path: &Path| -> Result<String> {
            let text = path
                .to_str()
                .with_context(|| format!("{} is not valid UTF-8", path.display()))?;
            if text.contains('\n') {
                bail!("{} has a line break in it", path.display());
            }
            let mut escaped = String::with_capacity(text.len());
            for c in text.chars() {
                if special.contains(&c) {
                    escaped.push('\\');
                }
                escaped.push(c);
            }
            Ok(escaped)
        };

        Ok(match dir.strip_prefix(home) {
            Ok(rest) if rest.as_os_str().is_empty() => "$HOME".to_string(),
            Ok(rest) => format!("$HOME/{}", escape(rest)?),
            Err(_) => escape(dir)?,
        })
    }

    /// The fenced block: put the folder first on `PATH`, unless it is on it already —
    /// startup files are read again by every nested shell.
    fn block(self, dir: &Path, home: &Path) -> Result<String> {
        let dir = self.escaped(dir, home)?;
        let body = match self {
            Syntax::Posix => format!(
                "case \":$PATH:\" in\n    *\":{dir}:\"*) ;;\n    *) export PATH=\"{dir}:$PATH\" ;;\nesac"
            ),
            Syntax::Fish => {
                format!("if not contains -- \"{dir}\" $PATH\n    set -gx PATH \"{dir}\" $PATH\nend")
            }
        };
        Ok(format!("{BEGIN}\n{body}\n{END}"))
    }
}

#[derive(PartialEq, Debug)]
enum Change {
    Added,
    Updated,
    Unchanged,
}

/// Writes the block into one file: appended the first time, replaced in place when the
/// folder has moved since, left alone when it is already what would be written.
fn apply(path: &Path, block: &str) -> Result<Change> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };

    let fenced = existing.find(BEGIN).and_then(|start| {
        let end = start + existing[start..].find(END)? + END.len();
        Some(start..end)
    });
    let (next, change) = match fenced {
        Some(range) if &existing[range.clone()] == block => return Ok(Change::Unchanged),
        Some(range) => (
            format!(
                "{}{block}{}",
                &existing[..range.start],
                &existing[range.end..]
            ),
            Change::Updated,
        ),
        None => {
            let gap = match existing.as_str() {
                "" => "",
                text if text.ends_with('\n') => "\n",
                _ => "\n\n",
            };
            (format!("{existing}{gap}{block}\n"), Change::Added)
        }
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(path, next).with_context(|| format!("writing {}", path.display()))?;
    Ok(change)
}

fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A home directory of its own per test, under the system's temporary folder.
    fn home(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("konstruktor-self-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn machine(home: &Path, shell: &str, macos: bool) -> Machine {
        Machine {
            home: home.to_path_buf(),
            shell: shell.to_string(),
            zsh_dir: home.to_path_buf(),
            fish_dir: home.join(".config").join("fish"),
            macos,
        }
    }

    /// With `/` whatever the platform: the files are a Unix shell's wherever this runs,
    /// and Windows joins a path with `\`, which is not what these tests are about.
    fn names(machine: &Machine) -> Vec<String> {
        machine
            .targets()
            .iter()
            .map(|target| tilde(&target.path, &machine.home).replace('\\', "/"))
            .collect()
    }

    #[test]
    fn only_the_login_shell_gets_a_file_made_for_it() {
        let home = home("login-shell");
        assert_eq!(names(&machine(&home, "zsh", false)), ["~/.zshrc"]);
        assert_eq!(names(&machine(&home, "bash", false)), ["~/.bashrc"]);
        assert_eq!(
            names(&machine(&home, "bash", true)),
            ["~/.bashrc", "~/.bash_profile"]
        );
        assert_eq!(
            names(&machine(&home, "fish", false)),
            ["~/.config/fish/conf.d/konstruktor.fish"]
        );
        assert_eq!(names(&machine(&home, "dash", false)), ["~/.profile"]);
        assert_eq!(names(&machine(&home, "", false)), ["~/.profile"]);
    }

    #[test]
    fn every_shell_that_is_set_up_is_covered() {
        let home = home("set-up");
        std::fs::write(home.join(".bashrc"), "").unwrap();
        std::fs::write(home.join(".profile"), "").unwrap();
        std::fs::create_dir_all(home.join(".config/fish")).unwrap();
        assert_eq!(
            names(&machine(&home, "zsh", false)),
            [
                "~/.zshrc",
                "~/.bashrc",
                "~/.profile",
                "~/.config/fish/conf.d/konstruktor.fish"
            ]
        );
    }

    #[test]
    fn a_login_file_bash_already_reads_is_not_shadowed_by_a_new_one() {
        let home = home("login-file");
        std::fs::write(home.join(".profile"), "").unwrap();
        assert_eq!(
            names(&machine(&home, "bash", true)),
            ["~/.bashrc", "~/.profile"]
        );

        std::fs::write(home.join(".bash_login"), "").unwrap();
        assert_eq!(
            names(&machine(&home, "bash", true)),
            ["~/.bashrc", "~/.bash_login", "~/.profile"]
        );
    }

    #[test]
    fn a_second_run_changes_nothing_and_a_moved_folder_is_replaced_in_place() {
        let home = home("idempotent");
        let rc = home.join(".zshrc");
        std::fs::write(&rc, "alias ll='ls -l'").unwrap();

        let first = Syntax::Posix
            .block(&home.join(".local/bin"), &home)
            .unwrap();
        assert_eq!(apply(&rc, &first).unwrap(), Change::Added);
        let written = std::fs::read_to_string(&rc).unwrap();
        assert_eq!(written, format!("alias ll='ls -l'\n\n{first}\n"));
        assert_eq!(apply(&rc, &first).unwrap(), Change::Unchanged);

        std::fs::write(&rc, format!("{written}export EDITOR=vi\n")).unwrap();
        let moved = Syntax::Posix.block(Path::new("/opt/k/bin"), &home).unwrap();
        assert_eq!(apply(&rc, &moved).unwrap(), Change::Updated);
        assert_eq!(
            std::fs::read_to_string(&rc).unwrap(),
            format!("alias ll='ls -l'\n\n{moved}\nexport EDITOR=vi\n")
        );
    }

    #[test]
    fn the_folder_is_quoted_for_the_shell_that_reads_it() {
        let home = Path::new("/home/me");
        assert_eq!(
            Syntax::Posix
                .escaped(Path::new("/home/me/.local/bin"), home)
                .unwrap(),
            "$HOME/.local/bin"
        );
        assert_eq!(
            Syntax::Posix
                .escaped(Path::new("/opt/my $tools/`bin`"), home)
                .unwrap(),
            "/opt/my \\$tools/\\`bin\\`"
        );
        assert_eq!(
            Syntax::Fish
                .escaped(Path::new("/opt/my $tools/`bin`"), home)
                .unwrap(),
            "/opt/my \\$tools/`bin`"
        );
    }
}
