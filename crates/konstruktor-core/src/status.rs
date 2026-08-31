//! One status line per registered deployment, for anything that wants to show "what is
//! up right now" without opening a dashboard — the tray, and one day the CLI's `list`.

use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;

use crate::docker::{self, Container};
use crate::registry::{self, DeploymentRecord};

/// The states a whole deployment can be in, judged by its containers.
///
/// `None` and `Stopped` are deliberately apart: a deployment with no containers at all has
/// nothing to restart, while one whose containers merely exited needs starting again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunState {
    Running,
    Partial,
    Stopped,
    None,
}

impl RunState {
    pub fn label(self) -> &'static str {
        match self {
            RunState::Running => "Running",
            RunState::Partial => "Partly running",
            RunState::Stopped => "Stopped",
            RunState::None => "No containers",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSummary {
    pub state: RunState,
    pub running: usize,
    pub total: usize,
}

/// Whether this container is one of the run-once ones, where exited is the happy end.
pub fn is_init_container(container: &Container) -> bool {
    container
        .service
        .as_deref()
        .map(|s| s == "minio_init" || s.ends_with("_init"))
        .unwrap_or(false)
}

/// Counted over **every** container in the compose project, not just the arkitekt
/// services, minus the init containers — the same rule the dashboard applies, so the tray
/// and the dashboard never disagree about whether a hub is up.
pub fn run_summary(containers: &[Container]) -> RunSummary {
    let counted: Vec<&Container> = containers
        .iter()
        .filter(|c| !is_init_container(c))
        .collect();
    let total = counted.len();
    let running = counted
        .iter()
        .filter(|c| c.state.as_deref() == Some("running"))
        .count();

    let state = if total == 0 {
        RunState::None
    } else if running == 0 {
        RunState::Stopped
    } else if running == total {
        RunState::Running
    } else {
        RunState::Partial
    };
    RunSummary {
        state,
        running,
        total,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentStatus {
    pub record: DeploymentRecord,
    pub run: RunSummary,
    /// Why the containers could not be listed — a missing engine, most often. The
    /// summary is then `None`, which is true as far as anyone can tell.
    pub error: Option<String>,
}

impl DeploymentStatus {
    pub fn is_engine(&self) -> bool {
        self.record.kind == "engine"
    }
}

/// Every registered deployment with its run state, queried concurrently.
///
/// Never fails as a whole: a deployment whose folder is gone or whose engine is silent
/// reports its error and the rest still answer. Registry order is preserved.
pub async fn all() -> Vec<DeploymentStatus> {
    let records = registry::load().deployments;
    let mut set = JoinSet::new();
    for (index, record) in records.into_iter().enumerate() {
        set.spawn(async move {
            let (run, error) = match docker::list_deployment_containers(&record.path).await {
                Ok(containers) => (run_summary(&containers), None),
                Err(e) => (run_summary(&[]), Some(e)),
            };
            (index, DeploymentStatus { record, run, error })
        });
    }

    let mut results = Vec::new();
    while let Some(joined) = set.join_next().await {
        if let Ok(item) = joined {
            results.push(item);
        }
    }
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, status)| status).collect()
}

// --- what one deployment *is*, as opposed to what it is doing --------------------------
//
// Everything below answers "describe this hub" rather than "is it up". It lives here so
// that the dashboard and `konstruktor status` cannot describe the same hub differently —
// before this, each derived its own gateway URL, its own service list and its own notion
// of a release channel, and only the tray was on shared code.

/// One service, as a front end lists it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceView {
    pub id: String,
    pub name: String,
    pub host: String,
    /// Where a browser reaches it through the gateway.
    pub url: String,
    /// The image the profile pins this service to, e.g. `jhnnsrs/rekuest:next`.
    pub image: Option<String>,
    /// That image's tag on its own — the service's release channel.
    pub tag: Option<String>,
}

/// The release channel a hub follows, read off the images its services are pinned to.
///
/// There is no channel field in the profile: the channel *is* the set of tags, and those
/// are per-service. A hub whose services carry different tags has no single channel, and
/// saying so is the point of `tags` — the alternative is a front end that picks one and
/// lies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelView {
    /// The one tag every service shares, when they do share one.
    pub tag: Option<String>,
    /// Every distinct tag in play, sorted. More than one means the hub is mixed.
    pub tags: Vec<String>,
}

/// The tag out of an image reference, tolerating a registry host with a port
/// (`registry:5000/image:tag`) by only looking after the last slash.
pub fn image_tag(image: &str) -> Option<String> {
    // A digest-pinned reference is `repo:tag@sha256:…`, and the last colon in that belongs
    // to the digest. The tag is still the channel, so drop the pin before looking.
    let image = image.split('@').next().unwrap_or(image);
    let last = image.rsplit('/').next().unwrap_or(image);
    last.rsplit_once(':').map(|(_, tag)| tag.to_string())
}

/// The gateway's own address, as a browser on this machine would type it.
///
/// A default port is left off; anything else has to be spelled out.
pub fn gateway_url(config: &crate::config::hub::HubConfig) -> String {
    let scheme = crate::config::hub::scheme_of(config);
    let port = crate::connect::manifest::advertised_port(config);
    let host = config.domain.clone().unwrap_or_else(|| "localhost".into());

    let default_port = if config.gateway.ssl { 443 } else { 80 };
    let authority = if port == default_port {
        host
    } else {
        format!("{host}:{port}")
    };
    format!("{scheme}://{authority}")
}

/// The channel every enabled service is pinned to, or none if they disagree.
pub fn channel_of(services: &[ServiceView]) -> ChannelView {
    let mut tags: Vec<String> = services.iter().filter_map(|s| s.tag.clone()).collect();
    tags.sort();
    tags.dedup();
    ChannelView {
        tag: if tags.len() == 1 {
            tags.first().cloned()
        } else {
            None
        },
        tags,
    }
}

/// Everything a front end reads out of a deployment folder, derived once here so neither
/// the dashboard nor the CLI does any URL-building of its own.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubView {
    pub profile: crate::profile::Profile,
    pub authorized: bool,
    pub identifier: Option<String>,
    pub authorized_at: Option<String>,
    /// The gateway's own address, and the admin account for every service's admin panel.
    pub gateway_url: String,
    pub admin_user: String,
    pub admin_password: String,
    pub services: Vec<ServiceView>,
    pub mesh_hostname: Option<String>,
    /// The port an alias advertises, as the manifest computes it — so a reachability
    /// probe aims at the same socket the coordination server would hand out.
    pub advertised_port: u16,
    /// What this hub last told the coordination server it was reachable at.
    ///
    /// The authorize screen seeds from this rather than from a fresh scan: it exists to
    /// *add* the tailnet address, and a scan of this machine will never find one.
    pub advertised_hosts: Vec<crate::connect::manifest::AdvertisedHost>,
    /// The release channel the enabled services are pinned to.
    pub channel: ChannelView,
    /// Where the database and object storage live: the engine's volumes or the folder.
    pub storage: crate::config::hub::StorageMode,
}

/// Read a hub folder and describe it.
pub fn hub_view(dir: &std::path::Path) -> Result<HubView, crate::profile::ProfileError> {
    let profile = crate::profile::read_profile(dir)?;
    let creds = crate::credentials::read_credentials(dir);
    let config = &profile.config;

    let gateway_url = gateway_url(config);
    let catalog = crate::catalog::catalog();
    let services: Vec<ServiceView> = config
        .enabled_services()
        .into_iter()
        .map(|id| {
            let block = config.service(id);
            let meta = catalog.iter().find(|m| m.id == id);
            ServiceView {
                id: id.as_str().to_string(),
                name: meta
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| id.as_str().into()),
                host: block.host.clone(),
                url: format!("{gateway_url}/{}", block.host),
                image: block.image.clone(),
                tag: block.image.as_deref().and_then(image_tag),
            }
        })
        .collect();

    Ok(HubView {
        authorized: creds.is_some(),
        identifier: creds.as_ref().map(|c| c.identifier.clone()),
        authorized_at: creds.as_ref().map(|c| c.authorized_at.clone()),
        gateway_url,
        admin_user: config.global_admin.clone(),
        admin_password: config.global_admin_password.clone(),
        channel: channel_of(&services),
        services,
        storage: crate::config::hub::storage_mode_of(config),
        mesh_hostname: config
            .mesh
            .as_ref()
            .filter(|m| m.enabled)
            .map(|m| m.hostname.clone()),
        advertised_port: crate::connect::manifest::advertised_port(config),
        advertised_hosts: creds
            .as_ref()
            .map(|c| c.advertised_hosts.clone())
            .unwrap_or_default(),
        profile,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::hub::{build_hub_config, HubConfigOptions};

    fn config_with(ssl: bool, http: Option<u16>, https: Option<u16>) -> crate::config::hub::HubConfig {
        build_hub_config(&HubConfigOptions {
            device_id: "device".into(),
            coord_server: "go.arkitekt.live".into(),
            ssl,
            http_port: http,
            https_port: https,
            ..Default::default()
        })
    }

    #[test]
    fn a_default_port_is_left_off_the_url() {
        assert_eq!(
            gateway_url(&config_with(false, Some(80), None)),
            "http://localhost"
        );
        assert_eq!(
            gateway_url(&config_with(true, None, Some(443))),
            "https://localhost"
        );
    }

    #[test]
    fn any_other_port_is_spelled_out() {
        assert_eq!(
            gateway_url(&config_with(false, Some(7080), None)),
            "http://localhost:7080"
        );
        assert_eq!(
            gateway_url(&config_with(true, None, Some(7443))),
            "https://localhost:7443"
        );
    }

    /// A registry host may carry a port, and that colon is not a tag separator.
    #[test]
    fn a_tag_is_read_after_the_last_slash() {
        assert_eq!(image_tag("jhnnsrs/rekuest:next").as_deref(), Some("next"));
        assert_eq!(image_tag("registry:5000/rekuest").as_deref(), None);
        assert_eq!(
            image_tag("registry:5000/rekuest:next").as_deref(),
            Some("next")
        );
        assert_eq!(image_tag("rekuest").as_deref(), None);
        // A digest pin must not be mistaken for the tag: this hub is still on `dev`.
        assert_eq!(
            image_tag("jhnnsrs/daten:dev@sha256:c692f316").as_deref(),
            Some("dev")
        );
    }

    fn view(tag: Option<&str>) -> ServiceView {
        ServiceView {
            id: "rekuest".into(),
            name: "Rekuest".into(),
            host: "rekuest".into(),
            url: "http://localhost/rekuest".into(),
            image: None,
            tag: tag.map(str::to_string),
        }
    }

    #[test]
    fn one_shared_tag_is_the_channel() {
        let channel = channel_of(&[view(Some("next")), view(Some("next"))]);
        assert_eq!(channel.tag.as_deref(), Some("next"));
        assert_eq!(channel.tags, vec!["next".to_string()]);
    }

    /// A mixed hub has no single channel, and must not be shown as if it had one.
    #[test]
    fn mixed_tags_leave_the_channel_unset_but_still_listed() {
        let channel = channel_of(&[view(Some("next")), view(Some("stable"))]);
        assert_eq!(channel.tag, None);
        assert_eq!(channel.tags, vec!["next".to_string(), "stable".to_string()]);
    }

    #[test]
    fn no_tags_at_all_is_not_a_channel() {
        let channel = channel_of(&[view(None)]);
        assert_eq!(channel.tag, None);
        assert!(channel.tags.is_empty());
    }

    fn container(service: &str, state: &str) -> Container {
        Container {
            id: None,
            names: None,
            image: None,
            image_id: None,
            labels: None,
            status: None,
            state: Some(state.to_string()),
            service: Some(service.to_string()),
        }
    }

    #[test]
    fn empty_is_none() {
        let summary = run_summary(&[]);
        assert_eq!(summary.state, RunState::None);
        assert_eq!((summary.running, summary.total), (0, 0));
    }

    #[test]
    fn all_running() {
        let summary = run_summary(&[container("db", "running"), container("web", "running")]);
        assert_eq!(summary.state, RunState::Running);
        assert_eq!((summary.running, summary.total), (2, 2));
    }

    #[test]
    fn one_exited_is_partial() {
        let summary = run_summary(&[container("db", "running"), container("web", "exited")]);
        assert_eq!(summary.state, RunState::Partial);
        assert_eq!((summary.running, summary.total), (1, 2));
    }

    #[test]
    fn all_exited_is_stopped() {
        let summary = run_summary(&[container("db", "exited")]);
        assert_eq!(summary.state, RunState::Stopped);
    }

    #[test]
    fn init_containers_do_not_count() {
        let summary = run_summary(&[
            container("db", "running"),
            container("minio_init", "exited"),
            container("gateway_init", "exited"),
        ]);
        assert_eq!(summary.state, RunState::Running);
        assert_eq!((summary.running, summary.total), (1, 1));

        let only_init = run_summary(&[container("minio_init", "exited")]);
        assert_eq!(only_init.state, RunState::None);
    }
}
