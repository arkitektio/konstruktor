//! `konstruktor tls`: the certificate a deployment's public site is served with.
//!
//! A certificate the gateway obtains over ACME renews itself, and there is nothing to do. One
//! that is provided is renewed where it lies — by certbot, `tailscale cert`, an institution —
//! and the gateway serves a copy of it: `reload` takes the renewed one in, without a restart,
//! and is what certbot's `--deploy-hook` runs. `set` points the site at other files, or from
//! ACME to files of one's own.

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Subcommand};
use konstruktor_core::config::hub::PublicTls;
use konstruktor_core::profile;

use crate::manage::Target;
use crate::ui;

#[derive(Subcommand)]
pub enum TlsCommand {
    /// What the public site is served with, and for how long.
    Status(Target),
    /// Take a renewed certificate in, from where it lies, and have the gateway serve it.
    Reload(Target),
    /// Serve the public site with these files from now on.
    Set(SetArgs),
}

#[derive(Args, Debug, Clone)]
pub struct SetArgs {
    /// The certificate and its chain, PEM, leaf first.
    #[arg(long, value_name = "FILE")]
    pub cert: String,
    /// Its private key, PEM.
    #[arg(long, value_name = "FILE")]
    pub key: String,
    #[command(flatten)]
    pub target: Target,
}

pub async fn run(command: TlsCommand, json: bool) -> Result<()> {
    match command {
        TlsCommand::Status(target) => status(&target, json),
        TlsCommand::Reload(target) => reload(&target).await,
        TlsCommand::Set(args) => set(args).await,
    }
}

fn public_site(dir: &std::path::Path) -> Result<konstruktor_core::config::hub::HubConfig> {
    let config = profile::read_profile(dir)?.config;
    if config.public_site().is_none() {
        bail!(
            "{} is served at no public name: it has nothing but its own certificate.",
            dir.display()
        );
    }
    Ok(config)
}

fn status(target: &Target, json: bool) -> Result<()> {
    let dir = target.resolve()?;
    let config = public_site(&dir)?;
    let site = config.public_site().expect("checked");
    let installed = konstruktor_core::tls::installed_certificate(&dir, &config);
    let now = konstruktor_core::tls::now_seconds();

    if json {
        return ui::emit_json(&serde_json::json!({
            "names": site.names(),
            "tls": site.tls,
            "certificate": installed.as_ref().and_then(|r| r.as_ref().ok()),
            "problem": installed.as_ref().and_then(|r| r.as_ref().err()),
        }));
    }
    ui::say("");
    let mut rows = vec![("names".to_string(), site.names().join(", "))];
    match &site.tls {
        PublicTls::Acme { email, ca, .. } => {
            rows.push((
                "from".into(),
                ca.clone()
                    .unwrap_or_else(|| "Let's Encrypt, obtained and renewed by the gateway".into()),
            ));
            rows.push(("account".into(), email.clone()));
        }
        PublicTls::Provided {
            cert_path,
            key_path,
        } => {
            rows.push(("certificate".into(), cert_path.clone()));
            rows.push(("key".into(), key_path.clone()));
            match installed {
                Some(Ok(info)) => {
                    rows.push(("valid".into(), format!("{} more days", info.days_left(now))))
                }
                Some(Err(why)) => rows.push(("problem".into(), why)),
                None => {}
            }
        }
    }
    ui::table(&rows);
    ui::say("");
    Ok(())
}

/// Copies the certificate in again and has the running gateway read it — `caddy reload`,
/// which keeps every connection it has.
async fn reload(target: &Target) -> Result<()> {
    let dir = target.resolve()?;
    let config = public_site(&dir)?;
    let info =
        konstruktor_core::tls::install_public_files(&dir, &config).map_err(|e| anyhow!("{e}"))?;
    reload_gateway(&dir, &config).await?;
    match info {
        Some(info) => ui::ok(&format!(
            "The gateway serves the certificate in {}, valid {} more days.",
            konstruktor_core::tls::FOLDER_DIR,
            info.days_left(konstruktor_core::tls::now_seconds())
        )),
        None => ui::ok("The gateway read its configuration again."),
    }
    Ok(())
}

async fn reload_gateway(
    dir: &std::path::Path,
    config: &konstruktor_core::config::hub::HubConfig,
) -> Result<()> {
    let args: Vec<String> = [
        "compose",
        "exec",
        "-T",
        config.gateway.host.as_str(),
        "caddy",
        "reload",
        "--config",
        "/etc/caddy/Caddyfile",
        "--adapter",
        "caddyfile",
        // An unchanged config is otherwise not loaded again, and with it the files it names.
        "--force",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    konstruktor_core::compose::run_streamed(dir, args, &|_| {})
        .await
        .map(|_| ())
        .map_err(|e| anyhow!("the gateway could not read it — is it running? {e}"))
}

/// Points the public site at other files: checked, written into the profile, the gateway's
/// files regenerated and the gateway recreated on them.
async fn set(args: SetArgs) -> Result<()> {
    let dir = args.target.resolve()?;
    let mut config = public_site(&dir)?;
    let absolute = |path: &str| -> Result<String> {
        Ok(std::fs::canonicalize(path)
            .with_context(|| format!("reading {path}"))?
            .to_string_lossy()
            .to_string())
    };
    let (cert_path, key_path) = (absolute(&args.cert)?, absolute(&args.key)?);
    let site = config.gateway.public.as_mut().expect("checked");
    site.tls = PublicTls::Provided {
        cert_path,
        key_path,
    };
    // Checked, and copied in, before the profile says so.
    let info = konstruktor_core::tls::install_public_files(&dir, &config)
        .map_err(|e| anyhow!("The certificate cannot be used: {e}."))?;
    let mut stored = profile::read_profile(&dir)?;
    stored.config = config.clone();
    profile::write_profile(&dir, &stored)?;
    profile::regenerate(&dir)?;
    konstruktor_core::compose::run_streamed(
        &dir,
        konstruktor_core::compose::up_service(&config.gateway.host),
        &|_| {},
    )
    .await
    .map_err(|e| anyhow!("the gateway did not start on it: {e}"))?;
    if let Some(info) = info {
        ui::ok(&format!(
            "The public site is served with it now, valid {} more days.",
            info.days_left(konstruktor_core::tls::now_seconds())
        ));
    }
    Ok(())
}
