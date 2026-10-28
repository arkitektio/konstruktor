//! `konstruktor coord create`: the third creation path, next to `hub` and `engine`.
//!
//! A coordination server is what the other two authorize *against* — where users,
//! organizations and permissions live. `go.arkitekt.live` is one; this is for running your
//! own. It asks nothing about identifiers or coordination servers, because it *is* the
//! coordination server: there is nobody to introduce itself to. What it asks is the name it
//! is reached at, where that name's certificate comes from, and who administers it.

use std::net::IpAddr;

use anyhow::{bail, Context, Result};
use clap::Args;
use konstruktor_core::config::coord::{SignupPolicy, SmtpBlock};
use konstruktor_core::config::hub::PublicTls;
use konstruktor_core::coord::{create_coord, CoordAnswers};

use crate::ui;

#[derive(Args, Debug, Clone)]
pub struct CoordCreateArgs {
    /// Where the coordination server lives. Defaults to the current directory.
    pub dir: Option<String>,
    /// What it calls itself — on its landing page, in its well-known. Defaults to the
    /// folder's name.
    #[arg(long)]
    pub name: Option<String>,
    /// The DNS name it is reached at: what hubs and apps pass to `--server`. It has to
    /// resolve to this machine.
    #[arg(long)]
    pub domain: Option<String>,

    /// Obtain its certificate from Let's Encrypt, which writes to this address about it.
    /// Needs ports 80 and 443 reachable from the internet.
    #[arg(long, value_name = "EMAIL", conflicts_with_all = ["tls_cert", "plain_http"])]
    pub acme_email: Option<String>,
    /// Another ACME CA's directory: Let's Encrypt's staging
    /// (`https://acme-staging-v02.api.letsencrypt.org/directory`) while trying it out, or a
    /// CA of your own.
    #[arg(long, value_name = "URL", requires = "acme_email")]
    pub acme_ca: Option<String>,
    /// The root that CA's directory is served under, when it is not a public one.
    #[arg(long, value_name = "FILE", requires = "acme_email")]
    pub acme_ca_root: Option<String>,
    /// Serve a certificate obtained elsewhere (certbot, `tailscale cert`, your institution):
    /// the PEM file of the certificate and its chain. Renewed where it lies, and picked up by
    /// `konstruktor tls reload`.
    #[arg(
        long,
        value_name = "FILE",
        requires = "tls_key",
        conflicts_with = "plain_http"
    )]
    pub tls_cert: Option<String>,
    /// That certificate's private key, PEM.
    #[arg(long, value_name = "FILE", requires = "tls_cert")]
    pub tls_key: Option<String>,
    /// Serve plain HTTP, for a lab or a test on a network you trust. Nothing on the
    /// internet should rely on it: its tokens travel in the clear.
    #[arg(long)]
    pub plain_http: bool,

    /// The mesh's own DNS name — `mesh.coord.example.org` — served by the same gateway.
    /// Organizations then get a private network of their own, and hubs can join it. Also
    /// needs UDP port 3478 reachable.
    #[arg(long, value_name = "NAME", conflicts_with = "plain_http")]
    pub mesh_domain: Option<String>,
    /// What the mesh's machine names end in. Defaults to `ts.<domain>`.
    #[arg(long, value_name = "SUFFIX", requires = "mesh_domain")]
    pub mesh_dns_suffix: Option<String>,

    /// The port it answers plain HTTP on — and, with Let's Encrypt, where the CA asks.
    #[arg(long, default_value_t = 80)]
    pub http_port: u16,
    /// The port it answers HTTPS on.
    #[arg(long, default_value_t = 443)]
    pub https_port: u16,

    /// The first account, which everything else is administered through.
    #[arg(long, default_value = "admin")]
    pub admin: String,
    /// Left out, a strong one is generated and written to `secrets/access.json`.
    #[arg(long)]
    pub admin_password: Option<String>,
    /// The first account's email address.
    #[arg(long)]
    pub admin_email: Option<String>,
    /// The organization the administrator starts out in, which hubs are accepted into.
    /// Defaults to the coordination server's name.
    #[arg(long)]
    pub org: Option<String>,
    /// Who may create an account: `closed` (an administrator makes them, the default) or
    /// `open` (anybody who reaches it).
    #[arg(long, default_value = "closed", value_parser = parse_signup)]
    pub signup: SignupPolicy,

    /// Where it sends mail from: login codes, verification, invitations. Without it,
    /// accounts are made by an administrator.
    #[arg(long, value_name = "HOST")]
    pub smtp_host: Option<String>,
    #[arg(long, default_value_t = 587, requires = "smtp_host")]
    pub smtp_port: u16,
    #[arg(long, requires = "smtp_host")]
    pub smtp_user: Option<String>,
    /// A file holding the SMTP password, so it is in no shell history.
    #[arg(long, value_name = "FILE", requires = "smtp_host")]
    pub smtp_password_file: Option<String>,
    /// The `From` of what it sends.
    #[arg(long, value_name = "ADDRESS", requires = "smtp_host")]
    pub smtp_from: Option<String>,
    /// Speak SMTP without STARTTLS.
    #[arg(long, requires = "smtp_host")]
    pub smtp_no_tls: bool,

    /// This machine's public address, when it cannot see it itself (behind a NAT or a load
    /// balancer): what the DNS check compares the names against. Repeatable.
    #[arg(long = "public-ip", value_name = "ADDRESS")]
    pub public_ips: Vec<IpAddr>,
    /// Create it whatever its names resolve to.
    #[arg(long)]
    pub skip_dns_check: bool,
    /// Run one of its containers on another image: `lok=jhnnsrs/lok:8.0.0`. Repeatable.
    #[arg(long = "image", value_name = "SERVICE=IMAGE")]
    pub images: Vec<String>,

    /// Write the files, but do not start the containers.
    #[arg(long)]
    pub no_start: bool,
    /// Print what would be written and stop. Nothing is created.
    #[arg(long)]
    pub dry_run: bool,
    /// Answer every question with its default, failing on one that has none.
    #[arg(long, short)]
    pub yes: bool,
    /// Print what was created as JSON on stdout: where it is, and how to administer it.
    #[arg(long)]
    pub json: bool,
}

fn parse_signup(value: &str) -> Result<SignupPolicy, String> {
    match value {
        "closed" => Ok(SignupPolicy::Closed),
        "open" => Ok(SignupPolicy::Open),
        other => Err(format!("`{other}` — expected closed or open")),
    }
}

/// Where the certificate comes from, asked for when no flag said.
fn tls_of(args: &CoordCreateArgs) -> Result<Option<PublicTls>> {
    if args.plain_http {
        return Ok(None);
    }
    if let (Some(cert), Some(key)) = (&args.tls_cert, &args.tls_key) {
        let absolute = |path: &str| -> Result<String> {
            Ok(std::fs::canonicalize(path)
                .with_context(|| format!("reading {path}"))?
                .to_string_lossy()
                .to_string())
        };
        return Ok(Some(PublicTls::Provided {
            cert_path: absolute(cert)?,
            key_path: absolute(key)?,
        }));
    }
    let email = match &args.acme_email {
        Some(email) => email.clone(),
        None if ui::is_interactive() && !args.yes => {
            let choice = inquire::Select::new(
                "Where does its certificate come from?",
                vec![
                    "Let's Encrypt (ports 80 and 443 reach this machine)",
                    "A certificate I have (--tls-cert, --tls-key)",
                    "None: plain HTTP, for a lab or a test",
                ],
            )
            .prompt()?;
            if choice.starts_with("A certificate") {
                bail!("Pass the certificate's files: `--tls-cert FILE --tls-key FILE`.");
            }
            if choice.starts_with("None") {
                return Ok(None);
            }
            inquire::Text::new("An email address Let's Encrypt can write to about it:").prompt()?
        }
        None => bail!(
            "Where its certificate comes from: `--acme-email EMAIL` for Let's Encrypt, \
             `--tls-cert FILE --tls-key FILE` for one you have, or `--plain-http` for a lab."
        ),
    };
    let absolute_root = args
        .acme_ca_root
        .as_deref()
        .map(|path| {
            std::fs::canonicalize(path)
                .with_context(|| format!("reading {path}"))
                .map(|p| p.to_string_lossy().to_string())
        })
        .transpose()?;
    Ok(Some(PublicTls::Acme {
        email,
        ca: args.acme_ca.clone(),
        ca_root: absolute_root,
    }))
}

fn email_of(args: &CoordCreateArgs) -> Result<Option<SmtpBlock>> {
    let Some(host) = &args.smtp_host else {
        return Ok(None);
    };
    let password = match &args.smtp_password_file {
        Some(path) => std::fs::read_to_string(path)
            .with_context(|| format!("reading {path}"))?
            .trim()
            .to_string(),
        None => bail!("The SMTP server needs its password: `--smtp-password-file FILE`."),
    };
    let user = args
        .smtp_user
        .clone()
        .context("The SMTP server needs its user: `--smtp-user`.")?;
    Ok(Some(SmtpBlock {
        host: host.clone(),
        port: args.smtp_port,
        use_tls: !args.smtp_no_tls,
        from: args.smtp_from.clone().unwrap_or_else(|| user.clone()),
        user,
        password,
    }))
}

pub async fn run(args: CoordCreateArgs) -> Result<()> {
    ui::say("");
    ui::say(&format!("  {}", ui::bold("Creating a coordination server")));
    ui::say("");

    let requested = args.dir.as_deref().unwrap_or(".");
    // Resolved without creating it: nothing is written until everything was checked.
    let dir = std::path::absolute(requested).with_context(|| format!("resolving {requested}"))?;
    let dir = konstruktor_core::paths::canonical(&dir).unwrap_or(dir);

    if let Some(kind) = konstruktor_core::profile::holds_a_deployment(&dir) {
        bail!(
            "{} already holds a {}. Create the coordination server in an empty folder.",
            dir.display(),
            kind.label()
        );
    }

    let name = args
        .name
        .clone()
        .unwrap_or_else(|| konstruktor_core::compose::basename(&dir.to_string_lossy()));
    let domain = match &args.domain {
        Some(domain) => domain.clone(),
        // What a hub's containers reach this machine at: its address on this network.
        None if args.plain_http => konstruktor_core::hosts::discover(
            konstruktor_core::hosts::ReachPresetId::ThisNetwork,
        )
        .await
        .into_iter()
        .find(|host| host.kind != konstruktor_core::hosts::HostCategory::Loopback)
        .map(|host| host.host)
        .context(
            "This machine has no address on a network to serve plain HTTP at: name one with \
             `--domain`.",
        )?,
        None if ui::is_interactive() && !args.yes => {
            inquire::Text::new("The DNS name it is reached at (it has to resolve to this machine):")
                .prompt()?
        }
        None => bail!("The DNS name it is reached at: `--domain coord.example.org`."),
    };
    let tls = tls_of(&args)?;

    let answers = CoordAnswers {
        dir: dir.to_string_lossy().to_string(),
        name,
        domain,
        tls,
        http_port: args.http_port,
        https_port: args.https_port,
        mesh_domain: args.mesh_domain.clone(),
        mesh_dns_suffix: args.mesh_dns_suffix.clone(),
        admin: args.admin.clone(),
        admin_password: args.admin_password.clone(),
        admin_email: args.admin_email.clone(),
        organization: args.org.clone(),
        email: email_of(&args)?,
        signup: args.signup,
        public_ips: args.public_ips.clone(),
        skip_dns_check: args.skip_dns_check,
        images: crate::create::parse_images(&args.images)?,
        default_images: crate::create::images_of_the_environment()?,
        storage: Default::default(),
        start: !args.no_start,
    };
    konstruktor_core::coord::validate(&answers)?;

    let certificate = match &answers.tls {
        Some(PublicTls::Acme { ca: Some(ca), .. }) => format!("ACME, from {ca}"),
        Some(PublicTls::Acme { .. }) => "Let's Encrypt".to_string(),
        Some(PublicTls::Provided { cert_path, .. }) => cert_path.clone(),
        None => "none — plain HTTP".to_string(),
    };
    let mut rows = vec![
        ("folder".into(), answers.dir.clone()),
        ("name".into(), answers.name.clone()),
        ("address".into(), konstruktor_core::coord::origin(&answers)),
        ("certificate".into(), certificate),
    ];
    if let Some(mesh) = &answers.mesh_domain {
        rows.push(("mesh".into(), format!("https://{mesh}")));
    }
    rows.push(("admin".into(), answers.admin.clone()));
    rows.push((
        "signup".into(),
        match answers.signup {
            SignupPolicy::Closed => "closed — once Lok reads `account.allow_signup`".into(),
            SignupPolicy::Open => "open to anybody".into(),
        },
    ));
    ui::table(&rows);
    ui::say("");

    if args.dry_run {
        let config = konstruktor_core::coord::build(&answers, "dry-run")?;
        let files = konstruktor_core::generate::generate_hub_files(
            &config,
            &Default::default(),
            &Default::default(),
        );
        ui::step("Would write (Lok's image is not asked, so its start command is left out):");
        for file in files.keys() {
            ui::step(&ui::dim(file));
        }
        ui::say("");
        return Ok(());
    }

    let created = create_coord(&answers, &|event| crate::create::report(event, false)).await?;

    let unready: Vec<&konstruktor_core::ready::Endpoint> =
        created.endpoints.iter().filter(|e| !e.ready).collect();
    ui::say("");
    for warning in created.endpoints.iter().filter(|e| e.warning) {
        ui::warn(&ui::bold(warning.detail.as_deref().unwrap_or_default()));
        ui::say("");
    }
    if created.endpoints.is_empty() {
        ui::ok(&format!(
            "Coordination server written to {} — `konstruktor up` starts it.",
            created.path.display()
        ));
    } else if unready.is_empty() {
        ui::ok(&format!(
            "Coordination server running at {}",
            konstruktor_core::coord::origin(&answers)
        ));
    } else {
        ui::warn("The coordination server was started, but not everything answers yet:");
        for endpoint in &unready {
            ui::step(&format!(
                "{}  {}",
                endpoint.name,
                endpoint
                    .detail
                    .clone()
                    .unwrap_or_else(|| match endpoint.status {
                        Some(status) => format!("answers {status}"),
                        None => "does not answer".into(),
                    })
            ));
        }
        ui::step(
            "`konstruktor wait` waits longer; `konstruktor logs gateway` shows how the \
             certificate is coming along.",
        );
    }
    if let Some(info) = &created.certificate {
        let days = info.days_left(konstruktor_core::tls::now_seconds());
        ui::step(&format!("The certificate is valid for {days} more days."));
    }
    ui::say("");
    ui::step(&format!(
        "Log in at {} as {} — the password is in {}.",
        konstruktor_core::coord::origin(&answers),
        answers.admin,
        created
            .path
            .join(konstruktor_core::generate::lok::ACCESS_FILE)
            .display()
    ));
    ui::step(&format!(
        "Connect a hub to it: `konstruktor hub create --server {}`.",
        konstruktor_core::coord::origin(&answers)
    ));
    ui::say("");

    if args.json {
        let access_file = created
            .path
            .join(konstruktor_core::generate::lok::ACCESS_FILE);
        let access: serde_json::Value = std::fs::read_to_string(&access_file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(serde_json::Value::Null);
        ui::emit_json(&serde_json::json!({
            "path": created.path,
            "record": created.record,
            "access": access,
            "certificate": created.certificate,
            "endpoints": created.endpoints,
        }))?;
    } else {
        println!("{}", created.path.to_string_lossy());
    }
    Ok(())
}
