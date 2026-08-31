use std::io::{IsTerminal, Write};

/// Terminal output, and the one rule that governs it: anything a machine might parse goes
/// to stdout, everything a human reads goes to stderr. `--json` then works in a pipe
/// without the progress chatter contaminating it.

pub fn is_interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

fn colour(code: &str, text: &str) -> String {
    if std::io::stderr().is_terminal() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

pub fn dim(text: &str) -> String {
    colour("2", text)
}
pub fn bold(text: &str) -> String {
    colour("1", text)
}

pub fn say(text: &str) {
    eprintln!("{text}");
}

pub fn step(text: &str) {
    eprintln!("  {text}");
}

pub fn ok(text: &str) {
    eprintln!("  {} {text}", colour("32", "✓"));
}

pub fn warn(text: &str) {
    eprintln!("  {} {text}", colour("33", "!"));
}

pub fn fail(text: &str) {
    eprintln!("  {} {text}", colour("31", "✗"));
}

/// Rewrites the current line, for progress that would otherwise scroll. Falls back to one
/// line per update when stderr is not a terminal, so CI logs stay readable.
pub fn progress(text: &str) {
    if std::io::stderr().is_terminal() {
        eprint!("\r\x1b[2K  {text}");
        let _ = std::io::stderr().flush();
    } else {
        eprintln!("  {text}");
    }
}

pub fn end_progress() {
    if std::io::stderr().is_terminal() {
        eprintln!();
    }
}

/// Opens a URL without ever failing on it — the address is printed regardless.
pub fn open_in_browser(url: &str) {
    let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![])
    } else if cfg!(target_os = "windows") {
        ("cmd", vec!["/c", "start", ""])
    } else {
        ("xdg-open", vec![])
    };

    let _ = std::process::Command::new(program)
        .args(args)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

pub fn table(rows: &[(String, String)]) {
    let width = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    for (key, value) in rows {
        eprintln!("  {:width$}  {value}", dim(key), width = width);
    }
}

/// The one machine-readable document a command produces, on stdout.
///
/// The stdout/stderr split this module is built around is what makes this work in a pipe:
/// every narration above goes to stderr, so `konstruktor status --json | jq` sees the
/// document and nothing else.
pub fn emit_json<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
