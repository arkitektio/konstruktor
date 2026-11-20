//! `konstruktor hub features`: what is switched on for a hub as a whole.
//!
//! Everything is `konstruktor_core::features`. A feature changes the hub's profile and the
//! files generated from it, and nobody has to accept it: nothing here goes to a
//! coordination server. The stack is brought to the files by the same start `up` runs,
//! unless `--no-apply` or the hub is not running.

use anyhow::Result;
use clap::{Args, Subcommand};
use konstruktor_core::features::{self, Asked, Feature, FeatureChange, FeatureView, Source};
use konstruktor_core::profile;
use serde::Serialize;

use crate::manage::Target;
use crate::ui;

#[derive(Subcommand, Debug, Clone)]
pub enum FeaturesCommand {
    /// List the features a hub can have, and which it has.
    List(Target),
    /// Switch a feature on, or point it somewhere else.
    ///
    /// Run here, in the hub's own stack, unless `--url` names one that already exists:
    /// `hub features add telemetry` runs a Jaeger beside the services and has every one of
    /// them send its traces there; `--url http://collector:4318` has them send to yours.
    Add(AddArgs),
    /// Switch a feature off. What it ran is removed; what it stored in a volume is kept.
    Remove(RemoveArgs),
}

#[derive(Args, Debug, Clone)]
pub struct AddArgs {
    /// The feature: `konstruktor hub features list` shows them.
    #[arg(value_parser = Feature::parse)]
    pub feature: Feature,
    /// The address of one that already exists. Left out, one is run in this stack.
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    /// For telemetry run here: the port of this machine its page is opened on. It is
    /// published on the loopback address only — the page shows every operation of the hub
    /// and asks nobody who they are. Left out, 16686.
    #[arg(long, value_name = "PORT", conflicts_with = "no_ui")]
    pub ui_port: Option<u16>,
    /// For telemetry run here: publish no page on this machine at all.
    #[arg(long)]
    pub no_ui: bool,
    #[command(flatten)]
    pub common: Common,
}

#[derive(Args, Debug, Clone)]
pub struct RemoveArgs {
    #[arg(value_parser = Feature::parse)]
    pub feature: Feature,
    #[command(flatten)]
    pub common: Common,
}

#[derive(Args, Debug, Clone)]
pub struct Common {
    /// The hub: a path, or a name from `konstruktor list`. Defaults to here.
    #[arg(long = "in", value_name = "TARGET")]
    pub in_hub: Option<String>,
    /// Write the files, but leave the running stack alone — `konstruktor up` brings it to
    /// them later.
    #[arg(long)]
    pub no_apply: bool,
}

pub async fn run(command: FeaturesCommand, json: bool) -> Result<()> {
    match command {
        FeaturesCommand::List(target) => list(&target, json),
        FeaturesCommand::Add(args) => {
            let dir = Target::named(args.common.in_hub.clone()).resolve_hub()?;
            let asked = Asked {
                url: args.url.clone(),
                ui_port: args.ui_port,
                no_ui: args.no_ui,
            };
            let change = features::add(&dir, args.feature, &asked)?;
            changed(&dir, change, &args.common, json).await
        }
        FeaturesCommand::Remove(args) => {
            let dir = Target::named(args.common.in_hub.clone()).resolve_hub()?;
            let change = features::remove(&dir, args.feature)?;
            changed(&dir, change, &args.common, json).await
        }
    }
}

/// How one feature reads in a table: off, here, or where.
fn state(view: &FeatureView) -> String {
    match (view.source, &view.address) {
        (Source::Off, _) => ui::dim("off"),
        (Source::Local, _) => ui::bold("on — run here"),
        (Source::Remote, Some(address)) => format!("{} — at {address}", ui::bold("on")),
        (Source::Remote, None) => ui::bold("on"),
    }
}

fn list(target: &Target, json: bool) -> Result<()> {
    let dir = target.resolve_hub()?;
    let config = profile::read_profile(&dir)?.config;
    let views = features::views(&config);
    if json {
        return ui::emit_json(&views);
    }

    ui::say("");
    ui::table(
        &views
            .iter()
            .map(|view| (view.feature.as_str().to_string(), state(view)))
            .collect::<Vec<_>>(),
    );
    ui::say("");
    for view in &views {
        ui::step(&format!(
            "{}: {}",
            ui::bold(view.feature.as_str()),
            ui::dim(view.summary)
        ));
        if let Some(page) = &view.page {
            ui::step(&ui::dim(&format!("  its page, on this machine: {page}")));
        }
        if let Some(note) = &view.note {
            ui::step(&ui::dim(&format!("  {note}")));
        }
    }
    ui::say("");
    ui::step(&ui::dim(
        "Change them with `konstruktor hub features add|remove <feature>`; `add --url URL` \
         uses one that already exists.",
    ));
    ui::say("");
    Ok(())
}

/// What a change answers with, as a document.
#[derive(Serialize)]
struct Changed<'a> {
    #[serde(flatten)]
    change: &'a FeatureChange,
    /// Whether the running stack was brought to it.
    applied: bool,
    /// For telemetry: which services send, and which do not yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    reporting: Option<features::Reporting>,
}

/// Says what a change did, and brings a running hub to it.
async fn changed(
    dir: &std::path::Path,
    change: FeatureChange,
    common: &Common,
    json: bool,
) -> Result<()> {
    let feature = change.view.feature;
    let say = |text: &str| {
        if !json {
            ui::say(text);
        }
    };

    if !change.changed {
        if json {
            return ui::emit_json(&Changed {
                change: &change,
                applied: false,
                reporting: None,
            });
        }
        ui::say("");
        ui::step(&format!(
            "{feature} is already so: {}.",
            state(&change.view)
        ));
        ui::say("");
        return Ok(());
    }

    say("");
    if !json {
        match change.view.source {
            Source::Off => ui::ok(&format!("{feature} is off for this hub.")),
            Source::Local => ui::ok(&format!(
                "{feature} is on: this hub runs {} of its own.",
                feature.runs()
            )),
            Source::Remote => ui::ok(&format!(
                "{feature} is on, at {}.",
                change.view.address.clone().unwrap_or_default()
            )),
        }
        if let Some(note) = &change.view.note {
            ui::step(&ui::dim(note));
        }
    }

    // A hub that is down has nothing to bring anywhere: its next start reads the files.
    let apply = !common.no_apply && features::is_up(dir).await;
    if apply {
        say("");
        let print = |line: konstruktor_core::compose::ComposeLine| {
            eprintln!("  {}", ui::dim(&line.line));
        };
        let report = features::apply(dir, &change, &print).await?;
        if !json {
            for warning in &report.warnings {
                ui::warn(warning);
            }
        }
    } else if !json {
        ui::step(&ui::dim(
            "Nothing running has changed: `konstruktor up` brings the hub to it.",
        ));
    }

    // Which services took it, read off the configs their own images just wrote.
    let reporting = (apply && feature == Feature::Telemetry && change.view.source != Source::Off)
        .then(|| profile::read_profile(dir).map(|p| features::reporting(dir, &p.config)))
        .transpose()?;
    if json {
        return ui::emit_json(&Changed {
            change: &change,
            applied: apply,
            reporting,
        });
    }
    if let Some(reporting) = &reporting {
        ui::say("");
        if !reporting.sending.is_empty() {
            ui::step(&format!("sending: {}", reporting.sending.join(", ")));
        }
        if !reporting.silent.is_empty() {
            ui::warn(&format!(
                "not sending: {} — their releases are from before services read the hub's \
                 collector. `konstruktor update` moves them to ones that do.",
                reporting.silent.join(", ")
            ));
        }
    }
    if let Some(page) = change.view.page.as_ref().filter(|_| apply) {
        ui::step(&format!("Its page, on this machine: {page}"));
    }
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
        command: FeaturesCommand,
    }

    fn parse(argv: &[&str]) -> Result<FeaturesCommand, clap::Error> {
        let mut full = vec!["features"];
        full.extend(argv);
        Cli::try_parse_from(full).map(|cli| cli.command)
    }

    #[test]
    fn a_feature_is_added_here_or_from_an_address() {
        let FeaturesCommand::Add(here) = parse(&["add", "telemetry"]).unwrap() else {
            panic!("add")
        };
        assert_eq!(here.feature, Feature::Telemetry);
        assert!(here.url.is_none());

        let FeaturesCommand::Add(there) = parse(&[
            "add",
            "ollama",
            "--url",
            "http://gpu-box:11434",
            "--in",
            "lab",
        ])
        .unwrap() else {
            panic!("add")
        };
        assert_eq!(there.feature, Feature::Ollama);
        assert_eq!(there.url.as_deref(), Some("http://gpu-box:11434"));
        assert_eq!(there.common.in_hub.as_deref(), Some("lab"));
    }

    #[test]
    fn an_unknown_feature_is_refused_with_the_ones_there_are() {
        let refused = parse(&["add", "jaeger"]).unwrap_err().to_string();
        assert!(refused.contains("telemetry, ollama"), "{refused}");
    }

    #[test]
    fn a_page_is_moved_or_left_out_but_not_both() {
        assert!(parse(&["add", "telemetry", "--ui-port", "17000"]).is_ok());
        assert!(parse(&["add", "telemetry", "--no-ui"]).is_ok());
        assert!(parse(&["add", "telemetry", "--ui-port", "17000", "--no-ui"]).is_err());
    }
}
