//! The host's own timer, for a hub whose jobs the host wakes ([`crate::config::hub::SchedulerMode::Host`]).
//!
//! Once a minute it calls `konstruktor schedule tick --waker host --in <hub>`: systemd user
//! units on Linux, a launchd agent on macOS, a Task Scheduler task on Windows. What it
//! writes and runs is said by [`for_this_host`] before anything is done, and taken away by
//! [`uninstall`] when the hub chooses another way — a timer left behind would do nothing
//! anyway (`tick` runs only for the waker the profile names), but it is not left to rot.

use std::path::{Path, PathBuf};

/// What installing the timer writes and runs, and what taking it away runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTimer {
    /// What it is, for the operator: `systemd user timer konstruktor-schedule-…`.
    pub describe: String,
    /// Files to write, by absolute path.
    pub files: Vec<(PathBuf, String)>,
    /// Commands to run after writing them, in order.
    pub install: Vec<Vec<String>>,
    /// Commands to run to take it away, in order; the files are removed after.
    pub uninstall: Vec<Vec<String>>,
    /// Something the operator should know: a systemd user timer only runs while the user
    /// is logged in, unless lingering is on.
    pub note: Option<String>,
}

/// A name the host's scheduler can hold, unique to the hub: from its uid.
fn name_of(hub_uid: &str) -> String {
    let short: String = hub_uid
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(12)
        .collect();
    format!("konstruktor-schedule-{short}")
}

fn words(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_string()).collect()
}

/// systemd quotes an argument with spaces in double quotes, escaping `"` and `\`.
fn systemd_quote(argument: &str) -> String {
    format!(
        "\"{}\"",
        argument.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

/// The systemd user units: a oneshot service and the timer that starts it each minute.
pub fn systemd(home: &Path, hub_uid: &str, hub_dir: &Path, exe: &Path) -> HostTimer {
    let name = name_of(hub_uid);
    let units = home.join(".config/systemd/user");
    let dir = hub_dir.display().to_string();
    let service = format!(
        "[Unit]\nDescription=Konstruktor: the jobs of the hub at {dir}\n\n[Service]\nType=oneshot\nExecStart={} schedule tick --waker host --in {}\n",
        systemd_quote(&exe.display().to_string()),
        systemd_quote(&dir),
    );
    let timer = format!(
        "[Unit]\nDescription=Konstruktor: wake the jobs of the hub at {dir} each minute\n\n[Timer]\nOnCalendar=*-*-* *:*:00\nAccuracySec=5s\n\n[Install]\nWantedBy=timers.target\n"
    );
    HostTimer {
        describe: format!("systemd user timer {name}.timer"),
        files: vec![
            (units.join(format!("{name}.service")), service),
            (units.join(format!("{name}.timer")), timer),
        ],
        install: vec![
            words(&["systemctl", "--user", "daemon-reload"]),
            vec![
                "systemctl".into(),
                "--user".into(),
                "enable".into(),
                "--now".into(),
                format!("{name}.timer"),
            ],
        ],
        uninstall: vec![
            vec![
                "systemctl".into(),
                "--user".into(),
                "disable".into(),
                "--now".into(),
                format!("{name}.timer"),
            ],
            words(&["systemctl", "--user", "daemon-reload"]),
        ],
        note: Some(
            "A user timer runs while you are logged in. For a server, keep it running after you log out with `loginctl enable-linger`.".into(),
        ),
    }
}

/// XML text, escaped.
fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The launchd agent: runs the tick every 60 seconds while the user is logged in.
pub fn launchd(home: &Path, hub_uid: &str, hub_dir: &Path, exe: &Path) -> HostTimer {
    let name = name_of(hub_uid);
    let label = format!("live.arkitekt.{name}");
    let plist_path = home.join(format!("Library/LaunchAgents/{label}.plist"));
    let arguments = [
        exe.display().to_string(),
        "schedule".into(),
        "tick".into(),
        "--waker".into(),
        "host".into(),
        "--in".into(),
        hub_dir.display().to_string(),
    ]
    .iter()
    .map(|argument| format!("    <string>{}</string>\n", xml(argument)))
    .collect::<String>();
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>{}</string>\n  <key>ProgramArguments</key>\n  <array>\n{arguments}  </array>\n  <key>StartInterval</key>\n  <integer>60</integer>\n  <key>RunAtLoad</key>\n  <true/>\n</dict>\n</plist>\n",
        xml(&label)
    );
    let path = plist_path.display().to_string();
    HostTimer {
        describe: format!("launchd agent {label}"),
        files: vec![(plist_path, plist)],
        install: vec![vec![
            "launchctl".into(),
            "load".into(),
            "-w".into(),
            path.clone(),
        ]],
        uninstall: vec![vec!["launchctl".into(), "unload".into(), "-w".into(), path]],
        note: None,
    }
}

/// The Task Scheduler task: every minute, as the user.
pub fn windows(hub_uid: &str, hub_dir: &Path, exe: &Path) -> HostTimer {
    let task = format!("Konstruktor\\{}", name_of(hub_uid));
    let action = format!(
        "\"{}\" schedule tick --waker host --in \"{}\"",
        exe.display(),
        hub_dir.display()
    );
    HostTimer {
        describe: format!("Task Scheduler task {task}"),
        files: Vec::new(),
        install: vec![vec![
            "schtasks".into(),
            "/Create".into(),
            "/F".into(),
            "/SC".into(),
            "MINUTE".into(),
            "/MO".into(),
            "1".into(),
            "/TN".into(),
            task.clone(),
            "/TR".into(),
            action,
        ]],
        uninstall: vec![vec![
            "schtasks".into(),
            "/Delete".into(),
            "/F".into(),
            "/TN".into(),
            task,
        ]],
        note: Some("The task runs while you are logged in.".into()),
    }
}

/// The timer this host would get, or why it gets none.
pub fn for_this_host(hub_uid: &str, hub_dir: &Path, exe: &Path) -> Result<HostTimer, String> {
    let home = dirs::home_dir().ok_or("this user has no home folder to keep a timer in")?;
    if cfg!(target_os = "linux") {
        Ok(systemd(&home, hub_uid, hub_dir, exe))
    } else if cfg!(target_os = "macos") {
        Ok(launchd(&home, hub_uid, hub_dir, exe))
    } else if cfg!(target_os = "windows") {
        Ok(windows(hub_uid, hub_dir, exe))
    } else {
        Err("this host has no scheduler Konstruktor knows: choose the container instead".into())
    }
}

fn run(command: &[String]) -> Result<(), String> {
    let output = std::process::Command::new(&command[0])
        .args(&command[1..])
        .output()
        .map_err(|error| format!("`{}` could not be run: {error}", command.join(" ")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "`{}` failed: {}",
            command.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// Write the timer's files and start it.
pub fn install(timer: &HostTimer) -> Result<(), String> {
    for (path, contents) in &timer.files {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(path, contents).map_err(|error| error.to_string())?;
    }
    timer.install.iter().try_for_each(|command| run(command))
}

/// Stop the timer and remove its files. One that is not there is already gone.
pub fn uninstall(timer: &HostTimer) -> Result<(), String> {
    let present = timer.files.is_empty() || timer.files.iter().any(|(path, _)| path.exists());
    if present {
        // Stopping one that was never started fails; removing its files is what matters.
        for command in &timer.uninstall {
            let _ = run(command);
        }
    }
    for (path, _) in &timer.files {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_systemd_timer_ticks_the_hub_each_minute() {
        let timer = systemd(
            Path::new("/home/ada"),
            "3f2a-9b1c-77e0-aa11",
            Path::new("/srv/my hub"),
            Path::new("/usr/local/bin/konstruktor"),
        );
        let (service_path, service) = &timer.files[0];
        let (timer_path, unit) = &timer.files[1];
        assert_eq!(
            service_path,
            Path::new("/home/ada/.config/systemd/user/konstruktor-schedule-3f2a9b1c77e0.service")
        );
        assert!(timer_path.ends_with("konstruktor-schedule-3f2a9b1c77e0.timer"));
        assert!(service.contains(
            "ExecStart=\"/usr/local/bin/konstruktor\" schedule tick --waker host --in \"/srv/my hub\""
        ));
        assert!(unit.contains("OnCalendar=*-*-* *:*:00"));
        assert_eq!(
            timer.install.last().unwrap(),
            &words(&[
                "systemctl",
                "--user",
                "enable",
                "--now",
                "konstruktor-schedule-3f2a9b1c77e0.timer"
            ])
        );
    }

    #[test]
    fn a_launchd_agent_runs_the_tick_every_sixty_seconds() {
        let timer = launchd(
            Path::new("/Users/ada"),
            "3f2a9b1c77e0",
            Path::new("/Users/ada/hubs/a&b"),
            Path::new("/opt/homebrew/bin/konstruktor"),
        );
        let (path, plist) = &timer.files[0];
        assert_eq!(
            path,
            Path::new("/Users/ada/Library/LaunchAgents/live.arkitekt.konstruktor-schedule-3f2a9b1c77e0.plist")
        );
        assert!(plist.contains("<integer>60</integer>"));
        assert!(plist.contains("<string>/Users/ada/hubs/a&amp;b</string>"));
        assert!(plist.contains("<string>--waker</string>\n    <string>host</string>"));
    }

    #[test]
    fn a_windows_task_runs_every_minute_as_the_user() {
        let timer = windows(
            "3f2a9b1c77e0",
            Path::new("C:\\hubs\\lab"),
            Path::new("C:\\Program Files\\Konstruktor\\konstruktor.exe"),
        );
        let create = &timer.install[0];
        assert_eq!(
            &create[..7],
            &words(&["schtasks", "/Create", "/F", "/SC", "MINUTE", "/MO", "1"])
        );
        assert_eq!(create[8], "Konstruktor\\konstruktor-schedule-3f2a9b1c77e0");
        assert_eq!(
            create[10],
            "\"C:\\Program Files\\Konstruktor\\konstruktor.exe\" schedule tick --waker host --in \"C:\\hubs\\lab\""
        );
    }
}
