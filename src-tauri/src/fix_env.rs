// Copyright 2021 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

/// The error that might happen on a [`fix`] call.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Shell(#[from] std::io::Error),
    #[error("failed to run shell echo: {0}")]
    EchoFailed(String),
}

/// Reads the shell configuration to properly set all given environment variables.
///
/// ## Platform-specific
///
/// - **Windows**: Does nothing as the environment variables are already set.
pub fn fix_vars(vars: &[&str]) -> std::result::Result<(), Error> {
    #[cfg(windows)]
    {
        let _vars = vars;
        return Ok(());
    }
    #[cfg(not(windows))]
    {
        let default_shell = if cfg!(target_os = "macos") {
            "/bin/zsh"
        } else {
            "/bin/sh"
        };
        let shell = std::env::var("SHELL").unwrap_or_else(|_| default_shell.into());

        let out = std::process::Command::new(shell)
            .arg("-ilc")
            .arg("echo -n \"_SHELL_ENV_DELIMITER_\"; env; echo -n \"_SHELL_ENV_DELIMITER_\"; exit")
            // Disables Oh My Zsh auto-update thing that can block the process.
            .env("DISABLE_AUTO_UPDATE", "true")
            .output()
            .map_err(Error::Shell)?;

        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            let env = stdout.split("_SHELL_ENV_DELIMITER_").nth(1).unwrap();
            for line in String::from_utf8_lossy(&strip_ansi_escapes::strip(env))
                .split('\n')
                .filter(|l| !l.is_empty())
            {
                let mut s = line.splitn(2, '=');
                if let (Some(var), Some(value)) = (s.next(), s.next()) {
                    if vars.is_empty() || vars.contains(&var) {
                        std::env::set_var(var, value);
                    }
                }
            }
            Ok(())
        } else {
            Err(Error::EchoFailed(
                String::from_utf8_lossy(&out.stderr).into_owned(),
            ))
        }
    }
}

/// Reads the shell configuration to properly set the variables needed to find and reach
/// the container engine.
///
/// `PATH` so the `docker` or `podman` binary can be found at all — an app launched from
/// Finder inherits `/usr/bin:/bin:/usr/sbin:/sbin`, which has neither Homebrew nor Docker
/// Desktop's own bin directory in it. The rest because a user who points their shell at a
/// non-default engine endpoint means it for this app too, and without them the app would
/// silently talk to a different daemon than their terminal does.
///
/// ## Platform-specific
///
/// - **Windows**: Does nothing as the environment variables are already set.
pub fn fix() -> std::result::Result<(), Error> {
    fix_vars(&[
        "PATH",
        // Docker and every docker-compatible runtime.
        "DOCKER_HOST",
        "DOCKER_CONTEXT",
        "DOCKER_CONFIG",
        // Podman.
        "CONTAINER_HOST",
        // Which engine to prefer, when both are installed.
        "KONSTRUKTOR_ENGINE",
    ])
}

/// Reads the shell configuration to properly set all environment variables.
///
/// ## Platform-specific
///
/// - **Windows**: Does nothing as the environment variables are already set.
pub fn fix_all_vars() -> std::result::Result<(), Error> {
    fix_vars(&[])
}
