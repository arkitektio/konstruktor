use anyhow::{bail, Context, Result};
use clap::Args;
use konstruktor_core::config::hub::{OllamaChoice, ServiceOptions, StorageMode};
use konstruktor_core::catalog::{ServiceId, SERVICE_IDS};
use konstruktor_core::connect::manifest::AdvertisedHost;
use konstruktor_core::create::{
    create_hub, identifier_from_folder, CreateEvent, HubAnswers, MeshMode,
};
use konstruktor_core::hosts;
use konstruktor_core::profile;
use tokio_util::sync::CancellationToken;

use crate::ui;

#[derive(Args, Debug, Clone)]
pub struct CreateArgs {
    /// Where the deployment lives. Defaults to the current directory.
    pub dir: Option<String>,
    /// How this deployment is labelled. Defaults to the folder's name.
    #[arg(long)]
    pub name: Option<String>,
    /// The coordination server this hub answers to.
    #[arg(long)]
    pub server: Option<String>,
    /// The hub's name inside the organization that accepts it.
    #[arg(long)]
    pub identifier: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    /// `local` runs Rekuest here; a host points at a remote provenance authority.
    #[arg(long, default_value = "local")]
    pub rekuest: String,
    /// Comma-separated. Defaults to rekuest,mikro,fluss,kabinet,kraph,alpaka.
    #[arg(long, value_delimiter = ',')]
    pub services: Option<Vec<String>>,
    #[arg(long, default_value_t = 7080)]
    pub http_port: u16,
    #[arg(long, default_value_t = 7443)]
    pub https_port: u16,
    #[arg(long)]
    pub ssl: bool,
    #[arg(long)]
    pub domain: Option<String>,
    #[arg(long, default_value = "admin")]
    pub admin: String,
    /// Left out, a strong one is generated.
    #[arg(long)]
    pub admin_password: Option<String>,
    /// An address to advertise. Repeatable. Overrides --reach.
    #[arg(long = "host")]
    pub hosts: Vec<String>,
    /// How far the hub should reach: local-only · this-network · public.
    ///
    /// Ignored when `--host` is given, which says exactly what to advertise.
    #[arg(long, default_value = "this-network", value_parser = crate::parse_reach)]
    pub reach: hosts::ReachPresetId,
    /// none · coordination · manual. By default the hub asks the coordination server for
    /// a key to join the organization's mesh.
    #[arg(long, default_value = "coordination", value_parser = crate::parse_mesh_mode)]
    pub mesh: MeshMode,
    /// Reach the hub over the mesh only: no port is opened on this machine and no
    /// address on its networks is advertised — just the tailnet node, and the gateway's
    /// name on the docker network for plugin apps. Every client has to be on the mesh.
    #[arg(long)]
    pub mesh_only: bool,
    /// A pre-authorized key, for `--mesh manual`. Prefer KONSTRUKTOR_MESH_KEY.
    #[arg(long)]
    pub mesh_key: Option<String>,
    #[arg(long)]
    pub mesh_coord_url: Option<String>,
    /// A dev hub: check every service's source out into `mounts/` and mount it into the
    /// containers, so they run the code on this machine. Needs git.
    #[arg(long)]
    pub dev: bool,
    /// The branch to check out, with `--dev`. Left out, each repository's default branch.
    #[arg(long)]
    pub dev_branch: Option<String>,
    /// Run one service from a checkout of its source instead of its image: `mikro`, or
    /// `mikro@my-branch`. Repeatable. Needs git. `--dev` is the same for every service.
    #[arg(long = "from-source", value_name = "SERVICE[@BRANCH]")]
    pub from_source: Vec<String>,
    /// Turn Django's debug mode on for one service. Repeatable. Debug shows internals to
    /// anyone who can reach the service.
    #[arg(long = "debug", value_name = "SERVICE")]
    pub debug: Vec<String>,
    /// Where Alpaka's language models come from: `local` runs an Ollama in this stack;
    /// anything else is the address of one that already exists.
    #[arg(long, value_name = "local|URL")]
    pub ollama: Option<String>,
    /// A repository Kabinet offers apps from, e.g. `jhnnsrs/ome:main`. Repeatable;
    /// replaces the ones Kabinet starts with.
    #[arg(long = "repository", value_name = "OWNER/REPO:BRANCH")]
    pub repositories: Vec<String>,
    /// Where the database and object storage keep their data. `volumes` (the default)
    /// uses Docker's own named volumes, which is by far the fastest on Docker Desktop;
    /// `folder` bind-mounts `./db_data` and `./rustfs_data` inside the deployment folder
    /// so the data is a directory you can see — at a real cost in I/O on macOS and
    /// Windows.
    #[arg(long, default_value = "volumes", value_parser = parse_storage)]
    pub storage: StorageMode,
    /// Print what would be written and stop. Nothing is created, and the coordination
    /// server is never contacted — so an unattended invocation can be rehearsed safely.
    #[arg(long)]
    pub dry_run: bool,
    /// Skip `docker compose up -d`.
    #[arg(long)]
    pub no_start: bool,
    /// Do not open a browser for the authorization.
    #[arg(long)]
    pub no_open: bool,
    /// Never prompt; a missing answer with no default is an error.
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// Walk through the desktop wizard's questions — services, storage, mesh, ports and
    /// addresses — instead of taking them from flags. Flags given anyway pre-fill the
    /// answers.
    #[arg(long, conflicts_with = "yes")]
    pub wizard: bool,
}

/// Asks, when there is somebody to ask. Otherwise takes the default, and fails loudly
/// when there is not one — a CI run must never block on a prompt.
struct Asker {
    interactive: bool,
}

impl Asker {
    fn text(&self, prompt: &str, flag: &str, default: Option<String>) -> Result<String> {
        if let Some(given) = &default {
            if !self.interactive {
                return Ok(given.clone());
            }
        }
        if !self.interactive {
            bail!("no value for {prompt} — pass {flag}");
        }
        let mut question = inquire::Text::new(prompt);
        if let Some(d) = &default {
            question = question.with_default(d);
        }
        Ok(question.prompt()?)
    }
}

pub async fn run(mut args: CreateArgs) -> Result<()> {
    let ask = Asker {
        interactive: ui::is_interactive() && !args.yes,
    };
    if args.wizard && !ask.interactive {
        bail!("--wizard asks questions, and this is not a terminal — pass the answers as flags");
    }

    ui::say("");
    ui::say(&format!("  {}", ui::bold("Creating a hub")));
    ui::say("");

    // --- folder ------------------------------------------------------------
    //
    // The current directory unless you name another one, the way `git init [dir]` does.
    // A hub folder holds the database and the object store, so it has to be possible to
    // put one on a chosen disk — but never implicitly: nothing here defaults to somewhere
    // you are not. Wherever it lands, the registry is what finds it again.
    let requested = args.dir.as_deref().unwrap_or(".");

    // Created before it is validated, as `git init` also does — so a mistyped path leaves
    // an empty folder behind. It has to exist for `canonicalize` to have an answer.
    std::fs::create_dir_all(requested).with_context(|| format!("creating {requested}"))?;

    // Load-bearing, and it has to happen before `HubAnswers` is built: the core hands
    // `answers.dir` straight to the registry, which compares paths as raw strings. A
    // relative path there would defeat the collision check and be recorded unusable.
    let dir = konstruktor_core::paths::canonical(requested).with_context(|| format!("resolving {requested}"))?;

    if profile::holds_a_hub(&dir) {
        bail!(
            "{} already holds a hub — `konstruktor status` describes it, and \
             `konstruktor up` starts it. Create a new one in an empty folder.",
            dir.display()
        );
    }

    let name = match &args.name {
        Some(name) => name.clone(),
        None => konstruktor_core::compose::basename(&dir.to_string_lossy()),
    };

    // --- coordination server -----------------------------------------------
    let server = match &args.server {
        Some(server) => server.clone(),
        None => ask.text(
            "Coordination server",
            "--server",
            Some(konstruktor_core::defaults::COORDINATION_SERVER.to_string()),
        )?,
    };

    let identifier = match &args.identifier {
        Some(id) => id.clone(),
        None => {
            let suggested = identifier_from_folder(&dir);
            let suggested = (!suggested.is_empty()).then_some(suggested);
            ask.text("Hub identifier", "--identifier", suggested)?
        }
    };
    // Checked before anything is asked of the coordination server, by the same rule the
    // wizard holds.
    konstruktor_core::create::validate_identifier(&identifier)?;

    if args.wizard {
        wizard(&mut args).await?;
    }

    // --- services -----------------------------------------------------------
    let services = match &args.services {
        Some(names) => parse_services(names)?,
        None => konstruktor_core::catalog::default_services(),
    };

    if args.mesh_only && args.mesh == MeshMode::None {
        bail!("`--mesh-only` needs a mesh — drop `--mesh none`");
    }

    // --- addresses ----------------------------------------------------------
    let hosts = if args.mesh_only {
        // Nothing on this machine's networks is advertised; the manifest carries the
        // tailnet node and the in-network gateway by itself.
        if !args.hosts.is_empty() {
            ui::warn("--host is ignored with --mesh-only: the hub is advertised on the mesh alone.");
        }
        Vec::new()
    } else if args.hosts.is_empty() {
        // Exactly what the wizard's preset of the same name selects — the rule lives in
        // the core precisely so these two cannot answer differently.
        let chosen = hosts::discover(args.reach).await;
        if chosen.is_empty() {
            bail!(
                "nothing on this machine matches --reach {} — widen it, or pass --host \
                 so clients have somewhere to reach this hub",
                args.reach.label()
            );
        }
        chosen
    } else {
        // A hand-given address is taken at face value; classification only decides how
        // widely the coordination server will offer it. The shared classifier is what
        // makes `--host localhost` local and `--host 100.64.1.2` a tailnet address —
        // both of which used to come out public.
        args.hosts
            .iter()
            .map(|host| AdvertisedHost {
                host: host.clone(),
                kind: hosts::classify_host(host),
            })
            .collect()
    };

    // --- mesh ---------------------------------------------------------------
    // Prefer the environment: a key on the command line lands in shell history.
    let mesh_key = args
        .mesh_key
        .clone()
        .or_else(|| std::env::var("KONSTRUKTOR_MESH_KEY").ok());
    // The core refuses this too; said here first, in terms of the flags.
    if args.mesh == MeshMode::Manual && mesh_key.as_deref().map(str::trim).unwrap_or("").is_empty()
    {
        bail!("`--mesh manual` needs a key — pass --mesh-key or set KONSTRUKTOR_MESH_KEY");
    }

    let service_options = service_options_from(&args, &services)?;

    // Checked here rather than at the checkout: by then the hub has been authorized and
    // written, and "install git and try again" would mean creating it a second time.
    let needs_git = args.dev || service_options.values().any(|o| o.from_source);
    if needs_git && !konstruktor_core::git::probe().is_ready() {
        bail!("running services from source checks them out with git, which is not installed");
    }

    let answers = HubAnswers {
        dir: dir.to_string_lossy().to_string(),
        name,
        coord_server: server.clone(),
        identifier: identifier.trim().to_string(),
        description: args.description.clone(),
        rekuest_server: args.rekuest.clone(),
        services,
        http_port: args.http_port,
        https_port: args.https_port,
        ssl: args.ssl,
        domain: args.domain.clone(),
        global_admin: args.admin.clone(),
        global_admin_password: args.admin_password.clone(),
        global_description: None,
        hosts,
        // The CLI has nobody to ask: a probe needs an external prober configured, and
        // `create` runs before anything is listening in any case.
        reachable_hosts: Vec::new(),
        mesh_mode: args.mesh.clone(),
        mesh_auth_key: mesh_key,
        mesh_coord_url: args.mesh_coord_url.clone(),
        mesh_only: args.mesh_only,
        start: !args.no_start,
        dev_hub: args.dev,
        dev_branch: args.dev_branch.clone(),
        storage: args.storage,
        // `--dev` and these are a union, as in the wizard: `--dev` for every service,
        // `--from-source` for the ones named.
        service_options,
    };

    summarise(&answers);

    if args.dry_run {
        ui::say(&ui::bold("  Would write:"));
        for name in konstruktor_core::create::preview_files(&answers) {
            ui::step(&ui::dim(&name));
        }
        ui::say("");
        ui::step("Nothing was created. Drop --dry-run to do it for real.");
        ui::say("");
        return Ok(());
    }

    // --- go -----------------------------------------------------------------
    let cancel = CancellationToken::new();
    let on_signal = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            on_signal.cancel();
        }
    });

    let open_browser = !args.no_open && ui::is_interactive();
    // Remembered from the event rather than the result: a start that fails still leaves a
    // written hub whose key is ticking, and that is exactly when it has to be said.
    let granted = std::sync::atomic::AtomicBool::new(false);
    let result = create_hub(&answers, &cancel, &|event| {
        if let CreateEvent::Granted { mesh_key } = &event {
            granted.store(*mesh_key, std::sync::atomic::Ordering::Relaxed);
        }
        report(event, open_browser)
    })
    .await;
    let requested = answers.mesh_mode == MeshMode::Coordination;
    let granted = granted.load(std::sync::atomic::Ordering::Relaxed);

    let created = match result {
        Ok(created) => created,
        Err(error @ konstruktor_core::create::CreateError::StartFailed) => {
            let reporter = profile::read_profile(&dir).is_ok_and(|p| p.config.reporter.is_some());
            ui::say("");
            report_outcome(requested, granted, reporter, false);
            return Err(error.into());
        }
        Err(error) => return Err(error.into()),
    };

    ui::say("");
    ui::ok(&format!(
        "Hub created at {}",
        created.path.to_string_lossy()
    ));
    report_outcome(
        requested,
        created.mesh_granted,
        created.config.reporter.is_some(),
        answers.start,
    );
    ui::say("");
    // The one line a script would want: stdout, not stderr.
    println!("{}", created.path.to_string_lossy());
    Ok(())
}

/// What an authorization left behind that a person has to know: whether the mesh key
/// came, the fifteen minutes it lives when the stack is not up yet, and whether the hub
/// reports its own health. Shared by `hub create` and `authorize`.
pub(crate) fn report_outcome(requested: bool, granted: bool, reporter: bool, started: bool) {
    if requested {
        if !granted {
            ui::warn("A mesh key was asked for, but the coordination server did not grant one.");
        } else if started {
            ui::step(&ui::dim(
                "A mesh key was granted: the hub is joining the mesh, and the coordination \
                 server advertises its tailnet address once it has.",
            ));
        } else {
            ui::warn(
                "A mesh key was granted. It is single-use and expires 15 minutes after it was \
                 issued — run `konstruktor up` before then, or `konstruktor authorize \
                 --mesh-key fresh` for a new one.",
            );
        }
    }
    if reporter {
        ui::step(&ui::dim(
            "The hub reports its own health to the coordination server while it runs.",
        ));
    }
}

pub(crate) fn report(event: CreateEvent, open_browser: bool) {
    match event {
        CreateEvent::CheckingDocker => ui::step("Checking Docker…"),
        CreateEvent::Building => ui::step("Building the profile…"),
        CreateEvent::Staged {
            user_code,
            verification_uri_complete,
            ..
        } => {
            ui::say("");
            ui::step("Somebody with an account has to accept this hub:");
            ui::say("");
            ui::say(&format!("      {}", ui::bold(&verification_uri_complete)));
            ui::say(&format!("      code  {}", ui::bold(&user_code)));
            ui::say("");
            if open_browser {
                ui::open_in_browser(&verification_uri_complete);
            }
        }
        CreateEvent::Waiting { seconds_left, .. } => {
            let minutes = seconds_left / 60;
            let seconds = seconds_left % 60;
            ui::progress(&ui::dim(&format!(
                "Waiting for it to be accepted… {minutes}m{seconds:02}s left"
            )));
        }
        CreateEvent::Granted { .. } => {
            ui::end_progress();
            ui::ok("Accepted.");
        }
        CreateEvent::Writing { file } => ui::step(&ui::dim(&format!("wrote {file}"))),
        CreateEvent::Cloning {
            service, branch, ..
        } => ui::step(&ui::dim(&match branch {
            Some(branch) => format!("checking {service} out at {branch}…"),
            None => format!("checking {service} out…"),
        })),
        CreateEvent::Starting => ui::step("Starting the stack…"),
        CreateEvent::Log { line } => ui::step(&ui::dim(&line)),
        CreateEvent::Done { .. } => {}
    }
}

pub fn parse_storage(value: &str) -> Result<StorageMode, String> {
    match value {
        "volumes" | "docker-volumes" => Ok(StorageMode::DockerVolumes),
        "folder" | "deployment-folder" => Ok(StorageMode::DeploymentFolder),
        other => Err(format!("unknown storage `{other}` — expected volumes or folder")),
    }
}

fn summarise(answers: &HubAnswers) {
    ui::table(&[
        ("folder".into(), answers.dir.clone()),
        (
            "storage".into(),
            match answers.storage {
                StorageMode::DockerVolumes => "Docker volumes (fast)".into(),
                StorageMode::DeploymentFolder => {
                    "bind mounts in the folder (slow on Docker Desktop)".into()
                }
            },
        ),
        ("coordination".into(), answers.coord_server.clone()),
        ("identifier".into(), answers.identifier.clone()),
        (
            "mesh".into(),
            match answers.mesh_mode {
                MeshMode::Coordination => "asks the coordination server for a key".into(),
                MeshMode::Manual => "joins with the key you supplied".into(),
                MeshMode::None => "none".into(),
            },
        ),
        (
            "services".into(),
            answers
                .services
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ),
        (
            "advertised".into(),
            if answers.mesh_only {
                "the mesh only — no ports opened here".into()
            } else {
                answers
                    .hosts
                    .iter()
                    .map(|h| h.host.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            },
        ),
    ]);
    ui::say("");
    if answers.mesh_only {
        ui::warn(
            "Mesh-only: every request goes through the mesh. Clients and apps on this \
             network that are not on the mesh cannot reach the hub, and relayed tailnet \
             traffic is slower than a direct connection.",
        );
        ui::say("");
    }
}

/// `--wizard`: the desktop wizard's questions after the server and the identifier, in its
/// order — services, storage, mesh, then ports and addresses unless mesh-only. Each is
/// pre-filled with what the flags say, which are the shared defaults unless given.
async fn wizard(args: &mut CreateArgs) -> Result<()> {
    use inquire::{Confirm, CustomType, MultiSelect, Password, Select, Text};
    use konstruktor_core::catalog::catalog;

    ui::say("");

    // --- services ---------------------------------------------------------------
    let offered: Vec<_> = catalog().into_iter().filter(|s| s.emitted).collect();
    let current = match &args.services {
        Some(names) => parse_services(names)?,
        None => konstruktor_core::defaults::services(),
    };
    let labels: Vec<String> = offered
        .iter()
        .map(|s| format!("{} — {}", s.name, s.description))
        .collect();
    let ticked: Vec<usize> = offered
        .iter()
        .enumerate()
        .filter(|(_, s)| current.contains(&s.id))
        .map(|(i, _)| i)
        .collect();
    let chosen = MultiSelect::new("Services", labels.clone())
        .with_default(&ticked)
        .prompt()?;
    if chosen.is_empty() {
        bail!("pick at least one service");
    }
    args.services = Some(
        chosen
            .iter()
            .filter_map(|label| labels.iter().position(|l| l == label))
            .map(|i| offered[i].id.as_str().to_string())
            .collect(),
    );

    // --- storage ----------------------------------------------------------------
    let storage_choices = vec![
        "Docker volumes — fast; the engine keeps the data",
        "Folders in the deployment — data you can see, slow on Docker Desktop",
    ];
    let storage = Select::new("Where should the data live?", storage_choices.clone())
        .with_starting_cursor(match args.storage {
            StorageMode::DockerVolumes => 0,
            StorageMode::DeploymentFolder => 1,
        })
        .prompt()?;
    args.storage = if storage == storage_choices[0] {
        StorageMode::DockerVolumes
    } else {
        StorageMode::DeploymentFolder
    };

    // --- mesh -------------------------------------------------------------------
    let mesh_choices = vec![
        "Join the organization's mesh — the coordination server grants the key",
        "Use a key I already have",
        "No mesh — reachable only at this machine's addresses",
    ];
    let mesh = Select::new("Mesh", mesh_choices.clone())
        .with_starting_cursor(match args.mesh {
            MeshMode::Coordination => 0,
            MeshMode::Manual => 1,
            MeshMode::None => 2,
        })
        .prompt()?;
    args.mesh = match mesh_choices.iter().position(|c| *c == mesh) {
        Some(0) => MeshMode::Coordination,
        Some(1) => MeshMode::Manual,
        _ => MeshMode::None,
    };
    if args.mesh == MeshMode::Manual {
        if args.mesh_key.is_none() && std::env::var("KONSTRUKTOR_MESH_KEY").is_err() {
            args.mesh_key = Some(
                Password::new("Mesh auth key")
                    .with_display_mode(inquire::PasswordDisplayMode::Masked)
                    .without_confirmation()
                    .prompt()?,
            );
        }
        let url = Text::new("Control server (empty for Tailscale's own)")
            .with_default(args.mesh_coord_url.as_deref().unwrap_or(""))
            .prompt()?;
        args.mesh_coord_url = Some(url.trim().to_string()).filter(|u| !u.is_empty());
    }
    args.mesh_only = args.mesh != MeshMode::None
        && Confirm::new("Mesh only? No ports are opened here, and only mesh clients can connect")
            .with_default(args.mesh_only)
            .prompt()?;

    // --- ports and addresses, unless mesh-only -----------------------------------
    if !args.mesh_only {
        args.http_port = CustomType::<u16>::new("HTTP port")
            .with_default(args.http_port)
            .prompt()?;
        args.https_port = CustomType::<u16>::new("HTTPS port")
            .with_default(args.https_port)
            .prompt()?;
        if args.http_port == args.https_port {
            bail!("the two ports must differ");
        }

        if args.hosts.is_empty() {
            let candidates = hosts::host_candidates(
                &hosts::bindings().await.unwrap_or_default(),
                &hosts::KnownMesh::default(),
            );
            let presets = hosts::reach_presets(&candidates);
            let labels: Vec<String> = presets
                .iter()
                .map(|p| {
                    let found = if p.values.is_empty() {
                        "nothing on this machine".to_string()
                    } else {
                        p.values.join(", ")
                    };
                    format!("{} — {found}", p.label)
                })
                .collect();
            let start = presets.iter().position(|p| p.id == args.reach).unwrap_or(0);
            let picked = Select::new("How far should the hub reach?", labels.clone())
                .with_starting_cursor(start)
                .prompt()?;
            if let Some(i) = labels.iter().position(|l| *l == picked) {
                args.reach = presets[i].id;
            }
        }
    }
    ui::say("");
    Ok(())
}

/// The per-service answers the wizard collects under each service's gear, from flags.
/// Only services somebody said something about are in the map, as in the wizard: an
/// untouched service takes the defaults.
fn service_options_from(
    args: &CreateArgs,
    services: &[ServiceId],
) -> Result<std::collections::BTreeMap<ServiceId, ServiceOptions>> {
    use std::collections::BTreeMap;

    let named = |name: &str, flag: &str| -> Result<ServiceId> {
        let id = parse_services(&[name.to_string()])?[0];
        if !services.contains(&id) && id != ServiceId::Rekuest {
            bail!("{flag} {name}: this hub does not run {name} — add it to --services");
        }
        Ok(id)
    };

    let mut options: BTreeMap<ServiceId, ServiceOptions> = BTreeMap::new();
    for spec in &args.from_source {
        let (name, branch) = match spec.split_once('@') {
            Some((name, branch)) => (name, Some(branch.trim().to_string())),
            None => (spec.as_str(), None),
        };
        let entry = options.entry(named(name, "--from-source")?).or_default();
        entry.from_source = true;
        entry.branch = branch.filter(|b| !b.is_empty());
    }
    for name in &args.debug {
        options.entry(named(name, "--debug")?).or_default().debug = true;
    }
    if let Some(ollama) = args.ollama.as_deref().map(str::trim) {
        let choice = match ollama {
            "local" => OllamaChoice {
                run_locally: true,
                url: None,
            },
            url => OllamaChoice {
                run_locally: false,
                url: Some(url.to_string()),
            },
        };
        options.entry(named("alpaka", "--ollama")?).or_default().ollama = Some(choice);
    }
    if !args.repositories.is_empty() {
        options.entry(named("kabinet", "--repository")?).or_default().repositories =
            Some(args.repositories.iter().map(|r| r.trim().to_string()).collect());
    }

    // The core holds the same rules for the wizard; asked here too so a bad flag is
    // refused before anybody is sent to a browser.
    konstruktor_core::create::validate_service_options(&options)?;
    Ok(options)
}

fn parse_services(names: &[String]) -> Result<Vec<ServiceId>> {
    names
        .iter()
        .map(|name| {
            SERVICE_IDS
                .into_iter()
                .find(|id| id.as_str() == name.trim())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "unknown service `{}` — known ones are {}",
                        name.trim(),
                        SERVICE_IDS
                            .iter()
                            .map(|i| i.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: CreateArgs,
    }

    fn args(extra: &[&str]) -> CreateArgs {
        let mut argv = vec!["konstruktor"];
        argv.extend_from_slice(extra);
        Cli::try_parse_from(argv).expect("the flags parse").args
    }

    /// The flags fill exactly the answers the wizard's gear collects, and nothing for a
    /// service nobody mentioned.
    #[test]
    fn per_service_flags_become_the_wizards_answers() {
        let args = args(&[
            "--from-source",
            "mikro@feature/zarr",
            "--from-source",
            "fluss",
            "--debug",
            "mikro",
            "--ollama",
            "http://gpu-box:11434",
            "--repository",
            "jhnnsrs/ome:main",
        ]);
        let services = konstruktor_core::defaults::services();
        let options = service_options_from(&args, &services).expect("valid answers");

        let mikro = &options[&ServiceId::Mikro];
        assert!(mikro.from_source && mikro.debug);
        assert_eq!(mikro.branch.as_deref(), Some("feature/zarr"));
        assert!(options[&ServiceId::Fluss].from_source);
        assert_eq!(options[&ServiceId::Fluss].branch, None);
        let ollama = options[&ServiceId::Alpaka].ollama.as_ref().expect("a provider");
        assert!(!ollama.run_locally);
        assert_eq!(ollama.url.as_deref(), Some("http://gpu-box:11434"));
        assert_eq!(
            options[&ServiceId::Kabinet].repositories.as_deref(),
            Some(&["jhnnsrs/ome:main".to_string()][..])
        );
        assert!(!options.contains_key(&ServiceId::Kraph));
    }

    /// Naming a service the hub does not run is a mistake worth saying, not an answer to
    /// drop silently.
    #[test]
    fn a_flag_for_a_service_that_is_not_running_is_refused() {
        let args = args(&["--debug", "elektro"]);
        let services = konstruktor_core::defaults::services();
        assert!(!services.contains(&ServiceId::Elektro));
        assert!(service_options_from(&args, &services).is_err());
    }

    /// The defaults the wizard starts from are the ones the flags default to.
    #[test]
    fn the_flag_defaults_are_the_shared_defaults() {
        use konstruktor_core::defaults;
        let args = args(&[]);
        assert_eq!(args.http_port, defaults::HTTP_PORT);
        assert_eq!(args.https_port, defaults::HTTPS_PORT);
        assert_eq!(args.reach, defaults::REACH);
        assert_eq!(args.mesh, defaults::MESH_MODE);
        assert_eq!(args.mesh_only, defaults::MESH_ONLY);
        assert_eq!(args.storage, defaults::STORAGE);
        assert_eq!(!args.no_start, defaults::START);
    }
}
