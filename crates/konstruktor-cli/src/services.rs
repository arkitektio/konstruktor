//! `konstruktor hub services`: what a hub runs, and adding or taking services out after it
//! was created.
//!
//! Everything is `konstruktor_core::services` — the same calls the desktop app's "Manage
//! services" dialog makes. A change is a re-authorization: the new set goes to the
//! coordination server, somebody accepts it, and only then is anything written. Then the
//! stack is brought to it, unless `--no-apply`.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use konstruktor_core::catalog::{catalog, ServiceId, SERVICE_IDS};
use konstruktor_core::contract::Description;
use konstruktor_core::services::{self, ServiceChange, ServicePlan};
use konstruktor_core::{credentials, profile};
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::manage::Target;
use crate::ui;

#[derive(Subcommand, Debug, Clone)]
pub enum ServicesCommand {
    /// List what the hub runs, what it could, and which removed services still keep
    /// their data.
    List(Target),
    /// Add services: re-authorize with them, write their files, and start them.
    ///
    /// By name for the services this konstruktor knows, or by image for any service at
    /// all (`--image`): the image is asked which service it is.
    Add(ChangeArgs),
    /// Take services out, keeping their data.
    ///
    /// Their databases and buckets are kept, so adding one back later finds its data
    /// again.
    Remove(ChangeArgs),
    /// Bring the running stack to the services the profile names.
    ///
    /// Creates missing databases, starts new containers, removes the ones taken out, and
    /// restarts the gateway and every service so they read their rewritten configs. What
    /// `--no-apply` leaves for later.
    Apply(Target),
}

#[derive(Args, Debug, Clone)]
pub struct ChangeArgs {
    /// The services, by id: `konstruktor hub services list` shows them.
    #[arg(required_unless_present = "images", value_parser = parse_service)]
    pub services: Vec<ServiceId>,
    /// With `add`: a service to add by the image it is, e.g. `--image example:1`.
    /// Repeatable. The image is asked which service it is, so it can be one this
    /// konstruktor has never heard of.
    #[arg(long = "image", value_name = "IMAGE")]
    pub images: Vec<String>,
    /// The hub: a path, or a name from `konstruktor list`. Defaults to here.
    // A flag rather than a leading positional: an optional hub in front of a list of
    // services cannot be told apart from the first service — as with `checkout --in`.
    #[arg(long = "in", value_name = "TARGET")]
    pub in_hub: Option<String>,
    /// Write the files, but leave the running stack alone — `konstruktor hub services
    /// apply` brings it to them later.
    #[arg(long)]
    pub no_apply: bool,
    /// Do not open a browser for the authorization.
    #[arg(long)]
    pub no_open: bool,
    /// Answer yes to the confirmation. It is never asked when this is not a terminal.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

/// A service by name. Any name a service can have is taken — a hub may run one that is
/// not in the catalogue, and it is removed by its name like any other; whether the hub
/// has it, or an image for it, is the plan's to say.
pub fn parse_service(value: &str) -> Result<ServiceId, String> {
    let name = value.trim().to_ascii_lowercase();
    ServiceId::parse(&name).map_err(|error| {
        format!(
            "unknown service `{value}`: {error}. The ones this konstruktor knows by name \
             are {}",
            SERVICE_IDS
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

/// Asks each of `images` which service it is: the services to add, the image each runs,
/// and what each said — so it is not asked a second time.
async fn services_of_images(
    images: &[String],
) -> Result<(
    Vec<ServiceId>,
    BTreeMap<ServiceId, String>,
    BTreeMap<String, Description>,
)> {
    let mut ids = Vec::new();
    let mut by_service = BTreeMap::new();
    let mut described = BTreeMap::new();
    for image in images {
        let image = image.trim();
        let said = konstruktor_core::contract::describe(image)
            .await
            .with_context(|| {
                format!(
                    "`{image}` does not say which service it is when it is run with no \
                     command: it cannot be pulled, or it is a release from before a service \
                     described itself"
                )
            })?;
        let id = ServiceId::parse(&said.name).with_context(|| {
            format!("`{image}` cannot be hosted under the name it gives itself")
        })?;
        if ids.contains(&id) {
            bail!("--image names `{id}` twice");
        }
        ids.push(id);
        by_service.insert(id, image.to_string());
        described.insert(image.to_string(), said);
    }
    Ok((ids, by_service, described))
}

pub async fn run(command: ServicesCommand, json: bool) -> Result<()> {
    match command {
        ServicesCommand::List(target) => list(&target, json),
        ServicesCommand::Add(args) => {
            let (by_image, images, described) = services_of_images(&args.images).await?;
            if let Some(twice) = by_image.iter().find(|id| args.services.contains(id)) {
                bail!("`{twice}` is named both by name and by `--image` — one will do");
            }
            let change = ServiceChange {
                add: args.services.iter().copied().chain(by_image).collect(),
                remove: Vec::new(),
                images,
            };
            change_services(&args, change, described).await
        }
        ServicesCommand::Remove(args) => {
            if !args.images.is_empty() {
                bail!("`--image` adds a service; one is removed by its name");
            }
            let change = ServiceChange {
                add: Vec::new(),
                remove: args.services.clone(),
                images: BTreeMap::new(),
            };
            change_services(&args, change, BTreeMap::new()).await
        }
        ServicesCommand::Apply(target) => apply(&target).await,
    }
}

/// One row of `list`.
#[derive(Serialize)]
struct Listed {
    id: ServiceId,
    name: String,
    /// `running`, `off`, or `kept` — taken out, its data still provisioned.
    state: &'static str,
    experimental: bool,
}

fn list(target: &Target, json: bool) -> Result<()> {
    let dir = target.resolve()?;
    let config = profile::read_profile(&dir)?.config;
    let running = config.enabled_services();

    let state_of = |id: ServiceId| {
        if running.contains(&id) {
            "running"
        } else if config.get(id).is_some_and(|block| block.retained) {
            "kept"
        } else {
            "off"
        }
    };
    let mut rows: Vec<Listed> = catalog()
        .into_iter()
        .filter(|meta| meta.emitted)
        .map(|meta| Listed {
            state: state_of(meta.id),
            id: meta.id,
            name: meta.name,
            experimental: meta.experimental,
        })
        .collect();
    // Then the services this hub has that the catalogue does not: added by their image,
    // and known here by the name the image gave.
    for id in config.service_ids() {
        if id.known().is_none() {
            rows.push(Listed {
                state: state_of(id),
                id,
                name: id.as_str().to_string(),
                experimental: false,
            });
        }
    }

    if json {
        return ui::emit_json(&rows);
    }

    ui::say("");
    ui::table(
        &rows
            .iter()
            .map(|row| {
                let state = match row.state {
                    "running" => ui::bold("running"),
                    "kept" => "off — data kept".to_string(),
                    other => ui::dim(other),
                };
                let tag = if row.experimental {
                    ui::dim("  (experimental)")
                } else {
                    String::new()
                };
                (row.id.as_str().to_string(), format!("{state}{tag}"))
            })
            .collect::<Vec<_>>(),
    );
    ui::say("");
    ui::step(&ui::dim(
        "Change them with `konstruktor hub services add|remove <ids…>`; add any other \
         service by its image, with `add --image IMAGE`.",
    ));
    ui::say("");
    Ok(())
}

fn describe(plan: &ServicePlan) -> Vec<(String, String)> {
    let names = |ids: &[ServiceId]| {
        ids.iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut rows = Vec::new();
    if !plan.added.is_empty() {
        rows.push(("adding".to_string(), names(&plan.added)));
    }
    if !plan.removed.is_empty() {
        rows.push(("removing".to_string(), names(&plan.removed)));
    }
    if !plan.unchanged.is_empty() {
        rows.push(("already so".to_string(), names(&plan.unchanged)));
    }
    rows.push(("afterwards".to_string(), names(&plan.services)));
    rows
}

async fn change_services(
    args: &ChangeArgs,
    change: ServiceChange,
    described: BTreeMap<String, Description>,
) -> Result<()> {
    let target = Target::named(args.in_hub.clone());
    let dir = target.resolve()?;
    let config = profile::read_profile(&dir)?.config;

    // Refused here, before anybody is sent to a browser; the core asks again.
    let plan = services::plan(&config, &konstruktor_core::contract::known(&dir), &change)?;
    let mut answers = services::answers_from_disk(&dir, change)?;
    answers.described = described;

    ui::say("");
    ui::say(&format!("  {}", ui::bold("Changing a hub's services")));
    ui::say("");
    let mut rows = vec![
        ("folder".to_string(), dir.to_string_lossy().to_string()),
        (
            "authorized as".to_string(),
            credentials::read_credentials(&dir)
                .map(|c| format!("{} at {}", c.identifier, c.server))
                .unwrap_or_default(),
        ),
    ];
    rows.extend(describe(&plan));
    ui::table(&rows);
    ui::say("");
    for note in &plan.notes {
        ui::step(&ui::dim(note));
    }
    if !plan.notes.is_empty() {
        ui::say("");
    }

    if !args.yes && ui::is_interactive() {
        let confirmed =
            inquire::Confirm::new("Send the new set of services to the coordination server?")
                .with_default(true)
                .prompt()
                .unwrap_or(false);
        if !confirmed {
            ui::say("");
            ui::step("Left alone.");
            ui::say("");
            return Ok(());
        }
    }

    let cancel = CancellationToken::new();
    let on_signal = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            on_signal.cancel();
        }
    });

    let open_browser = !args.no_open && ui::is_interactive();
    let done = services::change_services(&answers, !args.no_apply, &cancel, &|event| {
        crate::create::report(event, open_browser)
    })
    .await?;

    ui::say("");
    ui::ok(&format!(
        "{} now runs {}.",
        done.reauthorized.credentials.identifier,
        done.plan
            .services
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    crate::create::report_outcome(
        done.reauthorized.mesh_requested,
        done.reauthorized.mesh_granted,
        done.reauthorized.reporter_enabled,
        done.applied,
    );
    if !done.applied {
        ui::step(&ui::dim(
            "The files were rewritten, the stack left alone — `konstruktor hub services \
             apply` brings it to them.",
        ));
    }
    ui::say("");
    Ok(())
}

async fn apply(target: &Target) -> Result<()> {
    let dir = target.resolve()?;
    ui::say("");
    ui::step(&format!(
        "Bringing {} to its services…",
        ui::bold(&dir.to_string_lossy())
    ));
    let print = |line: konstruktor_core::compose::ComposeLine| {
        if line.stderr {
            eprintln!("  {}", ui::dim(&line.line));
        } else {
            println!("  {}", line.line);
        }
    };
    // Which configs changed since the containers read them is not known here, so every
    // service that reads one is restarted.
    let config = profile::read_profile(&dir)?.config;
    let restart = services::every_config_reader(&config);
    let report = services::apply_services(&dir, &restart, &print).await?;
    ui::say("");
    for warning in &report.warnings {
        ui::warn(warning);
    }
    ui::ok("Done.");
    ui::say("");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        command: ServicesCommand,
    }

    fn parse(argv: &[&str]) -> Result<ServicesCommand, clap::Error> {
        let mut full = vec!["services"];
        full.extend_from_slice(argv);
        Cli::try_parse_from(full).map(|cli| cli.command)
    }

    #[test]
    fn add_takes_services_and_the_hub_by_flag() {
        let ServicesCommand::Add(args) =
            parse(&["add", "bank", "Kuvert", "--in", "MyHub", "--no-apply", "-y"]).unwrap()
        else {
            panic!("not an add");
        };
        assert_eq!(args.services, [ServiceId::Bank, ServiceId::Kuvert]);
        assert_eq!(args.in_hub.as_deref(), Some("MyHub"));
        assert!(args.no_apply && args.yes && !args.no_open);
    }

    #[test]
    fn remove_needs_at_least_one_service_with_a_name_a_service_can_have() {
        assert!(parse(&["remove"]).is_err());
        let error = parse(&["remove", "bank/ing"]).unwrap_err().to_string();
        assert!(error.contains("unknown service `bank/ing`"), "{error}");
        // What the hub runs beside its services is not one of them.
        let error = parse(&["remove", "gateway"]).unwrap_err().to_string();
        assert!(error.contains("`gateway`"), "{error}");
        // A service outside the catalogue is removed by its name like any other.
        let ServicesCommand::Remove(args) = parse(&["remove", "example"]).unwrap() else {
            panic!("not a remove");
        };
        assert_eq!(args.services, [ServiceId::named("example")]);
    }

    /// A service is added by name or by the image it is — and by image alone is enough.
    #[test]
    fn add_takes_a_service_by_its_image() {
        let ServicesCommand::Add(args) =
            parse(&["add", "--image", "example:1", "--in", "MyHub"]).unwrap()
        else {
            panic!("not an add");
        };
        assert!(args.services.is_empty());
        assert_eq!(args.images, ["example:1"]);
        assert!(parse(&["add"]).is_err());
    }

    #[test]
    fn list_and_apply_take_the_usual_target() {
        let ServicesCommand::List(target) = parse(&["list", "MyHub"]).unwrap() else {
            panic!("not a list");
        };
        assert_eq!(target.target.as_deref(), Some("MyHub"));
        assert!(
            matches!(parse(&["apply"]).unwrap(), ServicesCommand::Apply(t) if t.target.is_none())
        );
    }
}
