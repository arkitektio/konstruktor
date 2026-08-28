use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::compose::project_name;

/// The index of known deployments — **the same file the desktop app writes**.
///
/// Deployments live wherever the user put them, so the app keeps its own index. Sharing
/// it means a hub created from the CLI shows up in the desktop app and vice versa, and
/// both agree on `device_id`, which becomes every service manifest's `node_id`.
///
/// It lives in Tauri's AppData directory, which is the platform data dir joined with the
/// bundle identifier — so the path has to be reproduced here exactly.
pub const REGISTRY_FILENAME: &str = "deployments.json";
pub const BUNDLE_IDENTIFIER: &str = "io.github.jhnnsrs.konstruktor";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentRecord {
    pub id: String,
    /// User-facing label; the folder name is the default.
    pub name: String,
    /// Canonical absolute path of the deployment directory.
    pub path: String,
    pub kind: String,
    /// Cached `project_name(path)`, for display only.
    pub project: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "lastGeneratedAt", skip_serializing_if = "Option::is_none")]
    pub last_generated_at: Option<String>,
    /// The coordination server this hub was authorized against.
    #[serde(rename = "coordServer", skip_serializing_if = "Option::is_none")]
    pub coord_server: Option<String>,
    /// The hub's identifier inside the organization that accepted it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryFile {
    pub version: u8,
    /// Stable id for this machine. It becomes the profile's `device_id` and the `node_id`
    /// on every service manifest, so a re-authorized hub is recognised as the same node.
    #[serde(rename = "deviceId")]
    pub device_id: String,
    pub deployments: Vec<DeploymentRecord>,
}

impl RegistryFile {
    fn empty(device_id: String) -> Self {
        Self {
            version: 1,
            device_id,
            deployments: Vec::new(),
        }
    }
}

/// Tauri's `BaseDirectory::AppData`: the platform data directory joined with the bundle
/// identifier.
pub fn registry_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(BUNDLE_IDENTIFIER))
}

pub fn registry_path() -> Option<PathBuf> {
    registry_dir().map(|d| d.join(REGISTRY_FILENAME))
}

fn new_id() -> String {
    // A UUID would be a dependency for one call site; 32 hex characters from the CSPRNG
    // carry the same entropy and the registry only ever compares them for equality.
    crate::secrets::generate_alpha_numeric_string(32)
}

/// Reads the registry, falling back to an empty one when the file is missing or corrupt —
/// the same tolerance the desktop app has, so a bad file never blocks either front end.
pub fn load() -> RegistryFile {
    let Some(path) = registry_path() else {
        return RegistryFile::empty(new_id());
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<RegistryFile>(&text)
            .map(|mut r| {
                if r.device_id.is_empty() {
                    r.device_id = new_id();
                }
                r
            })
            .unwrap_or_else(|_| RegistryFile::empty(new_id())),
        Err(_) => RegistryFile::empty(new_id()),
    }
}

pub fn save(registry: &RegistryFile) -> std::io::Result<()> {
    let Some(dir) = registry_dir() else {
        return Ok(());
    };
    std::fs::create_dir_all(&dir)?;
    let mut json = serde_json::to_string_pretty(registry).expect("serializes");
    json.push('\n');
    std::fs::write(dir.join(REGISTRY_FILENAME), json)
}

pub fn normalize_path(path: &str) -> String {
    path.trim_end_matches(['/', '\\']).to_string()
}

pub fn find_by_path<'a>(registry: &'a RegistryFile, path: &str) -> Option<&'a DeploymentRecord> {
    let wanted = normalize_path(path);
    registry
        .deployments
        .iter()
        .find(|d| normalize_path(&d.path) == wanted)
}

pub fn find_by_name<'a>(registry: &'a RegistryFile, name: &str) -> Option<&'a DeploymentRecord> {
    registry.deployments.iter().find(|d| d.name == name)
}

/// A deployment in a *different* folder that would derive the same compose project, and
/// therefore share this one's containers.
pub fn find_project_collision<'a>(
    registry: &'a RegistryFile,
    path: &str,
) -> Option<&'a DeploymentRecord> {
    let project = project_name(path);
    let wanted = normalize_path(path);
    registry
        .deployments
        .iter()
        .find(|d| d.project == project && normalize_path(&d.path) != wanted)
}

/// What we learned about a folder somebody wants to put a deployment in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderVerdict {
    /// Usable. `not_empty` is worth saying out loud, but does not block.
    Create {
        not_empty: bool,
    },
    /// Already holds a hub config — offer to adopt it instead of creating.
    Import,
    Missing,
    AlreadyRegistered {
        name: String,
    },
    ProjectCollision {
        other: String,
        project: String,
    },
}

pub fn inspect_folder(registry: &RegistryFile, path: &Path) -> FolderVerdict {
    if !path.exists() {
        return FolderVerdict::Missing;
    }
    let as_str = path.to_string_lossy().to_string();

    if let Some(existing) = find_by_path(registry, &as_str) {
        return FolderVerdict::AlreadyRegistered {
            name: existing.name.clone(),
        };
    }
    if let Some(other) = find_project_collision(registry, &as_str) {
        return FolderVerdict::ProjectCollision {
            other: other.path.clone(),
            project: other.project.clone(),
        };
    }
    if crate::profile::holds_a_hub(path) {
        return FolderVerdict::Import;
    }

    let not_empty = std::fs::read_dir(path)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false);
    FolderVerdict::Create { not_empty }
}

impl FolderVerdict {
    pub fn can_create(&self) -> bool {
        matches!(self, FolderVerdict::Create { .. })
    }

    pub fn describe(&self) -> String {
        match self {
            FolderVerdict::Create { not_empty: false } => "This folder can be used.".into(),
            FolderVerdict::Create { not_empty: true } => {
                "This folder is not empty. The deployment will be created alongside what \
                 is already in it."
                    .into()
            }
            FolderVerdict::Import => {
                "This folder already holds a hub configuration — it can be added as an \
                 existing deployment."
                    .into()
            }
            FolderVerdict::Missing => "That folder does not exist.".into(),
            FolderVerdict::AlreadyRegistered { name } => {
                format!("This folder is already registered as \"{name}\".")
            }
            FolderVerdict::ProjectCollision { other, project } => format!(
                "Another deployment at {other} would share the compose project \
                 \"{project}\", and therefore this one's containers."
            ),
        }
    }
}

/// Adds or replaces the record for a path.
///
/// `now` dates `last_generated_at` as well as `created_at`: registering happens straight
/// after the generated files are written, so the two really are the same instant. Leaving
/// it unset — as this did — left the dashboard's "Configured" stage permanently
/// unanswered and made its staleness check dead code, since `None` can never be older
/// than an authorization.
pub fn register(
    registry: &mut RegistryFile,
    name: &str,
    path: &str,
    coord_server: Option<String>,
    identifier: Option<String>,
    now: String,
) -> DeploymentRecord {
    let record = DeploymentRecord {
        id: new_id(),
        name: name.to_string(),
        path: path.to_string(),
        kind: "hub".to_string(),
        project: project_name(path),
        created_at: now.clone(),
        last_generated_at: Some(now),
        coord_server,
        identifier,
    };
    let wanted = normalize_path(path);
    registry
        .deployments
        .retain(|d| normalize_path(&d.path) != wanted);
    registry.deployments.push(record.clone());
    record
}

/// Records that a deployment's files were regenerated, folding in whatever the
/// authorization changed. Returns whether a record was found.
///
/// Deliberately not [`register`]: that mints a fresh id and replaces the record, which
/// would invalidate the `/dashboard/<id>` route the user is standing on. Re-authorizing
/// changes what a deployment *is*, not which deployment it is.
///
/// Without this, re-authorizing moves `authorized_at` forward while `last_generated_at`
/// stays where creation left it, and the dashboard reports configs as written before the
/// authorization on a folder whose configs were just rewritten.
pub fn record_regeneration(
    registry: &mut RegistryFile,
    path: &str,
    coord_server: Option<String>,
    identifier: Option<String>,
    now: String,
) -> bool {
    let wanted = normalize_path(path);
    let Some(record) = registry
        .deployments
        .iter_mut()
        .find(|d| normalize_path(&d.path) == wanted)
    else {
        return false;
    };

    record.last_generated_at = Some(now);
    if coord_server.is_some() {
        record.coord_server = coord_server;
    }
    if identifier.is_some() {
        record.identifier = identifier;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_with(paths: &[(&str, &str)]) -> RegistryFile {
        let mut registry = RegistryFile::empty("device".into());
        for (name, path) in paths {
            register(&mut registry, name, path, None, None, "now".into());
        }
        registry
    }

    #[test]
    fn resolves_the_same_appdata_path_the_desktop_app_uses() {
        let path = registry_path().expect("a data dir on this platform");
        assert!(
            path.ends_with("io.github.jhnnsrs.konstruktor/deployments.json"),
            "{path:?}"
        );
    }

    #[test]
    fn refuses_a_folder_that_is_already_registered() {
        let dir = std::env::temp_dir();
        let registry = registry_with(&[("MyHub", &dir.to_string_lossy())]);
        assert_eq!(
            inspect_folder(&registry, &dir),
            FolderVerdict::AlreadyRegistered {
                name: "MyHub".into()
            }
        );
    }

    /// Two folders with the same name in different places derive the same compose
    /// project, so starting one would adopt the other's containers.
    #[test]
    fn catches_a_compose_project_collision() {
        let registry = registry_with(&[("MyHub", "/elsewhere/MyHub")]);
        let collision = find_project_collision(&registry, "/home/someone/MyHub");
        assert_eq!(collision.map(|d| d.path.as_str()), Some("/elsewhere/MyHub"));

        // The same folder is not a collision with itself.
        assert!(find_project_collision(&registry, "/elsewhere/MyHub/").is_none());
    }

    #[test]
    fn re_registering_a_path_replaces_rather_than_duplicates() {
        let mut registry = registry_with(&[("MyHub", "/home/someone/MyHub")]);
        register(
            &mut registry,
            "Renamed",
            "/home/someone/MyHub",
            None,
            None,
            "now".into(),
        );
        assert_eq!(registry.deployments.len(), 1);
        assert_eq!(registry.deployments[0].name, "Renamed");
    }

    /// The dashboard answers "is this configured" from `last_generated_at`, and can only
    /// ever have registered a deployment whose files were just written.
    #[test]
    fn registering_dates_the_generation_as_well_as_the_creation() {
        let registry = registry_with(&[("hub", "/tmp/hub")]);
        let record = find_by_path(&registry, "/tmp/hub").expect("registered");
        assert_eq!(record.last_generated_at.as_deref(), Some("now"));
        assert_eq!(record.created_at, "now");
    }

    /// Re-authorizing rewrites the configs, so the record has to move with them — and has
    /// to keep its id, which the dashboard route the user is on is keyed by.
    #[test]
    fn recording_a_regeneration_updates_in_place() {
        let mut registry = registry_with(&[("hub", "/tmp/hub")]);
        let before = find_by_path(&registry, "/tmp/hub")
            .expect("registered")
            .id
            .clone();

        assert!(record_regeneration(
            &mut registry,
            // A trailing separator is the same folder.
            "/tmp/hub/",
            Some("go.arkitekt.live".into()),
            Some("renamed".into()),
            "later".into(),
        ));

        assert_eq!(registry.deployments.len(), 1);
        let record = find_by_path(&registry, "/tmp/hub").expect("still there");
        assert_eq!(record.id, before);
        assert_eq!(record.last_generated_at.as_deref(), Some("later"));
        assert_eq!(record.coord_server.as_deref(), Some("go.arkitekt.live"));
        assert_eq!(record.identifier.as_deref(), Some("renamed"));
    }

    #[test]
    fn recording_a_regeneration_for_an_unknown_folder_adds_nothing() {
        let mut registry = registry_with(&[("hub", "/tmp/hub")]);
        assert!(!record_regeneration(
            &mut registry,
            "/tmp/other",
            None,
            None,
            "later".into()
        ));
        assert_eq!(registry.deployments.len(), 1);
    }

    #[test]
    fn a_missing_folder_is_not_usable() {
        let registry = RegistryFile::empty("device".into());
        let verdict = inspect_folder(&registry, Path::new("/definitely/not/here"));
        assert_eq!(verdict, FolderVerdict::Missing);
        assert!(!verdict.can_create());
    }
}
