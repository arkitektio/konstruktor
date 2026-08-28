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
        std::fs::write(target, contents)?;
    }
    Ok(())
}
