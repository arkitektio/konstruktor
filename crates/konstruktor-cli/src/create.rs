use anyhow::{bail, Context, Result};
use clap::Args;
use konstruktor_core::catalog::{ServiceId, SERVICE_IDS};
use konstruktor_core::config::hub::{
    OllamaChoice, ServiceOptions, StorageMode, LOCAL_COORD_SERVER,
};
use konstruktor_core::connect::manifest::AdvertisedHost;
use konstruktor_core::create::{
    create_hub, identifier_from_folder, CreateEvent, HubAnswers, MeshMode, SeedAnswers,
};
use konstruktor_core::hosts;
use konstruktor_core::profile;
use konstruktor_core::templates;
use tokio_util::sync::CancellationToken;

use crate::ui;

#[derive(Args, Debug, Clone)]
pub struct CreateArgs {
    /// Where the deployment lives. Defaults to the current directory.
    pub dir: Option<String>,
    /// How this deployment is labelled. Defaults to the folder's name.
    #[arg(long)]
    pub name: Option<String>,
    /// The coordination server this hub answers to. `local` runs one in this stack: a
    /// self-contained hub, which nobody has to accept and which needs no network — see
    /// `--org`, `--user` and `--redeem-tokens` for what it starts out with.
    #[arg(long)]
    pub server: Option<String>,
    /// The hub's name inside the organization that accepts it.
    #[arg(long)]
    pub identifier: Option<String>,
    /// A line about the hub, sent to the coordination server with the request.
    #[arg(long)]
    pub description: Option<String>,
    /// `local` runs Rekuest here; a host points at a remote provenance authority.
    #[arg(long, default_value = "local")]
    pub rekuest: String,
    /// The kind of hub, which decides the services it starts with. `konstruktor hub
    /// templates` lists them. Left out, a terminal gets the wizard instead; with nobody
    /// to ask, `default`.
    #[arg(long, value_parser = parse_template)]
    pub template: Option<String>,
    /// Comma-separated. Overrides the template's services.
    #[arg(long, value_delimiter = ',')]
    pub services: Option<Vec<String>>,
    /// The port the gateway answers plain HTTP on.
    #[arg(long, default_value_t = konstruktor_core::defaults::HTTP_PORT)]
    pub http_port: u16,
    /// The port the gateway answers HTTPS on, with `--ssl`.
    #[arg(long, default_value_t = konstruktor_core::defaults::HTTPS_PORT)]
    pub https_port: u16,
    /// Turn HTTPS on at the gateway.
    #[arg(long)]
    pub ssl: bool,
    /// The domain name the hub is reached by. Left out, `localhost`.
    #[arg(long)]
    pub domain: Option<String>,
    /// The admin account's name.
    #[arg(long, default_value = "admin")]
    pub admin: String,
    /// Left out, a strong one is generated.
    #[arg(long)]
    pub admin_password: Option<String>,
    /// An address to advertise. Repeatable. Overrides --reach.
    #[arg(long = "host", value_name = "HOST")]
    pub hosts: Vec<String>,
    /// How far the hub should reach: local-only · this-network · public.
    ///
    /// Ignored when `--host` is given, which says exactly what to advertise.
    ///
    /// Defaults to `this-network` — and to `local-only` with `--server local`, where
    /// the hub is something a script on this machine built for itself unless told
    /// otherwise.
    #[arg(long, value_parser = crate::parse_reach)]
    pub reach: Option<hosts::ReachPresetId>,
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
    /// The mesh's control server, for `--mesh manual`. Left out, Tailscale's own.
    #[arg(long)]
    pub mesh_coord_url: Option<String>,
    /// A dev hub: check every service's source out into `mounts/` and mount it into the
    /// containers, so they run the code on this machine. Needs git.
    #[arg(long)]
    pub dev: bool,
    /// The branch to check out, with `--dev`. Left out, each repository's default branch.
    #[arg(long)]
    pub dev_branch: Option<String>,
    /// Run one service from its source instead of the code in its image. Repeatable.
    ///
    /// `mikro`, or `mikro@my-branch`: a checkout of the repository the service's image
    /// says it was built from — at the commit it names, unless a branch is asked for.
    /// `mikro=https://github.com/me/mikro[@branch]`: a checkout of another repository.
    /// `mikro=/path/to/a/folder`: a folder on this machine, used where it is — nothing is
    /// cloned, and nothing in it is touched. A checkout needs git; a folder does not.
    /// `--dev` is the first form for every service.
    #[arg(long = "from-source", value_name = "SERVICE[=URL|FOLDER][@BRANCH]")]
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
    /// Run one service on another image than the one a new hub gets: `rekuest=jhnnsrs/rekuest:1.2.3`.
    /// Repeatable. The name is the compose service — a service, or `lok`, `db`, `redis`,
    /// `rustfs`, `gateway`.
    ///
    /// `KONSTRUKTOR_IMAGES` takes the same, comma-separated, as a default for every hub
    /// created while it is set: an entry for a service a hub does not run is skipped
    /// there, where here it is an error.
    #[arg(long = "image", value_name = "SERVICE=IMAGE")]
    pub images: Vec<String>,
    /// A hub of exactly the services these images are: `--service-image jhnnsrs/mikro:7`.
    /// Repeatable. Each image is asked which service it is, so nothing but the image has
    /// to be named — which is how a client library says what it needs hosted. Rekuest
    /// runs here only when one of the images is Rekuest's.
    #[arg(long = "service-image", value_name = "IMAGE", conflicts_with_all = ["services", "template"])]
    pub service_images: Vec<String>,
    /// The services `--service-image` turned out to name, once the images were asked. Not
    /// a flag: a service named this way need not be one of the catalogue's, which is all
    /// `--services` takes.
    #[arg(skip)]
    pub services_of_images: Option<Vec<ServiceId>>,
    /// With `--server local`: the organization the hub's coordination server starts with.
    #[arg(long, default_value = "demo")]
    pub org: String,
    /// With `--server local`: the account in that organization that apps act as.
    #[arg(long, default_value = "demo")]
    pub user: String,
    /// Left out, a strong one is generated.
    #[arg(long)]
    pub user_password: Option<String>,
    /// With `--server local`: a redeem token to provision as given. Repeatable. An app
    /// trades one for a client without anybody accepting it — one token per app.
    #[arg(long = "redeem-token", value_name = "TOKEN")]
    pub redeem_token: Vec<String>,
    /// With `--server local`: how many redeem tokens to mint, besides any given. They are
    /// written to `secrets/access.json` with everything else an app needs to connect.
    #[arg(long, default_value_t = 1, value_name = "N")]
    pub redeem_tokens: usize,
    /// Print what would be written and stop. Nothing is created, no image is fetched or
    /// asked anything, and the coordination server is never contacted — so an unattended
    /// invocation can be rehearsed safely. Creating for real asks every image what it is
    /// first, so it needs them, even with `--no-start`.
    #[arg(long)]
    pub dry_run: bool,
    /// Write the files, but do not start the containers.
    #[arg(long)]
    pub no_start: bool,
    /// Do not open a browser for the authorization.
    #[arg(long)]
    pub no_open: bool,
    /// Never prompt: take every answer from flags and defaults. A missing answer with no
    /// default is an error.
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// Walk through the desktop wizard's questions — services, storage, mesh, ports and
    /// addresses — instead of taking them from flags. Flags given anyway pre-fill the
    /// answers. What happens anyway when neither `--template` nor `--services` says what
    /// the hub is.
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

pub async fn run(mut args: CreateArgs, json: bool) -> Result<()> {
    let ask = Asker {
        interactive: ui::is_interactive() && !args.yes,
    };
    if args.wizard && !ask.interactive {
        bail!("--wizard asks questions, and this is not a terminal — pass the answers as flags");
    }

    ui::say("");
    ui::say(&format!("  {}", ui::bold("Creating a hub")));
    ui::say("");

    // First, because the images are the answer to which services this hub runs: nothing
    // below asks that again.
    let described = if args.service_images.is_empty() {
        Default::default()
    } else {
        services_of_images(&mut args).await?
    };

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
    let dir = konstruktor_core::paths::canonical(requested)
        .with_context(|| format!("resolving {requested}"))?;

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

    // A hub that runs its own coordination server has nobody to ask for a mesh key, so
    // the default — ask the coordination server — means none here. Anything else that
    // was said about a mesh is refused by the core, in its own words.
    let self_contained = server.trim() == LOCAL_COORD_SERVER;
    if self_contained && args.mesh == MeshMode::Coordination {
        args.mesh = MeshMode::None;
    }

    if wants_wizard(&args, ask.interactive) {
        wizard(&mut args).await?;
    }

    // --- services -----------------------------------------------------------
    let services = services_from(&args)?;
    // Named in the summary only while it still describes the hub: `--services`, or the
    // wizard's own picks, replace what the template chose.
    let template = args
        .services
        .is_none()
        .then(|| template_of(&args).to_string());

    if args.mesh_only && args.mesh == MeshMode::None {
        bail!("`--mesh-only` needs a mesh — drop `--mesh none`");
    }

    // --- addresses ----------------------------------------------------------
    let hosts = if args.mesh_only {
        // Nothing on this machine's networks is advertised; the manifest carries the
        // tailnet node and the in-network gateway by itself.
        if !args.hosts.is_empty() {
            ui::warn(
                "--host is ignored with --mesh-only: the hub is advertised on the mesh alone.",
            );
        }
        Vec::new()
    } else if args.hosts.is_empty() {
        // Exactly what the wizard's preset of the same name selects — the rule lives in
        // the core precisely so these two cannot answer differently.
        let reach = args.reach.unwrap_or(if self_contained {
            hosts::ReachPresetId::LocalOnly
        } else {
            konstruktor_core::defaults::REACH
        });
        let chosen = hosts::discover(reach).await;
        if chosen.is_empty() {
            bail!(
                "nothing on this machine matches --reach {} — widen it, or pass --host \
                 so clients have somewhere to reach this hub",
                reach.label()
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
    // A folder somebody named is used where it is: nothing is checked out for it.
    let needs_git = args.dev
        || service_options.values().any(|o| {
            o.from_source
                && !o
                    .source
                    .as_deref()
                    .is_some_and(konstruktor_core::config::hub::is_local_folder)
        });
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
        images: parse_images(&args.images)?,
        default_images: images_of_the_environment()?,
        described,
        seed: SeedAnswers {
            organization: args.org.clone(),
            user: args.user.clone(),
            user_password: args.user_password.clone(),
            redeem_tokens: args.redeem_token.clone(),
            generated_redeem_tokens: args.redeem_tokens,
        },
    };

    summarise(&answers, template.as_deref());

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
    if created.config.running_lok().is_some() {
        ui::step(&ui::dim(&format!(
            "Runs its own coordination server. How to reach it — the address, the account \
             and the redeem tokens — is in {}.",
            konstruktor_core::generate::lok::ACCESS_FILE
        )));
    }
    ui::say("");
    // What a script would want: stdout, not stderr. The path alone, or with `--json` the
    // path and — on a self-contained hub — everything an app needs to connect to it.
    if json {
        let access = created
            .config
            .running_lok()
            .map(|lok| konstruktor_core::generate::lok::build_access(&created.config, lok));
        ui::emit_json(&serde_json::json!({
            "path": created.path.to_string_lossy(),
            "access": access,
        }))?;
    } else {
        println!("{}", created.path.to_string_lossy());
    }
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
        other => Err(format!(
            "unknown storage `{other}` — expected volumes or folder"
        )),
    }
}

/// `konstruktor hub templates`: the kinds of hub `hub create --template` can make.
pub fn templates(json: bool) -> Result<()> {
    let templates = templates::templates();
    if json {
        return ui::emit_json(&templates);
    }

    ui::say("");
    for template in &templates {
        let tag = if template.id == templates::DEFAULT {
            ui::dim("  (default)")
        } else {
            String::new()
        };
        ui::say(&format!("  {}{tag}", ui::bold(template.id)));
        ui::say(&format!("    {}", template.description));
        ui::say(&format!(
            "    {}",
            ui::dim(
                &template
                    .services
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        ));
        ui::say("");
    }
    ui::step(&ui::dim(
        "Create one with `konstruktor hub create --template <id>`.",
    ));
    ui::say("");
    Ok(())
}

pub fn parse_template(value: &str) -> Result<String, String> {
    match templates::find(value) {
        Some(template) => Ok(template.id.to_string()),
        None => Err(format!(
            "unknown template `{value}` — known ones are {}",
            templates::ids().join(", ")
        )),
    }
}

/// The services the flags ask for: `--services` when given, otherwise the template's.
/// Turns `--service-image` into what the rest of the command already understands: the
/// services the images say they are, each pinned to its image.
async fn services_of_images(
    args: &mut CreateArgs,
) -> Result<std::collections::BTreeMap<String, konstruktor_core::contract::Description>> {
    let overridden = images_of_the_environment()?;
    let mut names = Vec::new();
    let mut ids = Vec::new();
    let mut described = std::collections::BTreeMap::new();
    for image in &args.service_images {
        let image = image.trim();
        // The image that will run is the one to ask. Where the environment names another
        // build for the service this image's name stands for — a suite pointed at a server
        // not released yet — the declared one may not even be one that answers.
        let stands_for = image
            .split('@')
            .next()
            .unwrap_or(image)
            .rsplit('/')
            .next()
            .and_then(|name| name.split(':').next())
            .unwrap_or_default();
        let asked = overridden
            .get(stands_for)
            .map(String::as_str)
            .unwrap_or(image);
        let said = konstruktor_core::contract::describe(asked)
            .await
            .with_context(|| {
                format!(
                    "`{asked}` does not say which service it is when it is run with no command: \
                     it cannot be pulled, or it is a release from before a service \
                     described itself"
                )
            })?;
        // Any service is one a hub can host: what it needs, its image has just said. Only
        // its name has to be one a hub can give it.
        let known = ServiceId::parse(&said.name).with_context(|| {
            format!("`{image}` cannot be hosted under the name it gives itself")
        })?;
        if ids.contains(&known) {
            bail!("--service-image names `{}` twice", known.as_str());
        }
        ids.push(known);
        names.push(known.as_str().to_string());
        described.insert(asked.to_string(), said);
        // The image says which service this is; which build of it runs is still the
        // environment's to say. That is how a suite is pointed at a server not
        // released yet without touching what the client library declares.
        if !overridden.contains_key(known.as_str()) {
            args.images.push(format!("{}={image}", known.as_str()));
        }
    }
    args.rekuest = if names.iter().any(|name| name == ServiceId::Rekuest.as_str()) {
        "local".into()
    } else {
        "none".into()
    };
    args.services = Some(names);
    args.services_of_images = Some(ids);
    Ok(described)
}

/// `KONSTRUKTOR_IMAGES`: the images every hub created while it is set runs, by service.
fn images_of_the_environment() -> Result<std::collections::BTreeMap<String, String>> {
    parse_images(
        &std::env::var("KONSTRUKTOR_IMAGES")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|spec| !spec.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>(),
    )
    .context("reading KONSTRUKTOR_IMAGES")
}

/// The catalogue's service of that name, if it has one.
fn service_named(name: &str) -> Option<ServiceId> {
    SERVICE_IDS
        .iter()
        .copied()
        .find(|id| id.as_str() == name.trim())
}

fn known_services() -> String {
    SERVICE_IDS
        .iter()
        .map(|i| i.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn services_from(args: &CreateArgs) -> Result<Vec<ServiceId>> {
    // What the images said they are, when the hub was named by its images.
    if let Some(services) = &args.services_of_images {
        return Ok(services.clone());
    }
    match &args.services {
        Some(names) => parse_services(names),
        None => Ok(templates::find(template_of(args))
            .with_context(|| format!("unknown template `{}`", template_of(args)))?
            .services),
    }
}

fn template_of(args: &CreateArgs) -> &str {
    args.template.as_deref().unwrap_or(templates::DEFAULT)
}

/// Whether to ask the wizard's questions: when told to, and when nothing says what kind of
/// hub this is and there is somebody to ask. Naming a template, or the services
/// themselves, is the answer — the rest comes from flags and defaults.
fn wants_wizard(args: &CreateArgs, interactive: bool) -> bool {
    args.wizard || (interactive && args.template.is_none() && args.services.is_none())
}

fn summarise(answers: &HubAnswers, template: Option<&str>) {
    let mut rows: Vec<(String, String)> = vec![
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
        (
            "coordination".into(),
            if answers.coord_server.trim() == LOCAL_COORD_SERVER {
                "its own, in this stack".into()
            } else {
                answers.coord_server.clone()
            },
        ),
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
    ];
    if let Some(template) = template {
        let at = rows
            .iter()
            .position(|(key, _)| key == "services")
            .unwrap_or(rows.len());
        rows.insert(at, ("template".into(), template.to_string()));
    }
    ui::table(&rows);
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
    let current = services_from(args)?;
    let labels: Vec<String> = offered
        .iter()
        .map(|s| {
            let tag = if s.experimental {
                " (experimental)"
            } else {
                ""
            };
            format!("{}{tag} — {}", s.name, s.description)
        })
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
            let reach = args.reach.unwrap_or(konstruktor_core::defaults::REACH);
            let start = presets.iter().position(|p| p.id == reach).unwrap_or(0);
            let picked = Select::new("How far should the hub reach?", labels.clone())
                .with_starting_cursor(start)
                .prompt()?;
            if let Some(i) = labels.iter().position(|l| *l == picked) {
                args.reach = Some(presets[i].id);
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
        // One of this hub's services, whatever it is; else a name of the catalogue, to
        // say which service it is that the hub does not run.
        let id = match services.iter().find(|id| id.as_str() == name.trim()) {
            Some(id) => *id,
            None => parse_services(&[name.to_string()])?[0],
        };
        if !services.contains(&id) && id != ServiceId::Rekuest {
            bail!("{flag} {name}: this hub does not run {name} — add it to --services");
        }
        Ok(id)
    };

    let mut options: BTreeMap<ServiceId, ServiceOptions> = BTreeMap::new();
    for spec in &args.from_source {
        let asked = parse_from_source(spec)?;
        let entry = options
            .entry(named(&asked.service, "--from-source")?)
            .or_default();
        entry.from_source = true;
        entry.branch = asked.branch;
        entry.source = asked.source;
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
        options
            .entry(named("alpaka", "--ollama")?)
            .or_default()
            .ollama = Some(choice);
    }
    if !args.repositories.is_empty() {
        options
            .entry(named("kabinet", "--repository")?)
            .or_default()
            .repositories = Some(
            args.repositories
                .iter()
                .map(|r| r.trim().to_string())
                .collect(),
        );
    }

    // The core holds the same rules for the wizard; asked here too so a bad flag is
    // refused before anybody is sent to a browser.
    konstruktor_core::create::validate_service_options(&options)?;
    Ok(options)
}

/// One `--from-source`, taken apart.
#[derive(Debug, PartialEq, Eq)]
struct FromSource {
    service: String,
    /// A repository to clone, or the absolute path of a folder to use where it is. `None`
    /// leaves it to what the service's image says.
    source: Option<String>,
    branch: Option<String>,
}

/// `SERVICE`, `SERVICE@BRANCH`, `SERVICE=URL[@BRANCH]` or `SERVICE=FOLDER`.
///
/// A folder is recognised by being a path — absolute, or starting with `.` or `~` — and
/// is made absolute here, since it is mounted from wherever the hub's folder is. In a
/// repository's address an `@` is only a branch where it follows the path:
/// `git@github.com:me/mikro.git` has none, `git@github.com:me/mikro.git@dev` has one.
fn parse_from_source(spec: &str) -> Result<FromSource> {
    let clean = |value: &str| Some(value.trim().to_string()).filter(|v| !v.is_empty());
    let Some((service, source)) = spec.split_once('=') else {
        let (service, branch) = match spec.split_once('@') {
            Some((service, branch)) => (service, clean(branch)),
            None => (spec, None),
        };
        return Ok(FromSource {
            service: service.trim().to_string(),
            source: None,
            branch,
        });
    };
    let (service, source) = (service.trim().to_string(), source.trim());
    if source.is_empty() {
        bail!("--from-source {spec}: expected a repository or a folder after `=`");
    }

    let home = source.strip_prefix("~/").and_then(|rest| {
        std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(rest))
    });
    // `is_absolute` is what makes `C:\\src\\mikro` a folder on Windows, where no path starts
    // with a slash.
    if source.starts_with('/')
        || source.starts_with('.')
        || home.is_some()
        || std::path::Path::new(source).is_absolute()
    {
        let folder = home.unwrap_or_else(|| std::path::PathBuf::from(source));
        let folder = std::fs::canonicalize(&folder)
            .ok()
            .filter(|folder| folder.is_dir())
            .with_context(|| {
                format!(
                    "--from-source {spec}: there is no folder at {}",
                    folder.display()
                )
            })?;
        return Ok(FromSource {
            service,
            source: Some(folder.to_string_lossy().to_string()),
            branch: None,
        });
    }

    // Where the path of the address starts: after the authority of a URL, or after the
    // colon of `user@host:path`. A branch is what follows an `@` from there on.
    let path_from = match source.find("://") {
        Some(scheme) => source[scheme + 3..]
            .find('/')
            .map(|slash| scheme + 3 + slash),
        None => source.find(':'),
    }
    .unwrap_or(0);
    let (repository, branch) = match source[path_from..].find('@') {
        Some(at) => (
            &source[..path_from + at],
            clean(&source[path_from + at + 1..]),
        ),
        None => (source, None),
    };
    Ok(FromSource {
        service,
        source: Some(repository.to_string()),
        branch,
    })
}

/// `["rekuest=jhnnsrs/rekuest:1.2.3"]` → `{rekuest: jhnnsrs/rekuest:1.2.3}`. Split on the
/// first `=` only: an image reference may carry one of its own, in a digest.
fn parse_images(specs: &[String]) -> Result<std::collections::BTreeMap<String, String>> {
    specs
        .iter()
        .map(|spec| match spec.split_once('=') {
            Some((service, image)) if !service.trim().is_empty() && !image.trim().is_empty() => {
                Ok((service.trim().to_string(), image.trim().to_string()))
            }
            _ => {
                bail!("--image {spec}: expected SERVICE=IMAGE, e.g. rekuest=jhnnsrs/rekuest:1.2.3")
            }
        })
        .collect()
}

fn parse_services(names: &[String]) -> Result<Vec<ServiceId>> {
    names
        .iter()
        .map(|name| {
            service_named(name).ok_or_else(|| {
                anyhow::anyhow!(
                    "unknown service `{}` — `--services` takes the services this \
                     konstruktor knows by name: {}. Any other is hosted by its image, with \
                     `--service-image IMAGE`",
                    name.trim(),
                    known_services()
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// The forms `--from-source` takes: the image's own repository, another one, or a
    /// folder — and where an `@` is a branch.
    #[test]
    fn a_source_is_a_branch_a_repository_or_a_folder() {
        let parsed = |spec: &str| parse_from_source(spec).expect("a valid form");
        let from = |service: &str, source: Option<&str>, branch: Option<&str>| FromSource {
            service: service.into(),
            source: source.map(String::from),
            branch: branch.map(String::from),
        };
        assert_eq!(parsed("mikro"), from("mikro", None, None));
        assert_eq!(
            parsed("mikro@feature/zarr"),
            from("mikro", None, Some("feature/zarr"))
        );
        assert_eq!(
            parsed("mikro=https://github.com/me/mikro"),
            from("mikro", Some("https://github.com/me/mikro"), None)
        );
        assert_eq!(
            parsed("mikro=https://github.com/me/mikro@feature/zarr"),
            from(
                "mikro",
                Some("https://github.com/me/mikro"),
                Some("feature/zarr")
            )
        );
        // An `@` in the authority is a user, not a branch.
        assert_eq!(
            parsed("mikro=https://me@git.example.org/me/mikro.git"),
            from(
                "mikro",
                Some("https://me@git.example.org/me/mikro.git"),
                None
            )
        );
        assert_eq!(
            parsed("mikro=git@github.com:me/mikro.git"),
            from("mikro", Some("git@github.com:me/mikro.git"), None)
        );
        assert_eq!(
            parsed("mikro=git@github.com:me/mikro.git@dev"),
            from("mikro", Some("git@github.com:me/mikro.git"), Some("dev"))
        );

        // A folder is used where it is, by its whole path.
        let folder = std::env::temp_dir().join(format!("konstruktor-src-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let whole = std::fs::canonicalize(&folder).unwrap();
        let asked = parsed(&format!("example={}", folder.display()));
        assert_eq!(asked, from("example", Some(&whole.to_string_lossy()), None));
        assert!(konstruktor_core::config::hub::is_local_folder(
            asked.source.as_deref().unwrap()
        ));
        std::fs::remove_dir_all(&folder).ok();

        let missing = parse_from_source("example=/no/such/folder/anywhere").unwrap_err();
        assert!(missing.to_string().contains("no folder"), "{missing}");
        assert!(parse_from_source("example=").is_err());
    }

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
        let ollama = options[&ServiceId::Alpaka]
            .ollama
            .as_ref()
            .expect("a provider");
        assert!(!ollama.run_locally);
        assert_eq!(ollama.url.as_deref(), Some("http://gpu-box:11434"));
        assert_eq!(
            options[&ServiceId::Kabinet].repositories.as_deref(),
            Some(&["jhnnsrs/ome:main".to_string()][..])
        );
        assert!(!options.contains_key(&ServiceId::Kraph));
        // Nobody named a source: it is the image's to say.
        assert_eq!(mikro.source, None);
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

    /// A template is where the services come from until `--services` says otherwise.
    #[test]
    fn the_template_chooses_the_services_unless_they_are_named() {
        let none = args(&[]);
        assert_eq!(template_of(&none), templates::DEFAULT);
        assert_eq!(
            services_from(&none).unwrap(),
            konstruktor_core::defaults::services()
        );

        let personal = services_from(&args(&["--template", "personal"])).unwrap();
        assert!(personal.contains(&ServiceId::Bank) && personal.contains(&ServiceId::Kuvert));
        assert!(!personal.contains(&ServiceId::Mikro));

        let named = args(&["--template", "personal", "--services", "rekuest,mikro"]);
        assert_eq!(
            services_from(&named).unwrap(),
            [ServiceId::Rekuest, ServiceId::Mikro]
        );
    }

    /// No template and somebody to ask is the wizard; a template, the services, or
    /// nobody to ask is not.
    #[test]
    fn the_wizard_is_what_a_terminal_gets_without_a_template() {
        assert!(wants_wizard(&args(&[]), true));
        assert!(!wants_wizard(&args(&[]), false));
        assert!(!wants_wizard(&args(&["--template", "default"]), true));
        assert!(!wants_wizard(&args(&["--services", "rekuest,mikro"]), true));
        assert!(wants_wizard(
            &args(&["--template", "personal", "--wizard"]),
            true
        ));
    }

    #[test]
    fn an_unknown_template_is_refused_with_the_known_ones() {
        let error = Cli::try_parse_from(["konstruktor", "--template", "nope"])
            .err()
            .expect("refused")
            .to_string();
        assert!(error.contains("unknown template `nope`"), "{error}");
        for id in templates::ids() {
            assert!(error.contains(id), "{error}");
        }
    }

    /// The defaults the wizard starts from are the ones the flags default to.
    #[test]
    fn the_flag_defaults_are_the_shared_defaults() {
        use konstruktor_core::defaults;
        let args = args(&[]);
        assert_eq!(args.http_port, defaults::HTTP_PORT);
        assert_eq!(args.https_port, defaults::HTTPS_PORT);
        // Left unsaid, so that a self-contained hub can default to its own.
        assert_eq!(args.reach, None);
        assert_eq!(args.mesh, defaults::MESH_MODE);
        assert_eq!(args.mesh_only, defaults::MESH_ONLY);
        assert_eq!(args.storage, defaults::STORAGE);
        assert_eq!(!args.no_start, defaults::START);
    }

    #[test]
    fn an_image_pin_is_split_on_its_first_equals_sign() {
        let pins = parse_images(&[
            "rekuest=jhnnsrs/rekuest:5.0.1".to_string(),
            "db = jhnnsrs/daten@sha256:abc=".to_string(),
        ])
        .unwrap();
        assert_eq!(pins["rekuest"], "jhnnsrs/rekuest:5.0.1");
        assert_eq!(pins["db"], "jhnnsrs/daten@sha256:abc=");
        assert!(parse_images(&["rekuest".to_string()]).is_err());
        assert!(parse_images(&["=jhnnsrs/rekuest".to_string()]).is_err());
    }
}
