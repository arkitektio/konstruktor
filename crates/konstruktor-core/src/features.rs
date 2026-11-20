//! What a hub's operator switches on for the hub as a whole, after it was created or with it.
//!
//! A feature is a capability of the hub and not of one service's settings: it changes what
//! several services are told, and may bring a container of its own that is no service of
//! the hub — it has no contract, no database and nothing to authorize. A setting that only
//! one service reads is `konstruktor config`'s; a service is `hub services`'.
//!
//! Each feature is `local` (run here, in this stack) or the address of one that already
//! exists, the same two answers `hub create` takes for it. Nothing here talks to a
//! coordination server: a feature changes the profile and the files generated from it, and
//! the stack is brought to them by the same start every other change uses.

use std::path::Path;

use serde::Serialize;

use crate::catalog::ServiceId;
use crate::config::hub::{HubConfig, Kinded, OllamaBlock, TelemetryBlock};
use crate::profile::ProfileError;

/// The features this build knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Feature {
    /// Traces and metrics of every service, sent to a collector the operator chose.
    Telemetry,
    /// The model server Alpaka's language models run on.
    Ollama,
}

pub const FEATURES: [Feature; 2] = [Feature::Telemetry, Feature::Ollama];

impl Feature {
    pub fn as_str(self) -> &'static str {
        match self {
            Feature::Telemetry => "telemetry",
            Feature::Ollama => "ollama",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        let name = value.trim().to_ascii_lowercase();
        FEATURES
            .into_iter()
            .find(|feature| feature.as_str() == name)
            .ok_or_else(|| {
                format!(
                    "unknown feature `{value}` — the ones this konstruktor knows are {}",
                    FEATURES.map(Feature::as_str).join(", ")
                )
            })
    }

    /// One line on what it is, for a list.
    pub fn summary(self) -> &'static str {
        match self {
            Feature::Telemetry => {
                "Traces of every service, to a collector on this hub or one you name. \
                 Nothing leaves the hub otherwise."
            }
            Feature::Ollama => "The model server Alpaka's language models run on.",
        }
    }

    /// What `local` runs, for the lines that describe a change.
    pub fn runs(self) -> &'static str {
        match self {
            Feature::Telemetry => "a Jaeger",
            Feature::Ollama => "an Ollama",
        }
    }
}

impl std::fmt::Display for Feature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a feature comes from on one hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Off,
    /// A container in this stack.
    Local,
    /// One that exists elsewhere.
    Remote,
}

/// One feature, as a hub has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FeatureView {
    pub feature: Feature,
    pub summary: &'static str,
    pub source: Source,
    /// What the services are pointed at, when it is on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// A page a person opens, on this machine, when the feature has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    /// What stands between the profile and a running feature, if anything.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Every feature this build knows, as the hub of `config` has it.
pub fn views(config: &HubConfig) -> Vec<FeatureView> {
    FEATURES
        .into_iter()
        .map(|feature| view(config, feature))
        .collect()
}

pub fn view(config: &HubConfig, feature: Feature) -> FeatureView {
    let source = |local: bool| if local { Source::Local } else { Source::Remote };
    let mut view = FeatureView {
        feature,
        summary: feature.summary(),
        source: Source::Off,
        address: None,
        page: None,
        note: None,
    };
    match feature {
        Feature::Telemetry => {
            if let Some(telemetry) = &config.telemetry {
                view.source = source(telemetry.enabled);
                view.address = Some(telemetry.endpoint.clone());
                view.page = telemetry.ui_url();
                if !telemetry.takes_metrics() {
                    view.note = Some(
                        "Traces only: the Jaeger run here takes no metrics. A collector of \
                         your own takes both."
                            .to_string(),
                    );
                }
            }
        }
        Feature::Ollama => {
            if let Some(ollama) = &config.local_ollama {
                view.source = source(ollama.enabled);
                view.address = Some(ollama.url.clone());
                if !config.service(ServiceId::Alpaka).enabled {
                    view.note = Some(
                        "Kept for when Alpaka runs here again: nothing uses it now, and its \
                         container is not started."
                            .to_string(),
                    );
                }
            }
        }
    }
    view
}

#[derive(Debug, thiserror::Error)]
pub enum FeatureError {
    #[error(transparent)]
    Profile(#[from] ProfileError),
    /// The hub cannot have the feature as asked. Nothing was written.
    #[error("{0}")]
    Refused(String),
}

/// What a change did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FeatureChange {
    /// False when the profile already said exactly this: nothing was written.
    pub changed: bool,
    pub view: FeatureView,
    /// The compose services whose config this installer writes itself and wrote
    /// differently — Lok, on a hub that runs its own. A start does not see those change,
    /// so [`apply`] restarts them; the services proper are restarted by the start, which
    /// has their images write their configs.
    #[serde(skip)]
    pub rewritten: Vec<String>,
}

/// What `add` is asked for beside the feature.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Asked {
    /// The address of one that exists. Left out, one is run in this stack.
    pub url: Option<String>,
    /// For a local telemetry collector: the port of this machine its page is published on.
    pub ui_port: Option<u16>,
    /// For a local telemetry collector: publish no page at all.
    pub no_ui: bool,
}

/// Puts `feature` into `config` as asked: the profile change alone, with nothing written.
pub fn switch_on(
    config: &mut HubConfig,
    feature: Feature,
    asked: &Asked,
) -> Result<(), FeatureError> {
    let url = asked
        .url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty() && *url != TelemetryBlock::LOCAL);
    match feature {
        Feature::Telemetry => {
            let mut block = match url {
                Some(url) => TelemetryBlock::remote(url),
                None => TelemetryBlock::local(),
            };
            if block.enabled {
                // The image somebody pinned for it stays: only where it sends changes.
                if let Some(held) = config.telemetry.as_ref().filter(|held| held.enabled) {
                    block.image = held.image.clone();
                    block.exposed_ui_port = held.exposed_ui_port;
                }
                if let Some(port) = asked.ui_port {
                    block.exposed_ui_port = Some(port);
                }
                if asked.no_ui {
                    block.exposed_ui_port = None;
                }
            } else if asked.ui_port.is_some() || asked.no_ui {
                return Err(FeatureError::Refused(
                    "a collector that already exists has its own page: there is none here \
                     to publish or to leave out"
                        .to_string(),
                ));
            }
            config.telemetry = Some(block);
        }
        Feature::Ollama => {
            if !config.service(ServiceId::Alpaka).enabled {
                return Err(FeatureError::Refused(
                    "Ollama is Alpaka's model server, and this hub does not run Alpaka: \
                     `konstruktor hub services add alpaka` first"
                        .to_string(),
                ));
            }
            let mut block = match url {
                Some(url) => OllamaBlock::remote(url),
                None => OllamaBlock::local(),
            };
            if let Some(held) = config.local_ollama.as_ref().filter(|held| held.enabled) {
                if block.enabled {
                    block.image = held.image.clone();
                }
            }
            let kind = if block.enabled {
                Kinded::local()
            } else {
                Kinded::global()
            };
            config.local_ollama = Some(block);
            if let Some(alpaka) = config.services.get_mut(&ServiceId::Alpaka) {
                alpaka.ollama_config = Some(kind);
            }
        }
    }
    Ok(())
}

/// Takes `feature` out of `config`: the profile change alone.
pub fn switch_off(config: &mut HubConfig, feature: Feature) {
    match feature {
        Feature::Telemetry => config.telemetry = None,
        Feature::Ollama => {
            config.local_ollama = None;
            // What a hub that was never asked says: a local provider nothing starts.
            if let Some(alpaka) = config.services.get_mut(&ServiceId::Alpaka) {
                alpaka.ollama_config = Some(Kinded::local());
            }
        }
    }
}

/// Applies `change` to the profile of the hub in `dir` and writes its generated files from
/// the result. A hub whose files are of an older layout is refused, as everywhere else:
/// those are an update's to rewrite.
fn rewrite(
    dir: &Path,
    feature: Feature,
    change: impl FnOnce(&mut HubConfig) -> Result<(), FeatureError>,
) -> Result<FeatureChange, FeatureError> {
    let before = crate::profile::read_profile(dir)?.config;
    if let Some(reason) = crate::migrate::behind(dir, &before) {
        return Err(ProfileError::Layout(reason).into());
    }
    let mut config = before.clone();
    change(&mut config)?;
    let changed = config != before;
    let view = view(&config, feature);
    let mut rewritten = Vec::new();
    if changed {
        let configs = crate::services::snapshot_configs(dir);
        let lok = config.running_lok().map(|lok| lok.host.clone());
        crate::profile::rewrite(dir, config, &[])?;
        let differ =
            crate::services::changed_configs(&configs, &crate::services::snapshot_configs(dir));
        rewritten.extend(lok.filter(|host| differ.contains(&format!("{host}.yaml"))));
    }
    Ok(FeatureChange {
        changed,
        view,
        rewritten,
    })
}

/// Brings the running hub in `dir` to a change: the start every other change uses — new
/// containers come, removed ones go, each service's image writes its config again and
/// the ones that changed are restarted — and then what the start cannot see
/// ([`FeatureChange::rewritten`]).
pub async fn apply(
    dir: &Path,
    change: &FeatureChange,
    on_line: &(dyn Fn(crate::compose::ComposeLine) + Send + Sync),
) -> Result<crate::start::StartReport, crate::start::StartError> {
    let report = crate::start::start_removing_orphans(dir, on_line).await?;
    let restart: Vec<String> = change
        .rewritten
        .iter()
        .filter(|name| crate::compose_file::declares_service(dir, name))
        .cloned()
        .collect();
    if !restart.is_empty() {
        let mut args = vec!["compose".to_string(), "restart".to_string()];
        args.extend(restart);
        crate::compose::run_streamed(dir, args, on_line)
            .await
            .map_err(crate::start::StartError::Compose)?;
    }
    Ok(report)
}

/// Switches `feature` on for the hub in `dir`, or moves it to where `asked` says. The
/// stack is not touched: [`crate::start::start_removing_orphans`] brings it to the files.
pub fn add(dir: &Path, feature: Feature, asked: &Asked) -> Result<FeatureChange, FeatureError> {
    rewrite(dir, feature, |config| {
        switch_on(config, feature, asked)?;
        // A collector that is new here, with nothing said of its page: the usual port if
        // this machine has it free, another if not — a second hub, or a Jaeger of the
        // operator's own, already has it.
        let new_here = config.running_telemetry().is_some()
            && crate::profile::read_profile(dir)
                .map(|held| held.config.running_telemetry().is_none())
                .unwrap_or(true);
        if feature == Feature::Telemetry && new_here && asked.ui_port.is_none() && !asked.no_ui {
            if let Some(telemetry) = config.telemetry.as_mut() {
                telemetry.exposed_ui_port = Some(free_page_port(telemetry.ui_port));
            }
        }
        Ok(())
    })
}

/// `wanted` if nothing on this machine's loopback address listens there, else a port
/// nothing does. What it finds can be taken again before the container starts; compose
/// then says so, and `--ui-port` names another.
pub fn free_page_port(wanted: u16) -> u16 {
    let free = |port: u16| {
        std::net::TcpListener::bind(("127.0.0.1", port))
            .and_then(|listener| listener.local_addr())
            .map(|address| address.port())
    };
    free(wanted).or_else(|_| free(0)).unwrap_or(wanted)
}

/// Switches `feature` off for the hub in `dir`.
pub fn remove(dir: &Path, feature: Feature) -> Result<FeatureChange, FeatureError> {
    rewrite(dir, feature, |config| {
        switch_off(config, feature);
        Ok(())
    })
}

/// Which of the hub's services are configured to send telemetry, by compose service, and
/// which are not — read off the configs their own images wrote, so it is what they will do
/// and not what was hoped. A release from before services read the hub's collector from
/// their facts is among the second, until it is updated.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Reporting {
    pub sending: Vec<String>,
    pub silent: Vec<String>,
}

pub fn reporting(dir: &Path, config: &HubConfig) -> Reporting {
    let mut out = Reporting::default();
    let hosts = config
        .running_lok()
        .map(|lok| lok.host.clone())
        .into_iter()
        .chain(
            config
                .enabled_services()
                .into_iter()
                .map(|id| config.service(id).host.clone()),
        );
    for host in hosts {
        let sends = std::fs::read_to_string(dir.join("configs").join(format!("{host}.yaml")))
            .ok()
            .and_then(|text| serde_norway::from_str::<serde_norway::Value>(&text).ok())
            .and_then(|document| {
                document
                    .get("telemetry")
                    .and_then(|telemetry| telemetry.get("endpoint"))
                    .and_then(|endpoint| endpoint.as_str().map(|e| !e.trim().is_empty()))
            })
            .unwrap_or(false);
        if sends {
            out.sending.push(host);
        } else {
            out.silent.push(host);
        }
    }
    out
}

/// Whether any container of the deployment in `dir` runs: a change is applied to a hub
/// that is up, and left for the next start of one that is not.
pub async fn is_up(dir: &Path) -> bool {
    crate::engine_probe::engine()
        .async_command()
        .args(["compose", "ps", "--status", "running", "--services"])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map(|out| !String::from_utf8_lossy(&out.stdout).trim().is_empty())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::hub::{build_hub_config, HubConfigOptions};

    fn hub(services: Vec<ServiceId>) -> HubConfig {
        build_hub_config(&HubConfigOptions {
            services: Some(services),
            ..Default::default()
        })
    }

    fn peers(config: &HubConfig, id: ServiceId) -> serde_norway::Value {
        crate::contract::facts(config, id, &Default::default(), &Default::default())
            .get("peers")
            .cloned()
            .expect("facts name the peers")
    }

    fn compose(config: &HubConfig) -> serde_norway::Value {
        crate::generate::compose::build_compose(
            config,
            &config.enabled_services(),
            &Default::default(),
        )
    }

    #[test]
    fn a_hub_nobody_asked_sends_nothing() {
        let config = hub(vec![ServiceId::Mikro]);
        assert!(config.telemetry.is_none());
        assert!(peers(&config, ServiceId::Mikro).get("telemetry").is_none());
        assert_eq!(view(&config, Feature::Telemetry).source, Source::Off);
    }

    #[test]
    fn local_telemetry_runs_a_collector_and_tells_every_service() {
        let mut config = hub(vec![ServiceId::Mikro, ServiceId::Kraph]);
        switch_on(&mut config, Feature::Telemetry, &Asked::default()).unwrap();

        let services = compose(&config);
        let jaeger = services
            .get("services")
            .and_then(|s| s.get("jaeger"))
            .expect("the collector is a container of the stack");
        // Its page on the loopback address only.
        let ports = jaeger.get("ports").and_then(|p| p.as_sequence()).unwrap();
        assert_eq!(ports[0].as_str(), Some("127.0.0.1:16686:16686"));

        for id in [ServiceId::Mikro, ServiceId::Kraph] {
            let told = peers(&config, id);
            let peer = told.get("telemetry").expect("each service is told");
            assert_eq!(
                peer.get("offers")
                    .and_then(|o| o.get("otlp"))
                    .and_then(|v| v.as_str()),
                Some("http://jaeger:4318")
            );
            // The Jaeger run here takes traces only.
            assert_eq!(
                peer.get("settings")
                    .and_then(|s| s.get("metrics"))
                    .and_then(|v| v.as_str()),
                Some("false")
            );
        }
        let seen = view(&config, Feature::Telemetry);
        assert_eq!(seen.source, Source::Local);
        assert_eq!(seen.page.as_deref(), Some("http://localhost:16686"));
    }

    #[test]
    fn a_collector_of_ones_own_adds_no_container() {
        let mut config = hub(vec![ServiceId::Mikro]);
        let asked = Asked {
            url: Some("collector.lab:4318/".into()),
            ..Default::default()
        };
        switch_on(&mut config, Feature::Telemetry, &asked).unwrap();

        assert!(compose(&config)
            .get("services")
            .and_then(|s| s.get("jaeger"))
            .is_none());
        let told = peers(&config, ServiceId::Mikro);
        let peer = told.get("telemetry").unwrap();
        assert_eq!(
            peer.get("url").and_then(|v| v.as_str()),
            Some("http://collector.lab:4318")
        );
        // Taken to accept metrics too: nothing switches them off.
        assert!(peer.get("settings").is_none());
        assert_eq!(view(&config, Feature::Telemetry).source, Source::Remote);

        // A page of somebody else's collector is not ours to publish.
        let refused = switch_on(
            &mut config,
            Feature::Telemetry,
            &Asked {
                url: Some("collector.lab:4318".into()),
                ui_port: Some(9999),
                ..Default::default()
            },
        );
        assert!(matches!(refused, Err(FeatureError::Refused(_))));
    }

    #[test]
    fn the_page_moves_or_goes_and_a_pinned_image_stays() {
        let mut config = hub(vec![ServiceId::Mikro]);
        switch_on(&mut config, Feature::Telemetry, &Asked::default()).unwrap();
        config.set_service_image("jaeger", "jaegertracing/jaeger:pinned");

        let moved = Asked {
            ui_port: Some(17000),
            ..Default::default()
        };
        switch_on(&mut config, Feature::Telemetry, &moved).unwrap();
        let block = config.telemetry.clone().unwrap();
        assert_eq!(block.exposed_ui_port, Some(17000));
        assert_eq!(block.image, "jaegertracing/jaeger:pinned");
        // Asked again with nothing said, the port it has is kept.
        switch_on(&mut config, Feature::Telemetry, &Asked::default()).unwrap();
        assert_eq!(
            config.telemetry.as_ref().unwrap().exposed_ui_port,
            Some(17000)
        );

        let hidden = Asked {
            no_ui: true,
            ..Default::default()
        };
        switch_on(&mut config, Feature::Telemetry, &hidden).unwrap();
        let services = compose(&config);
        let jaeger = services
            .get("services")
            .and_then(|s| s.get("jaeger"))
            .unwrap();
        assert!(jaeger.get("ports").is_none());
    }

    #[test]
    fn taking_telemetry_out_leaves_no_trace_in_the_profile() {
        let untouched = hub(vec![ServiceId::Mikro]);
        let mut config = untouched.clone();
        switch_on(&mut config, Feature::Telemetry, &Asked::default()).unwrap();
        switch_off(&mut config, Feature::Telemetry);
        assert_eq!(config, untouched);
    }

    #[test]
    fn the_collector_is_part_of_the_stack_its_image_can_be_written_back() {
        let mut config = hub(vec![ServiceId::Mikro]);
        switch_on(&mut config, Feature::Telemetry, &Asked::default()).unwrap();
        assert!(config
            .stack_images()
            .iter()
            .any(|(service, image)| service == "jaeger" && image.starts_with("jaegertracing/")));
    }

    #[test]
    fn ollama_is_alpakas_and_a_hub_without_alpaka_is_refused() {
        let mut without = hub(vec![ServiceId::Mikro]);
        assert!(matches!(
            switch_on(&mut without, Feature::Ollama, &Asked::default()),
            Err(FeatureError::Refused(_))
        ));

        let mut config = hub(vec![ServiceId::Alpaka]);
        switch_on(&mut config, Feature::Ollama, &Asked::default()).unwrap();
        assert!(compose(&config)
            .get("services")
            .and_then(|s| s.get("ollama"))
            .is_some());
        assert_eq!(
            peers(&config, ServiceId::Alpaka)
                .get("ollama")
                .and_then(|p| p.get("url"))
                .and_then(|v| v.as_str()),
            Some("http://ollama:11434")
        );

        let elsewhere = Asked {
            url: Some("gpu-box:11434".into()),
            ..Default::default()
        };
        switch_on(&mut config, Feature::Ollama, &elsewhere).unwrap();
        assert!(compose(&config)
            .get("services")
            .and_then(|s| s.get("ollama"))
            .is_none());
        assert_eq!(view(&config, Feature::Ollama).source, Source::Remote);
        assert_eq!(
            view(&config, Feature::Ollama).address.as_deref(),
            Some("http://gpu-box:11434")
        );

        switch_off(&mut config, Feature::Ollama);
        assert_eq!(view(&config, Feature::Ollama).source, Source::Off);
        assert!(peers(&config, ServiceId::Alpaka).get("ollama").is_none());
    }

    #[test]
    fn a_feature_is_named_as_it_is_listed() {
        for feature in FEATURES {
            assert_eq!(Feature::parse(feature.as_str()), Ok(feature));
        }
        assert!(Feature::parse("jaeger")
            .unwrap_err()
            .contains("telemetry, ollama"));
    }
}
