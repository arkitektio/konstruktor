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
/// Removes the data as well — the only destructive path in the app.
pub fn down_volumes() -> Vec<&'static str> {
    vec!["compose", "down", "--volumes"]
}
pub fn ps() -> Vec<&'static str> {
    vec!["compose", "ps", "--format", "json"]
}

pub fn logs(service: Option<&str>, tail: u32) -> Vec<String> {
    let mut args: Vec<String> = vec!["compose".into(), "logs".into(), "--tail".into(), tail.to_string()];
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
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_string()
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
    fn logs_take_an_optional_service() {
        assert_eq!(logs(None, 200), ["compose", "logs", "--tail", "200"]);
        assert_eq!(
            logs(Some("mikro"), 50),
            ["compose", "logs", "--tail", "50", "mikro"]
        );
    }
}
