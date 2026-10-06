use konstruktor_core::catalog::ServiceId;
use konstruktor_core::config::hub::{HubConfig, HubConfigOptions};
use konstruktor_core::contract::{Description, Said};
use konstruktor_core::generate::compose::build_compose;
use serde_norway::Value;

mod support;

// How a service is started is its image's to say. The compose file is written from what the
// images answered, which is why they are asked before anything of a hub is written.

fn hub() -> HubConfig {
    support::hub(&HubConfigOptions {
        device_id: "device".into(),
        services: Some(vec![ServiceId::Rekuest, ServiceId::Mikro]),
        ..Default::default()
    })
}

fn mikro_says(serve: &[&str], debug: &[&str]) -> Said {
    let words = |parts: &[&str]| parts.iter().map(|part| part.to_string()).collect();
    [(
        "mikro".to_string(),
        Description {
            contract: 2,
            name: "mikro".into(),
            identifier: "live.arkitekt.mikro".into(),
            serve: words(serve),
            debug: words(debug),
            ..Description::default()
        },
    )]
    .into()
}

fn command_of(config: &HubConfig, said: &Said, service: &str) -> Option<Vec<String>> {
    let compose = build_compose(config, &config.enabled_services(), said);
    let command = compose["services"][service].get("command")?;
    Some(
        command
            .as_sequence()
            .expect("an argument list, not a shell line")
            .iter()
            .map(|part| part.as_str().expect("a string").to_string())
            .collect(),
    )
}

#[test]
fn a_service_is_started_with_the_command_its_image_names() {
    let said = mikro_says(&["bash", "serve.sh"], &["bash", "dev.sh"]);
    assert_eq!(
        command_of(&hub(), &said, "mikro"),
        Some(vec!["bash".to_string(), "serve.sh".to_string()])
    );
}

#[test]
fn in_debug_it_is_started_with_the_one_it_names_for_that() {
    let mut config = hub();
    config
        .service_mut(konstruktor_core::catalog::ServiceId::Mikro)
        .debug = true;
    let said = mikro_says(&["bash", "serve.sh"], &["bash", "dev.sh"]);
    assert_eq!(
        command_of(&config, &said, "mikro"),
        Some(vec!["bash".to_string(), "dev.sh".to_string()])
    );
}

/// Nothing is assumed for an image that was not asked: no command is written for it. Its own
/// only says what it is, which is why creating a hub asks every image first and refuses to
/// write anything when one does not answer.
#[test]
fn a_service_whose_image_was_not_asked_gets_no_command() {
    let said = mikro_says(&["bash", "serve.sh"], &["bash", "dev.sh"]);
    assert_eq!(command_of(&hub(), &said, "rekuest"), None);
    assert_eq!(command_of(&hub(), &Said::new(), "mikro"), None);
    // The same for a mode the image names nothing for.
    let mut config = hub();
    config
        .service_mut(konstruktor_core::catalog::ServiceId::Mikro)
        .debug = true;
    assert_eq!(
        command_of(&config, &mikro_says(&["bash", "serve.sh"], &[]), "mikro"),
        None
    );
    assert!(matches!(
        build_compose(&config, &config.enabled_services(), &Said::new())["services"]["mikro"],
        Value::Mapping(_)
    ));
}
