//! Tearing down the stacks a front end started, when that front end goes away.
//!
//! Deliberately blocking: the only caller is a process that is already exiting, so
//! anything handed to an async runtime would never get to run.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::compose;

/// Stops one deployment's containers, leaving them in place.
///
/// `timeout_secs` overrides every service's grace period, so `None` — meaning "whatever
/// the compose file says" — is the right default: the app services declare
/// `stop_grace_period: 2s` and the database declares none, which is exactly the one that
/// should be allowed compose's full ten seconds to close cleanly.
///
/// Returns whether compose reported success. A folder that has since been deleted, or a
/// Docker that is no longer running, is a `false` and nothing more — there is no one left
/// to report it to.
pub fn stop(dir: &Path, timeout_secs: Option<u32>) -> bool {
    let args: Vec<String> = match timeout_secs {
        Some(seconds) => compose::stop_timeout(seconds),
        None => compose::stop().into_iter().map(String::from).collect(),
    };

    Command::new("docker")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Stops every deployment in `dirs`, in parallel, and waits for all of them.
///
/// Parallel because a grace period is per stack: stopping five of them one after another
/// would be five times the wait for no reason.
pub fn stop_all(dirs: &[PathBuf], timeout_secs: Option<u32>) {
    let handles: Vec<_> = dirs
        .iter()
        .cloned()
        .map(|dir| std::thread::spawn(move || stop(&dir, timeout_secs)))
        .collect();

    for handle in handles {
        let _ = handle.join();
    }
}
