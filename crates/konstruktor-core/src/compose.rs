/// Host-side `docker compose` invocations. All of them run with the working directory set
/// to the deployment folder, so the project name is derived the way the Python CLI derives
/// it — deliberately not overridden with `-p`, so `arkitekt-next hub up` in that same
/// folder lands on the same stack.

pub fn up() -> Vec<&'static str> {
    vec!["compose", "up", "-d"]
}

/// What `up` should run in a deployment folder, and why, when it is not plain [`up`].
///
/// The health reporter is the one container a hub can run without. Its image lives on a
/// registry this machine may not reach, or — on a development build — may not be
/// published yet, and compose treats a failed pull as fatal for the whole project: a hub
/// would not start at all for want of its health reports.
///
/// So when the reporter's image is neither on this machine nor pullable, every *other*
/// service is started by name, and the second element says what was left out. Anything
/// that is not the reporter still fails `up` exactly as before.
pub async fn up_in(dir: &std::path::Path) -> (Vec<String>, Option<String>) {
    let plain = || up().into_iter().map(String::from).collect::<Vec<_>>();

    let Some(reporter) = crate::profile::read_profile(dir)
        .ok()
        .and_then(|p| p.config.reporter)
        .filter(|r| r.enabled)
    else {
        return (plain(), None);
    };

    let docker_ok = |args: &'static [&'static str], image: String| async move {
        crate::engine_probe::engine()
            .async_command()
            .args(args)
            .arg(&image)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .is_ok_and(|s| s.success())
    };
    if docker_ok(&["image", "inspect"], reporter.image.clone()).await
        || docker_ok(&["pull"], reporter.image.clone()).await
    {
        return (plain(), None);
    }

    // The services the file on disk declares — not a regeneration, which could disagree
    // with a compose file somebody edited.
    let declared: Vec<String> = std::fs::read_to_string(dir.join("docker-compose.yaml"))
        .ok()
        .and_then(|text| serde_norway::from_str::<serde_norway::Value>(&text).ok())
        .and_then(|doc| {
            doc.get("services")?.as_mapping().map(|services| {
                services
                    .keys()
                    .filter_map(|k| k.as_str().map(str::to_string))
                    .collect()
            })
        })
        .unwrap_or_default();
    let others: Vec<String> = declared.into_iter().filter(|s| *s != reporter.host).collect();
    if others.is_empty() {
        return (plain(), None);
    }

    let mut args = plain();
    args.extend(others);
    (
        args,
        Some(format!(
            "Starting without the health reporter: its image {} is not on this machine and \
             could not be pulled. The hub runs, but its coordination server will show it \
             as offline until the image is available.",
            reporter.image
        )),
    )
}

/// One line a compose invocation wrote, and on which stream.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeLine {
    pub line: String,
    pub stderr: bool,
}

/// Runs one compose invocation in a deployment folder, handing every line of both
/// streams to `on_line` as it is written, and returns what it put on stdout.
///
/// `args` starts with `compose`. Plain, line-by-line narration is asked for: without a
/// TTY compose already avoids its redrawing progress UI, `--ansi never` keeps colour codes
/// out, and `--progress plain` adds each layer's download and extract steps — what a pull
/// of several gigabytes is otherwise silent about for minutes. A failure carries both
/// streams, since compose explains itself on stderr.
pub async fn run_streamed(
    dir: &std::path::Path,
    mut args: Vec<String>,
    on_line: &(dyn Fn(ComposeLine) + Send + Sync),
) -> Result<String, String> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    args.splice(1..1, ["--ansi", "never", "--progress", "plain"].map(String::from));

    let mut child = crate::engine_probe::engine()
        .async_command()
        .args(&args)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| e.to_string())?;

    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let clean = |raw: &str| String::from_utf8_lossy(&strip_ansi_escapes::strip(raw)).into_owned();

    // Both streams in this one task: a callback borrowed for the call cannot be handed to
    // a spawned one, and neither needs to be.
    let read_out = async {
        let mut collected = String::new();
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(raw)) = lines.next_line().await {
            let line = clean(&raw);
            collected.push_str(&line);
            collected.push('\n');
            on_line(ComposeLine { line, stderr: false });
        }
        collected
    };
    let read_err = async {
        let mut collected = String::new();
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(raw)) = lines.next_line().await {
            let line = clean(&raw);
            if line.trim().is_empty() {
                continue;
            }
            collected.push_str(&line);
            collected.push('\n');
            on_line(ComposeLine { line, stderr: true });
        }
        collected
    };
    let (out, err) = tokio::join!(read_out, read_err);
    let status = child.wait().await.map_err(|e| e.to_string())?;

    if status.success() {
        Ok(out)
    } else {
        Err(format!("{out}{err}"))
    }
}

pub fn stop() -> Vec<&'static str> {
    vec!["compose", "stop"]
}
/// `stop`, but with an explicit per-container grace period.
///
/// Compose's default is 10 seconds *per container*, which is far too long for a teardown
/// that runs while the app is quitting — the services declare `stop_grace_period: 2s`
/// anyway, so nothing loses time it was actually using.
pub fn stop_timeout(seconds: u32) -> Vec<String> {
    vec![
        "compose".into(),
        "stop".into(),
        "-t".into(),
        seconds.to_string(),
    ]
}
pub fn pull() -> Vec<&'static str> {
    vec!["compose", "pull"]
}
/// Fetch one service's image, leaving everything that is running alone.
pub fn pull_service(service: &str) -> Vec<String> {
    vec!["compose".into(), "pull".into(), service.into()]
}
/// Recreate one service against the image its tag points at *now*, and nothing else.
///
/// `--no-deps` is the load-bearing flag. Every generated service declares
/// `depends_on: [redis, db, minio]`, so without it updating one service on a stopped
/// stack would quietly boot the infrastructure and leave the hub half up — the one state
/// the dashboard draws as a fault.
pub fn up_service(service: &str) -> Vec<String> {
    vec![
        "compose".into(),
        "up".into(),
        "-d".into(),
        "--no-deps".into(),
        service.into(),
    ]
}
/// Removes containers and networks; volumes (the database!) survive.
pub fn down() -> Vec<&'static str> {
    vec!["compose", "down"]
}
/// Removes named volumes as well as the containers.
///
/// With the default profile the database and object storage *are* named volumes, so this
/// deletes the data. A profile that opted into bind mounts in the deployment folder
/// declares no data volumes, and for it this takes nothing with it. Either way, deleting
/// a hub's data on purpose is `destroy::purge_data`'s job, which knows which case it is in.
pub fn down_volumes() -> Vec<&'static str> {
    vec!["compose", "down", "--volumes"]
}
/// Everything this project ever put on the machine: containers, networks, volumes, and
/// anything an earlier shape of the compose file left behind. Only for deleting a hub
/// outright — `--remove-orphans` is too eager for a routine `down`, since a service the
/// user has temporarily commented out counts as an orphan.
pub fn down_everything() -> Vec<&'static str> {
    vec!["compose", "down", "--volumes", "--remove-orphans"]
}
/// Creates a Django superuser in one running service, answering with what Django printed.
///
/// A failure carries both streams: Django says why on stderr — "that username is already
/// taken", most often — which is what a person needs to read, not an exit code.
pub async fn run_superuser(
    dir: &std::path::Path,
    service: &str,
    username: &str,
    password: &str,
    email: Option<&str>,
) -> Result<String, String> {
    let username = username.trim();
    if username.is_empty() || password.is_empty() {
        return Err("a username and a password are both required".into());
    }
    let email = email.map(str::trim).filter(|e| !e.is_empty());
    let output = crate::engine_probe::engine()
        .async_command()
        .args(create_superuser(service, username, password, email))
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if output.status.success() {
        Ok(stdout)
    } else {
        Err(format!("{stdout}{}", String::from_utf8_lossy(&output.stderr))
            .trim()
            .to_string())
    }
}

/// Create a Django superuser inside one running service.
///
/// Per service on purpose: each service keeps its own database and its own admin site,
/// so "an account for the hub" is really one account per service, made in the container
/// that owns the table. The credentials go in as environment variables rather than on
/// the command line — `--noinput` is what reads them, and a password in `argv` is
/// visible to every process on the machine for as long as the command runs.
///
/// `-T` because there is no terminal on the other end of this: the desktop app runs it
/// through a pipe, and compose otherwise tries to allocate a TTY and fails.
pub fn create_superuser(
    service: &str,
    username: &str,
    password: &str,
    email: Option<&str>,
) -> Vec<String> {
    let mut args: Vec<String> = vec!["compose".into(), "exec".into(), "-T".into()];
    for (key, value) in [
        ("DJANGO_SUPERUSER_USERNAME", username.to_string()),
        ("DJANGO_SUPERUSER_PASSWORD", password.to_string()),
        (
            "DJANGO_SUPERUSER_EMAIL",
            email.unwrap_or_default().to_string(),
        ),
    ] {
        args.push("-e".into());
        args.push(format!("{key}={value}"));
    }
    args.push(service.into());
    // The images run everything through uv, which owns the virtualenv the service's
    // dependencies live in — a bare `python manage.py` finds a different interpreter.
    for part in [
        "uv",
        "run",
        "python",
        "manage.py",
        "createsuperuser",
        "--noinput",
    ] {
        args.push(part.into());
    }
    args
}

pub fn ps() -> Vec<&'static str> {
    vec!["compose", "ps", "--format", "json"]
}

pub fn logs(service: Option<&str>, tail: u32) -> Vec<String> {
    logs_following(service, tail, false)
}

/// `logs`, optionally left attached to the stream.
///
/// Only a terminal can use `--follow`: it never returns on its own, so the desktop app —
/// which reads a command's whole output and then renders it — must never pass `true`.
pub fn logs_following(service: Option<&str>, tail: u32, follow: bool) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "compose".into(),
        "logs".into(),
        "--tail".into(),
        tail.to_string(),
    ];
    if follow {
        args.push("--follow".into());
    }
    if let Some(service) = service {
        args.push(service.to_string());
    }
    args
}

/// Compose's own normalisation of a directory name into a project name: lowercase, only
/// `[a-z0-9_-]` kept, and no leading non-alphanumeric.
///
/// Deliberately not `Path::file_name` — this splits on both separators regardless of
/// platform, because a Windows path may be inspected on Linux and vice versa.
pub fn basename(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed.rsplit(['/', '\\']).next().unwrap_or("").to_string()
}

pub fn project_name(path: &str) -> String {
    basename(path)
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_' || *c == '-')
        .skip_while(|c| !c.is_ascii_lowercase() && !c.is_ascii_digit())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_a_path_apart_on_either_separator() {
        assert_eq!(basename("/home/someone/MyHub"), "MyHub");
        assert_eq!(basename("/home/someone/MyHub/"), "MyHub");
        // A Windows path may well be read on Linux; `Path::file_name` would not split it.
        assert_eq!(basename(r"C:\Users\Someone\MyHub"), "MyHub");
    }

    #[test]
    fn normalises_a_project_name_the_way_compose_does() {
        assert_eq!(project_name("/home/someone/MyHub"), "myhub");
        assert_eq!(project_name("/home/someone/My Hub 2"), "myhub2");
        assert_eq!(project_name("/home/someone/-leading"), "leading");
        assert_eq!(project_name("/home/someone/lab_hub-2"), "lab_hub-2");
    }

    #[test]
    fn a_superuser_is_made_in_the_service_that_owns_the_table() {
        let args = create_superuser("mikro", "someone", "s3cret", None);
        assert_eq!(&args[..3], ["compose", "exec", "-T"]);
        // The password is an env var, never an argument.
        assert!(args.contains(&"-e".to_string()));
        assert!(args.contains(&"DJANGO_SUPERUSER_PASSWORD=s3cret".to_string()));
        assert!(!args.iter().any(|a| a == "s3cret"));
        // The service name comes before the command, as compose wants it.
        let service = args.iter().position(|a| a == "mikro").expect("the service");
        let uv = args.iter().position(|a| a == "uv").expect("the runner");
        assert!(service < uv);
        assert_eq!(args.last().unwrap(), "--noinput");
    }

    #[test]
    fn logs_take_an_optional_service() {
        assert_eq!(logs(None, 200), ["compose", "logs", "--tail", "200"]);
        assert_eq!(
            logs(Some("mikro"), 50),
            ["compose", "logs", "--tail", "50", "mikro"]
        );
    }
}
