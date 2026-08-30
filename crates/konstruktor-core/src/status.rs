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

#[cfg(test)]
mod tests {
    use super::*;

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
