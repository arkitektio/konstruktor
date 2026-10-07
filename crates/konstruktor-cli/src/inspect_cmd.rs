//! `konstruktor inspect`: what a service's image says of itself.
//!
//! Everything this installer does with a service is read off that answer — what it
//! registers, what it sets up, how it starts the service and what it runs beforehand. This
//! shows it: for a service of a hub, or for any image, which is how to find out whether an
//! image is one a hub could run before putting it in one.

use anyhow::{anyhow, bail, Result};
use clap::Args;

use konstruktor_core::contract::{self, Description};
use konstruktor_core::profile;

use crate::manage::Target;
use crate::ui;

#[derive(Args, Debug, Clone)]
pub struct InspectArgs {
    /// A service of the hub, as `konstruktor ps` names it — or, with `--image`, left out.
    pub service: Option<String>,
    /// Ask this image instead of a hub's service: `--image jhnnsrs/mikro:7`. Needs no hub.
    #[arg(long, value_name = "IMAGE", conflicts_with = "service")]
    pub image: Option<String>,
    /// The deployment: a path, or a name from `konstruktor list`. Defaults to here.
    #[arg(long = "in", value_name = "TARGET", conflicts_with = "image")]
    pub in_deployment: Option<String>,
}

pub async fn run(args: InspectArgs, json: bool) -> Result<()> {
    let image = match (args.image, args.service) {
        (Some(image), _) => image,
        (None, Some(service)) => {
            let dir = Target::named(args.in_deployment).resolve()?;
            let config = profile::read_profile(&dir)?.config;
            contract::image_of(&dir, &config, &service)
                .ok_or_else(|| anyhow!("`{service}` is not a service this hub runs an image for"))?
        }
        (None, None) => bail!("name a service of the hub, or an image with --image"),
    };

    let answer = contract::answer(&image).await.ok_or_else(|| {
        anyhow!("`{image}` could not be run, or did not stop: it is not here and cannot be pulled, or it is not a service's image")
    })?;
    let Ok(said) = serde_json::from_str::<Description>(&answer) else {
        bail!(
            "`{image}` did not say what it is when run with no command: it is not a service's \
             image, or it is a release from before a service described itself"
        );
    };
    if json {
        // As the image printed it: whatever it says that this build does not read is kept.
        let whole: serde_json::Value = serde_json::from_str(&answer)?;
        return ui::emit_json(&whole);
    }

    let words = |parts: &[String]| parts.join(" ");
    let listed = |parts: &[String]| {
        if parts.is_empty() {
            "none".to_string()
        } else {
            parts.join(", ")
        }
    };
    ui::say("");
    ui::say(&format!("  {}  {}", ui::bold(&said.name), ui::dim(&image)));
    if !said.summary.is_empty() {
        ui::say(&format!("  {}", said.summary));
    }
    ui::say("");
    let mut rows: Vec<(String, String)> = vec![
        ("registered as".into(), said.identifier.clone()),
        ("contract".into(), said.contract.to_string()),
        ("databases".into(), listed(&said.needs.databases)),
        (
            "wired to".into(),
            listed(
                &[
                    (said.needs.redis, "the redis"),
                    (said.needs.admin, "the operator account"),
                ]
                .iter()
                .filter(|(needed, _)| *needed)
                .map(|(_, what)| what.to_string())
                .collect::<Vec<_>>(),
            ),
        ),
        ("storage".into(), listed(&said.needs.storage)),
        ("secrets".into(), listed(&said.needs.secrets)),
        ("beside it".into(), listed(&said.needs.peers)),
        (
            "a key of its own".into(),
            if said.needs.instance_key { "yes" } else { "no" }.into(),
        ),
        (
            "scopes".into(),
            listed(
                &said
                    .needs
                    .scopes
                    .iter()
                    .map(|s| s.key.clone())
                    .collect::<Vec<_>>(),
            ),
        ),
        (
            "roles".into(),
            listed(
                &said
                    .needs
                    .roles
                    .iter()
                    .map(|s| s.key.clone())
                    .collect::<Vec<_>>(),
            ),
        ),
        ("health".into(), said.offers.health.clone()),
    ];
    for (kind, path) in &said.offers.endpoints {
        rows.push((format!("offers {kind}"), path.clone()));
    }
    rows.push(("started with".into(), words(&said.serve)));
    rows.push(("with --debug".into(), words(&said.debug)));
    rows.push(("config written by".into(), words(&said.render)));
    rows.push((
        "prepared by".into(),
        match (&said.prepare, said.preparation()) {
            (Some(job), Some(command)) => format!("{job}  ({})", words(command)),
            _ => "nothing to prepare".into(),
        },
    ));
    for sidecar in &said.sidecars {
        // The image it would actually run: the description names it relative to the
        // service's own (`{repository}-takt:{tag}`), so it is resolved against the image
        // that was asked — the hub's own build of the service, or the one `--image` named.
        rows.push((
            format!("sidecar {}", sidecar.name),
            format!(
                "{}{}",
                sidecar.image_beside(&image),
                if sidecar.optional { "  (optional)" } else { "" }
            ),
        ));
    }
    for (peer, versions) in &said.requires {
        rows.push((format!("needs {peer}"), versions.clone()));
    }
    if let Some(oldest) = &said.upgrade_from {
        rows.push(("upgradable from".into(), oldest.clone()));
    }
    // Where its code came from: what `--from-source` clones, and where it mounts it.
    rows.push((
        "source".into(),
        match &said.source {
            Some(source) => format!(
                "{}{}  (in the image at {})",
                source.repository,
                source
                    .revision
                    .as_deref()
                    .map(|revision| format!(" at {revision}"))
                    .unwrap_or_default(),
                source.path
            ),
            None => "not said: `--from-source` has to name one".into(),
        },
    ));
    // What exists on a hub because it is there: how many, and which.
    let counted = |count: usize, what: &str, identifiers: Vec<&str>| {
        if count == 0 {
            "none".to_string()
        } else {
            format!(
                "{count} {what}{}: {}",
                if count == 1 { "" } else { "s" },
                identifiers.join(", ")
            )
        }
    };
    rows.push((
        "hosts".into(),
        counted(
            said.hosts.structures.len(),
            "structure",
            said.hosts
                .structures
                .iter()
                .map(|structure| structure.identifier.as_str())
                .collect(),
        ),
    ));
    rows.push((
        "announces".into(),
        counted(
            said.hosts.signals.len(),
            "signal",
            said.hosts
                .signals
                .iter()
                .map(|signal| signal.identifier.as_str())
                .collect(),
        ),
    ));
    ui::table(&rows);

    ui::say("");
    ui::say(&format!("  {}", ui::bold("jobs")));
    if said.jobs.is_empty() {
        ui::say(&format!("  {}", ui::dim("none")));
    }
    let jobs: Vec<(String, String)> = said
        .jobs
        .iter()
        .map(|(name, job)| {
            let mut about = job.summary.clone();
            if !job.includes.is_empty() {
                about.push_str(&format!(" Runs: {}.", job.includes.join(", ")));
            }
            (name.clone(), about.trim().to_string())
        })
        .collect();
    ui::table(&jobs);
    ui::say("");
    ui::say(&format!(
        "  {}",
        ui::dim("--json prints the whole description, as the image printed it.")
    ));
    ui::say("");
    Ok(())
}
