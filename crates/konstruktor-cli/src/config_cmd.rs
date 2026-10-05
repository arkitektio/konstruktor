//! `konstruktor config`: what an operator set for one hub's services.
//!
//! The generated configs are written again on every update; this is where a setting goes
//! that has to outlive that. See `konstruktor_core::overrides`.

use anyhow::{anyhow, bail, Result};
use clap::{Args, Subcommand};

use konstruktor_core::updates::Reads;
use konstruktor_core::{migrate, overrides, profile, updates};

use crate::manage::Target;
use crate::ui;

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigCommand {
    /// Print what was set for a service, beyond what is generated.
    Show(ShowArgs),
    /// Set a value in a service's config that every update keeps.
    ///
    /// The key is dotted (`datalayer.quotas.default`), the value YAML (`10`, `true`,
    /// `[admin, uploader]`). The service's release is asked whether it reads the result;
    /// if it does not, nothing is kept.
    Set(SetArgs),
    /// Take a value back out: the generated one applies again.
    Unset(UnsetArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ShowArgs {
    /// The service, as `konstruktor ps` names it.
    pub service: String,
    /// The deployment: a path, or a name from `konstruktor list`. Defaults to here.
    #[arg(long = "in", value_name = "TARGET")]
    pub in_deployment: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct SetArgs {
    pub service: String,
    pub key: String,
    pub value: String,
    #[arg(long = "in", value_name = "TARGET")]
    pub in_deployment: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct UnsetArgs {
    pub service: String,
    pub key: String,
    #[arg(long = "in", value_name = "TARGET")]
    pub in_deployment: Option<String>,
}

pub async fn run(command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Show(args) => show(args),
        ConfigCommand::Set(args) => {
            let change = |dir: &std::path::Path| {
                overrides::set(dir, &args.service, &args.key, &args.value).map(|()| true)
            };
            changed(args.in_deployment.clone(), &args.service, &change).await
        }
        ConfigCommand::Unset(args) => {
            let change = |dir: &std::path::Path| overrides::unset(dir, &args.service, &args.key);
            changed(args.in_deployment.clone(), &args.service, &change).await
        }
    }
}

/// The hub's folder, once `service` is known to be one of its services.
fn hub_with(target: Option<String>, service: &str) -> Result<std::path::PathBuf> {
    let dir = Target::named(target).resolve()?;
    let config = profile::read_profile(&dir)?.config;
    let runs = config
        .enabled_services()
        .into_iter()
        .any(|id| config.service(id).host == service);
    if !runs {
        bail!("this hub runs no service called `{service}`");
    }
    Ok(dir)
}

fn show(args: ShowArgs) -> Result<()> {
    let dir = hub_with(args.in_deployment, &args.service)?;
    match std::fs::read_to_string(overrides::path(&dir, &args.service)) {
        Ok(text) => print!("{text}"),
        Err(_) => ui::step(&format!(
            "Nothing is set for {}: it runs on what is generated.",
            args.service
        )),
    }
    Ok(())
}

/// Applies a change to a service's overrides, writes the hub's files with it, and asks the
/// service's release whether it reads the result — putting everything back if not.
async fn changed(
    target: Option<String>,
    service: &str,
    change: &dyn Fn(&std::path::Path) -> std::io::Result<bool>,
) -> Result<()> {
    let dir = hub_with(target, service)?;
    let file = overrides::path(&dir, service);
    let before = std::fs::read(&file).ok();
    if !change(&dir)? {
        ui::step("That was not set.");
        return Ok(());
    }

    let config = profile::read_profile(&dir)?.config;
    // A hub whose files are an update's to rewrite keeps the setting for that update.
    if migrate::behind(&dir, &config).is_some() {
        ui::ok("Kept. This hub's files are rewritten by its next `konstruktor update`, with this in them.");
        return Ok(());
    }
    profile::rewrite(&dir, config.clone(), &[])?;
    if let Reads::No(said) = updates::reads_its_config(&dir, service).await {
        match &before {
            Some(bytes) => std::fs::write(&file, bytes)?,
            None => {
                let _ = std::fs::remove_file(&file);
            }
        }
        profile::rewrite(&dir, config, &[])?;
        return Err(anyhow!(
            "`{service}` does not read that, so nothing was kept. It said:\n{said}"
        ));
    }
    ui::ok(&format!(
        "Written into {service}'s config, and kept by every update."
    ));
    ui::step(&ui::dim(&format!(
        "`konstruktor restart {service}` makes the running service read it."
    )));
    Ok(())
}
