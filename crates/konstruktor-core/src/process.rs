//! Every child process Konstruktor starts goes through here.
//!
//! On Windows a console program started from a GUI process — the desktop app has no
//! console of its own — gets a fresh console window, which flashes up for every `docker`,
//! `git` or `winget` call. The dashboard and the tray poll the engine, so that is a window
//! every few seconds. `CREATE_NO_WINDOW` starts the child without one; its output still
//! reaches us through the pipes every caller here sets up.

use std::ffi::OsStr;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `std::process::Command::new`, minus the console window on Windows.
pub fn command(program: impl AsRef<OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// `tokio::process::Command::new`, minus the console window on Windows.
pub fn async_command(program: impl AsRef<OsStr>) -> tokio::process::Command {
    #[allow(unused_mut)]
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}
