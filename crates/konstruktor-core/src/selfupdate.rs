//! Konstruktor replacing itself with a newer release of itself.
//!
//! The installers fetch the newest published release, check it against the release's
//! `SHA256SUMS` and put it in a folder. This is the same three steps from inside the
//! binary, so a machine that has Konstruktor does not need the installer again to get the
//! next one — and with it the releases of the services that only a newer Konstruktor
//! knows how to run.
//!
//! Only a binary an installer put there replaces itself. One that came in a Python wheel
//! belongs to the environment it was installed into, which would go on believing it holds
//! the old version: that one is upgraded with the tool that installed it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

const REPOSITORY: &str = "https://github.com/arkitektio/konstruktor";
const TAG_PREFIX: &str = "konstruktor-v";
const TIMEOUT: Duration = Duration::from_secs(10);
/// A binary is tens of megabytes: longer than a question, shorter than forever.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// The version this binary is.
pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The build of Konstruktor a release publishes for the machine this one was built for,
/// by the name the release calls it. `None` where no release has one.
pub fn asset() -> Option<&'static str> {
    // Linux is the static musl build whatever this binary was linked against: it is the
    // one that runs on every distribution, and the one the installer fetches.
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("konstruktor-x86_64-unknown-linux-musl"),
        ("linux", "aarch64") => Some("konstruktor-aarch64-unknown-linux-musl"),
        ("macos", "x86_64") => Some("konstruktor-x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("konstruktor-aarch64-apple-darwin"),
        // Windows on ARM runs the x64 build under emulation, as the installer has it.
        ("windows", _) => Some("konstruktor-x86_64-pc-windows-msvc.exe"),
        _ => None,
    }
}

/// The tag of a release: `konstruktor-v0.20.0` for `0.20.0`, `v0.20.0` or the tag itself.
pub fn tag_of(version: &str) -> String {
    let version = version.trim();
    match version.strip_prefix(TAG_PREFIX) {
        Some(_) => version.to_string(),
        None => format!("{TAG_PREFIX}{}", version.trim_start_matches('v')),
    }
}

/// The version a release's tag names.
pub fn version_of(tag: &str) -> &str {
    tag.strip_prefix(TAG_PREFIX).unwrap_or(tag)
}

/// The tag a release's page is served under, read off its address.
pub(crate) fn tag_in(address: &str) -> Option<&str> {
    let (_, tag) = address.rsplit_once("/releases/tag/")?;
    let tag = tag.split(['?', '#', '/']).next()?;
    tag.starts_with(TAG_PREFIX).then_some(tag)
}

/// Whether `version` is a later release than this binary.
pub fn is_newer(version: &str) -> bool {
    newer_than(version, current())
}

pub(crate) fn newer_than(version: &str, than: &str) -> bool {
    !crate::contract::at_least(than, version)
}

fn client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(concat!("konstruktor/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

/// The tag of the newest published release.
///
/// Read off where `releases/latest` leads, which is what the installers follow too: a
/// release that is still a draft — one of its builds failed, or is not attached yet — is
/// not it. Asked of the site rather than of GitHub's API, which answers an address without
/// a token only sixty times an hour.
pub async fn latest() -> Result<String, String> {
    let response = client(TIMEOUT)?
        .get(format!("{REPOSITORY}/releases/latest"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    tag_in(response.url().as_str())
        .map(str::to_string)
        .ok_or_else(|| "no published release was found".to_string())
}

/// The checksum a release's `SHA256SUMS` lists for `asset`.
pub(crate) fn listed<'a>(sums: &'a str, asset: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let (digest, name) = line.split_once(char::is_whitespace)?;
        // `sha256sum` marks a file read as binary with a star before its name.
        (name.trim().trim_start_matches('*') == asset).then_some(digest)
    })
}

/// `bytes`, if they are what the release's `sums` say `asset` is.
pub(crate) fn verified(bytes: Vec<u8>, sums: &str, asset: &str) -> Result<Vec<u8>, String> {
    let expected = listed(sums, asset)
        .ok_or_else(|| format!("{asset} is not listed in that release's SHA256SUMS"))?;
    let actual: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    match actual.eq_ignore_ascii_case(expected) {
        true => Ok(bytes),
        false => Err(format!(
            "the download does not match that release's checksum — nothing was replaced \
             (expected {expected}, got {actual})"
        )),
    }
}

/// The build for this machine out of the release `tag`, checked against the checksums the
/// release publishes. A release without them is refused, as the installers refuse it.
pub async fn download(tag: &str) -> Result<Vec<u8>, String> {
    let asset = asset().ok_or_else(|| {
        format!(
            "no release has a build for {} on {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;
    let base = format!("{REPOSITORY}/releases/download/{tag}");
    let fetch = |name: &'static str, what: &'static str| {
        let url = format!("{base}/{name}");
        async move {
            client(DOWNLOAD_TIMEOUT)?
                .get(url)
                .send()
                .await
                .and_then(|response| response.error_for_status())
                .map_err(|e| match e.status() {
                    Some(reqwest::StatusCode::NOT_FOUND) => {
                        format!("the release {tag} has no {what}")
                    }
                    _ => e.to_string(),
                })?
                .bytes()
                .await
                .map_err(|e| e.to_string())
        }
    };
    let sums = fetch("SHA256SUMS", "SHA256SUMS to check a download against").await?;
    let binary = fetch(asset, "build for this machine").await?;
    verified(binary.to_vec(), &String::from_utf8_lossy(&sums), asset)
}

/// Who a binary belongs to, as far as replacing it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    /// An installer put it there, or somebody by hand: it replaces itself.
    Itself,
    /// A Python environment, which it came into inside the `konstruktor` wheel.
    Python,
}

/// Who the binary at `exe` belongs to.
///
/// A wheel puts its binary in the `bin` (`Scripts` on Windows) of the environment it was
/// installed into, and an environment says it is one in the `pyvenv.cfg` above that — a
/// venv, and what `uv tool` and `pipx` make. An interpreter beside the binary says nothing:
/// `~/.local/bin` holds both on many machines.
pub fn owner(exe: &Path) -> Owner {
    let environment = exe
        .parent()
        .and_then(Path::parent)
        .is_some_and(|root| root.join("pyvenv.cfg").exists());
    match environment {
        true => Owner::Python,
        false => Owner::Itself,
    }
}

/// Where the binary that was running is put while a new one takes its name, on Windows.
fn set_aside(exe: &Path) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    exe.with_file_name(name)
}

/// Puts `binary` where `exe` is.
///
/// Written beside it and moved over it, so there is never half a binary under the name:
/// the move either happened or it did not, and a running Konstruktor goes on as the file
/// it opened. Windows will not have a running program's file replaced, but lets it be
/// renamed — so there the old one steps aside first, and is removed by the next update.
pub fn replace(exe: &Path, binary: &[u8]) -> std::io::Result<()> {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".new");
    let fresh = exe.with_file_name(name);
    let written = (|| {
        std::fs::write(&fresh, binary)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o755))?;
        }
        if cfg!(windows) && exe.exists() {
            let aside = set_aside(exe);
            let _ = std::fs::remove_file(&aside);
            std::fs::rename(exe, &aside)?;
            if let Err(error) = std::fs::rename(&fresh, exe) {
                // Back under its name: a machine left without the command is the one
                // outcome worse than not updating.
                let _ = std::fs::rename(&aside, exe);
                return Err(error);
            }
            return Ok(());
        }
        std::fs::rename(&fresh, exe)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&fresh);
    }
    written
}

/// Removes what an earlier update set aside ([`replace`], on Windows). Nothing to do, and
/// nothing said, where there is none or it is still running.
pub fn tidy(exe: &Path) {
    let _ = std::fs::remove_file(set_aside(exe));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "konstruktor-selfupdate-{tag}-{}-{}",
            std::process::id(),
            crate::lock::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_release_is_named_by_its_tag_however_it_is_asked_for() {
        for asked in ["0.20.0", "v0.20.0", "konstruktor-v0.20.0", " 0.20.0 "] {
            assert_eq!(tag_of(asked), "konstruktor-v0.20.0", "{asked}");
        }
        assert_eq!(version_of("konstruktor-v0.20.0"), "0.20.0");
    }

    #[test]
    fn the_newest_release_is_read_off_where_latest_leads() {
        assert_eq!(
            tag_in("https://github.com/arkitektio/konstruktor/releases/tag/konstruktor-v0.20.0"),
            Some("konstruktor-v0.20.0")
        );
        // A repository without a published release leads back to its list of none.
        assert_eq!(
            tag_in("https://github.com/arkitektio/konstruktor/releases"),
            None
        );
        assert_eq!(
            tag_in("https://github.com/arkitektio/konstruktor/releases/tag/nightly"),
            None
        );
    }

    #[test]
    fn only_a_later_release_is_newer() {
        assert!(newer_than("0.20.0", "0.19.0"));
        // By number: 0.9 is before 0.19.
        assert!(!newer_than("0.9.1", "0.19.0"));
        assert!(!newer_than("0.19.0", "0.19.0"));
        assert!(!newer_than("0.18.0", "0.19.0"));
    }

    #[test]
    fn a_download_is_kept_only_if_it_is_what_the_release_says() {
        let binary = b"a build".to_vec();
        let digest: String = Sha256::digest(&binary)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let sums = format!(
            "{}  konstruktor-other\n{digest} *konstruktor-mine\n",
            "0".repeat(64)
        );

        assert_eq!(
            verified(binary.clone(), &sums, "konstruktor-mine"),
            Ok(binary.clone())
        );
        let other = verified(binary.clone(), &sums, "konstruktor-other").unwrap_err();
        assert!(other.contains("nothing was replaced"), "{other}");
        let unlisted = verified(binary, &sums, "konstruktor-unlisted").unwrap_err();
        assert!(unlisted.contains("not listed"), "{unlisted}");
    }

    #[test]
    fn a_replaced_binary_is_the_new_one_and_runs() {
        let dir = scratch("replace");
        let exe = dir.join("konstruktor");
        std::fs::write(&exe, "old").unwrap();

        replace(&exe, b"new").unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&exe).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "it can be run");
        }
        tidy(&exe);
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, ["konstruktor"], "nothing is left beside it");
        std::fs::remove_dir_all(&dir).ok();
    }

    // Where a file is moved over the name. Windows moves what holds the name aside first,
    // and a folder steps aside as readily as a file.
    #[cfg(unix)]
    #[test]
    fn a_binary_that_cannot_be_replaced_is_left_as_it_was() {
        let dir = scratch("refused");
        // A folder under the binary's name: nothing can be moved over it.
        let exe = dir.join("konstruktor");
        std::fs::create_dir_all(exe.join("held")).unwrap();

        assert!(replace(&exe, b"new").is_err());
        assert!(exe.join("held").exists());
        assert!(!dir.join("konstruktor.new").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_binary_in_a_python_environment_is_that_environments() {
        let dir = scratch("owner");
        let installed = dir.join("local/bin/konstruktor");
        let wheel = dir.join("venv/bin/konstruktor");
        for exe in [&installed, &wheel] {
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(exe, "").unwrap();
        }
        std::fs::write(dir.join("venv/pyvenv.cfg"), "").unwrap();

        assert_eq!(owner(&installed), Owner::Itself);
        assert_eq!(owner(&wheel), Owner::Python);
        // An interpreter beside it does not make the folder an environment.
        std::fs::write(dir.join("local/bin/python3"), "").unwrap();
        assert_eq!(owner(&installed), Owner::Itself);
        std::fs::remove_dir_all(&dir).ok();
    }
}
