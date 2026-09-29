use std::path::Path;

use super::GeneratedFiles;

/// The only part of generation that touches the filesystem.
///
/// Paths in a [`GeneratedFiles`] map are relative and POSIX-separated; they are joined
/// onto the deployment folder here and their parent directories created on the way.
pub fn write_generated_files(dir: &Path, files: &GeneratedFiles) -> std::io::Result<()> {
    for (relative, contents) in files {
        let target = dir.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, contents)?;
        // Key files are readable by their owner alone, where the platform can say so. The
        // containers run as root, so the read-only mount still reads them.
        #[cfg(unix)]
        if relative.starts_with("secrets/") {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    Ok(())
}

/// Writes the mesh sidecar's `mesh.env` from the profile when the hub is on a mesh and the
/// file is missing. True when it wrote one.
pub fn ensure_mesh_env(
    dir: &Path,
    config: &crate::config::hub::HubConfig,
) -> std::io::Result<bool> {
    let Some(mesh) = config.mesh.as_ref().filter(|m| m.enabled) else {
        return Ok(false);
    };
    let target = dir.join(crate::config::mesh::MESH_ENV_FILE);
    if target.exists() {
        return Ok(false);
    }
    std::fs::write(&target, mesh.env_file_contents())?;
    Ok(true)
}
