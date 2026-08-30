/// Host-side `docker compose` invocations. All of them run with the working directory set
/// to the deployment folder, so the project name is derived the way the Python CLI derives
/// it — deliberately not overridden with `-p`, so `arkitekt-next hub up` in that same
/// folder lands on the same stack.

pub fn up() -> Vec<&'static str> {
    vec!["compose", "up", "-d"]
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
    let mut args: Vec<String> = vec![
        "compose".into(),
        "logs".into(),
        "--tail".into(),
        tail.to_string(),
    ];
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
