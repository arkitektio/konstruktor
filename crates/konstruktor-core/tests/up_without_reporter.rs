//! A hub whose health reporter has no image still starts — without the reporter.

use std::path::PathBuf;

use konstruktor_core::compose::up_in;
use konstruktor_core::config::hub::{build_hub_config, HubConfigOptions, ReporterBlock};
use konstruktor_core::generate::write::write_generated_files;
use konstruktor_core::generate::{generate_hub_files, IssuedIdentity};
use konstruktor_core::profile::{hub_profile, write_profile};

fn docker_available() -> bool {
    konstruktor_core::docker::command()
        .args(["version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn hub_with_reporter(image: &str, label: &str) -> PathBuf {
    let mut config = build_hub_config(&HubConfigOptions {
        device_id: "device".into(),
        coord_server: "go.arkitekt.live".into(),
        ..Default::default()
    });
    config.reporter = Some(ReporterBlock {
        image: image.into(),
        ..ReporterBlock::default()
    });

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(label);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    write_profile(&dir, &hub_profile(config.clone())).expect("profile");
    write_generated_files(&dir, &generate_hub_files(&config, &IssuedIdentity::default()))
        .expect("files");
    dir
}

/// No such image anywhere: every other service is named, the reporter is not, and the
/// reason says which image was missing.
#[tokio::test]
async fn an_unpullable_reporter_image_is_left_out_not_fatal() {
    if !docker_available() {
        eprintln!("skipping: no docker on this machine");
        return;
    }
    let image = "jhnnsrs/konstruktor-test-image-that-does-not-exist:never";
    let dir = hub_with_reporter(image, "up-without-reporter");

    let (args, left_out) = up_in(&dir).await;

    assert_eq!(&args[..3], ["compose", "up", "-d"]);
    assert!(!args.iter().any(|a| a == "reporter"), "{args:?}");
    assert!(args.iter().any(|a| a == "gateway"), "{args:?}");
    assert!(left_out.expect("a reason").contains(image));
    std::fs::remove_dir_all(&dir).ok();
}

/// A hub without a reporter is started exactly as it always was.
#[tokio::test]
async fn a_hub_without_a_reporter_is_plain_up() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("up-plain");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let config = build_hub_config(&HubConfigOptions::default());
    write_profile(&dir, &hub_profile(config)).expect("profile");

    let (args, left_out) = up_in(&dir).await;
    assert_eq!(args, ["compose", "up", "-d"]);
    assert!(left_out.is_none());
    std::fs::remove_dir_all(&dir).ok();
}
