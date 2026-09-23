//! Absolute paths in the form everything else writes them.
//!
//! `std::fs::canonicalize` on Windows answers with a verbatim path — `\\?\C:\Users\me\Hub`.
//! Nothing else spells a folder that way: compose labels its containers with
//! `C:\Users\me\Hub`, Docker will not bind-mount a `\\?\` source, and the registry compares
//! paths as strings. So a container filter built from the verbatim form matched nothing,
//! and a running hub looked like one that had never been started.

use std::io;
use std::path::{Path, PathBuf};

/// `std::fs::canonicalize`, minus Windows' verbatim prefix where the plain form means the
/// same thing. Elsewhere it is exactly `std::fs::canonicalize`.
pub fn canonical(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    std::fs::canonicalize(path).map(plain)
}

/// `\\?\C:\…` → `C:\…` and `\\?\UNC\server\share\…` → `\\server\share\…`. Anything else —
/// a device path, or a verbatim path too long to be written without the prefix — is
/// returned as it came.
pub fn plain(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    let stripped = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        let drive = rest.as_bytes();
        if drive.len() >= 2 && drive[0].is_ascii_alphabetic() && drive[1] == b':' {
            rest.to_string()
        } else {
            return path;
        }
    } else {
        return path;
    };
    // MAX_PATH: past it the prefix is what makes the path usable at all.
    if stripped.len() >= 260 {
        return path;
    }
    PathBuf::from(stripped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_the_verbatim_prefix_from_a_drive_path() {
        assert_eq!(plain(PathBuf::from(r"\\?\C:\Users\me\MyHub")), PathBuf::from(r"C:\Users\me\MyHub"));
    }

    #[test]
    fn turns_a_verbatim_unc_path_back_into_a_share() {
        assert_eq!(plain(PathBuf::from(r"\\?\UNC\server\share\hub")), PathBuf::from(r"\\server\share\hub"));
    }

    #[test]
    fn leaves_everything_else_alone() {
        for p in ["/home/me/MyHub", r"C:\Users\me\MyHub", r"\\?\Volume{abc}\hub", r"\\.\pipe\x"] {
            assert_eq!(plain(PathBuf::from(p)), PathBuf::from(p));
        }
    }

    #[test]
    fn canonical_matches_what_compose_labels_with() {
        let dir = std::env::temp_dir();
        let resolved = canonical(&dir).unwrap();
        assert!(!resolved.to_string_lossy().starts_with(r"\\?\"));
    }
}
