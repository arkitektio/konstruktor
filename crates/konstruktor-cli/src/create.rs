use anyhow::{bail, Context, Result};
use clap::Args;
use konstruktor_core::catalog::{ServiceId, SERVICE_IDS};
use konstruktor_core::connect::manifest::AdvertisedHost;
use konstruktor_core::create::{
    create_hub, identifier_from_folder, CreateEvent, HubAnswers, MeshMode,
};
use konstruktor_core::hosts;
use konstruktor_core::profile;
use tokio_util::sync::CancellationToken;

use crate::ui;

const DEFAULT_COORDINATION_SERVER: &str = "go.arkitekt.live";

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
    /// Comma-separated. Defaults to rekuest,mikro,fluss,kabinet,kraph.
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
    /// none · coordination · manual
    #[arg(long, default_value = "none", value_parser = crate::parse_mesh_mode)]
    pub mesh: MeshMode,
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
    /// Skip `docker compose up -d`.
    #[arg(long)]
    pub no_start: bool,
    /// Do not open a browser for the authorization.
    #[arg(long)]
    pub no_open: bool,
    /// Never prompt; a missing answer with no default is an error.
    #[arg(long, short = 'y')]
    pub yes: bool,
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

pub async fn run(args: CreateArgs) -> Result<()> {
    let ask = Asker {
        interactive: ui::is_interactive() && !args.yes,
    };

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
    let dir = std::fs::canonicalize(requested).with_context(|| format!("resolving {requested}"))?;

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
            Some(DEFAULT_COORDINATION_SERVER.to_string()),
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
    if identifier.trim().len() < 2 {
        bail!("the hub identifier needs at least two characters");
    }

    // --- services -----------------------------------------------------------
    let services = match &args.services {
        Some(names) => parse_services(names)?,
        None => konstruktor_core::catalog::default_services(),
    };

    // --- addresses ----------------------------------------------------------
    let hosts = if args.hosts.is_empty() {
        // No tailnet identity to go on: the hub has not joined one yet, so any tailscale
        // address on this machine belongs to somebody else's and is not offered.
        let candidates = hosts::host_candidates(
            &hosts::bindings().await.unwrap_or_default(),
            &hosts::KnownMesh::default(),
        );
        // Exactly what the wizard's preset of the same name would select — the rule lives
        // in the core precisely so these two cannot answer differently. `usable` matters:
        // host_candidates reports everything it finds now, bridges included, and without
        // it `create` would happily advertise docker0.
        let chosen: Vec<AdvertisedHost> = candidates
            .iter()
            .filter(|c| c.usable && args.reach.accepts(c.kind))
            .map(|c| AdvertisedHost {
                host: c.value.clone(),
                kind: c.kind,
            })
            .collect();
        if chosen.is_empty() {
            bail!(
                "nothing on this machine matches --reach {} — widen it, or pass --host \
                 so clients have somewhere to reach this hub",
                match args.reach {
                    hosts::ReachPresetId::LocalOnly => "local-only",
                    hosts::ReachPresetId::ThisNetwork => "this-network",
                    hosts::ReachPresetId::Public => "public",
                }
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
    if args.mesh == MeshMode::Manual && mesh_key.as_deref().map(str::trim).unwrap_or("").is_empty()
    {
        bail!("`--mesh manual` needs a key — pass --mesh-key or set KONSTRUKTOR_MESH_KEY");
    }

    // Checked here rather than at the checkout: by then the hub has been authorized and
    // written, and "install git and try again" would mean creating it a second time.
    if args.dev && !konstruktor_core::git::probe().is_ready() {
        bail!("`--dev` checks the services' source out with git, which is not installed");
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
        start: !args.no_start,
        dev_hub: args.dev,
        dev_branch: args.dev_branch.clone(),
        // `--dev` is all or nothing here. Picking source mode for one service at a time
        // is a wizard affordance; the flag stays the CLI's whole answer.
        service_options: Default::default(),
    };

    summarise(&answers);

    // --- go -----------------------------------------------------------------
    let cancel = CancellationToken::new();
    let on_signal = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            on_signal.cancel();
        }
    });

    let open_browser = !args.no_open && ui::is_interactive();
    let created = create_hub(&answers, &cancel, &|event| report(event, open_browser)).await?;

    ui::say("");
    ui::ok(&format!(
        "Hub created at {}",
        created.path.to_string_lossy()
    ));
    if answers.mesh_mode != MeshMode::None {
        if created.mesh_granted {
            ui::step(&ui::dim(
                "A mesh key was granted. Once the stack is up, find the address the mesh \
                 gave it and authorize again to advertise it.",
            ));
        } else {
            ui::warn("A mesh key was asked for, but the coordination server did not grant one.");
        }
    }
    ui::say("");
    // The one line a script would want: stdout, not stderr.
    println!("{}", created.path.to_string_lossy());
    Ok(())
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

fn summarise(answers: &HubAnswers) {
    ui::table(&[
        ("folder".into(), answers.dir.clone()),
        ("coordination".into(), answers.coord_server.clone()),
        ("identifier".into(), answers.identifier.clone()),
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
            answers
                .hosts
                .iter()
                .map(|h| h.host.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ),
    ]);
    ui::say("");
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
