//! A service's databases, by the names it asks for them under.
//!
//! A service says which databases it keeps (`needs.databases`, `main` unless it says
//! otherwise); the hub calls each `<service>_<name>`, creates it, and hands it back to the
//! service under the name it asked by. The names end up in SQL unquoted — in the database
//! image's init script and in `ensure_database` — so what cannot be written that way is
//! refused where a description is taken in, before anything is provided.

use konstruktor_core::catalog::ServiceId;
use konstruktor_core::config::hub::{
    build_hub_config, database_name, HubConfig, HubConfigOptions, InvalidDatabase,
    LOCAL_COORD_SERVER,
};
use konstruktor_core::contract::{Description, Needs, Said};
use konstruktor_core::generate::{generate_hub_files, IssuedIdentity};
use serde_norway::Value;

mod support;

fn example() -> ServiceId {
    ServiceId::named("example")
}

/// `example`, asking for these databases.
fn asking_for(databases: &[&str]) -> Description {
    let mut said = support::example();
    said.needs.databases = databases.iter().map(|name| name.to_string()).collect();
    said
}

/// A hub of Mikro and `example`, nothing provided yet, and what their images say.
fn hub_with(example_says: Description) -> (HubConfig, Said) {
    let mut config = build_hub_config(&HubConfigOptions {
        services: Some(vec![ServiceId::Mikro, example()]),
        ..Default::default()
    });
    config.set_service_image("example", "example:1");
    let mut said = support::said();
    said.insert("example".to_string(), example_says);
    (config, said)
}

#[test]
fn a_database_is_called_after_its_service_and_its_name() {
    assert_eq!(database_name("mikro", "main").as_deref(), Ok("mikro_main"));
    assert_eq!(
        database_name("omero_ark", "main").as_deref(),
        Ok("omero_ark_main")
    );
    assert_eq!(
        database_name("example", "events_2").as_deref(),
        Ok("example_events_2")
    );
}

/// Left unsaid, a service has the one database every service had: `main`.
#[test]
fn a_service_that_says_nothing_has_a_main_database() {
    let said: Description = serde_json::from_str(
        r#"{"contract": 2, "name": "example", "identifier": "org.example", "needs": {}}"#,
    )
    .expect("a description");
    assert_eq!(said.needs.databases, ["main"]);
    assert_eq!(Needs::default().databases, ["main"]);

    let (mut config, said) = hub_with(support::example());
    assert_eq!(config.databases_can_be_provided(&said), Ok(()));
    config.provide(&said);
    assert_eq!(
        config.service(example()).database("main"),
        Some("example_main")
    );
    assert_eq!(
        config.service(ServiceId::Mikro).database("main"),
        Some("mikro_main")
    );
}

/// Both are created, and both are handed to the service under the names it gave.
#[test]
fn a_service_declaring_two_databases_gets_both() {
    let (mut config, said) = hub_with(asking_for(&["main", "events"]));
    assert_eq!(config.databases_can_be_provided(&said), Ok(()));
    config.provide(&said);

    let block = config.service(example());
    assert_eq!(block.database("main"), Some("example_main"));
    assert_eq!(block.database("events"), Some("example_events"));

    // Both in the list the database is initialised with, after the services before it.
    let files = generate_hub_files(&config, &IssuedIdentity::default(), &said);
    let compose: Value = serde_norway::from_str(&files["docker-compose.yaml"]).unwrap();
    assert_eq!(
        compose["services"]["db"]["environment"]["POSTGRES_MULTIPLE_DATABASES"].as_str(),
        Some("rekuest_main,mikro_main,example_events,example_main")
    );
    assert_eq!(
        config.provisioned_databases(),
        [
            "rekuest_main",
            "mikro_main",
            "example_events",
            "example_main"
        ]
    );

    // And both in what the service is told, each a whole database of its own.
    let facts =
        konstruktor_core::contract::facts(&config, example(), &IssuedIdentity::default(), &said);
    let told = facts["databases"].as_mapping().expect("by name");
    assert_eq!(told.len(), 2);
    for (name, database) in [("main", "example_main"), ("events", "example_events")] {
        let entry = &facts["databases"][name];
        assert_eq!(entry["name"].as_str(), Some(database), "{name}");
        assert_eq!(entry["host"].as_str(), Some("db"), "{name}");
        assert_eq!(entry["port"].as_u64(), Some(5432), "{name}");
        assert_eq!(
            entry["username"].as_str(),
            Some(config.db.postgres_user.as_str())
        );
        assert_eq!(
            entry["password"].as_str(),
            Some(config.db.postgres_password.as_str())
        );
    }
    // The one database there used to be is not told any more.
    assert!(facts.get("database").is_none());

    // The profile holds them by name, and reads them back.
    let text = serde_norway::to_string(&config).unwrap();
    let value: Value = serde_norway::from_str(&text).unwrap();
    assert_eq!(
        value["services"]["example"]["databases"]["events"].as_str(),
        Some("example_events")
    );
    let back: HubConfig = serde_norway::from_str(&text).unwrap();
    assert_eq!(back, config);
}

/// A service that keeps nothing in Postgres gets nothing there, and waits on no database.
#[test]
fn a_service_declaring_no_database_gets_none() {
    let (mut config, said) = hub_with(asking_for(&[]));
    config.provide(&said);
    assert!(config.service(example()).databases.is_empty());
    assert_eq!(
        config.provisioned_databases(),
        ["rekuest_main", "mikro_main"]
    );
    let facts =
        konstruktor_core::contract::facts(&config, example(), &IssuedIdentity::default(), &said);
    assert!(facts["databases"]
        .as_mapping()
        .is_some_and(|databases| databases.is_empty()));
}

/// A database holds data: one a release stops declaring stays the service's.
#[test]
fn a_database_once_provided_stays() {
    let (mut config, said) = hub_with(asking_for(&["main", "events"]));
    config.provide(&said);

    let (_, narrower) = hub_with(asking_for(&["main"]));
    assert!(!config.provide(&narrower), "nothing changes");
    assert_eq!(
        config.service(example()).database("events"),
        Some("example_events")
    );
    assert!(config
        .provisioned_databases()
        .contains(&"example_events".to_string()));
}

/// Any image can describe itself, so a name is checked again here: what Postgres would
/// not take as written is refused, naming the service and the name, and nothing of the
/// description is provided.
#[test]
fn a_name_postgres_would_not_take_unquoted_is_refused() {
    for bad in [
        "Main",
        "2nd",
        "_main",
        "has space",
        "events-log",
        "x; DROP DATABASE mikro_main",
        "",
    ] {
        let (config, said) = hub_with(asking_for(&["main", bad]));
        let refused = config.databases_can_be_provided(&said).unwrap_err();
        assert_eq!(
            refused,
            InvalidDatabase::Name {
                service: "example".into(),
                name: bad.into()
            },
            "{bad}"
        );
        let why = konstruktor_core::contract::acceptable(&config, &said).unwrap_err();
        assert!(why.contains("`example`"), "{why}");
        assert!(why.contains(&format!("`{bad}`")), "{why}");
    }
}

#[test]
fn a_database_named_twice_is_refused() {
    let (config, said) = hub_with(asking_for(&["main", "events", "main"]));
    let refused = config.databases_can_be_provided(&said).unwrap_err();
    assert_eq!(
        refused,
        InvalidDatabase::Twice {
            service: "example".into(),
            name: "main".into()
        }
    );
    assert!(refused.to_string().contains("twice"), "{refused}");
}

/// Postgres keeps 63 bytes of a name and drops the rest without a word, which would let
/// two databases become one: the whole name, the service's part included, has to fit.
#[test]
fn a_database_whose_name_would_not_fit_is_refused() {
    // `example_` is eight bytes: 55 more fit, 56 do not.
    let fits = "a".repeat(55);
    let (config, said) = hub_with(asking_for(&[&fits]));
    assert_eq!(config.databases_can_be_provided(&said), Ok(()));
    assert_eq!(database_name("example", &fits).unwrap().len(), 63);

    let too_long = "a".repeat(56);
    let (config, said) = hub_with(asking_for(&[&too_long]));
    let refused = config.databases_can_be_provided(&said).unwrap_err();
    assert!(
        matches!(&refused, InvalidDatabase::TooLong { service, name, .. }
            if service == "example" && *name == too_long),
        "{refused}"
    );
    assert!(refused.to_string().contains("63"), "{refused}");
}

/// Two services can still arrive at one name — `a` asking for `b_main`, `a_b` asking for
/// `main` — and the second to ask is refused rather than handed the first one's rows.
#[test]
fn a_database_that_is_another_services_is_refused() {
    let wide = ServiceId::named("example_events");
    let mut config = build_hub_config(&HubConfigOptions {
        services: Some(vec![example(), wide]),
        ..Default::default()
    });
    config.set_service_image("example", "example:1");
    config.set_service_image("example_events", "example-events:1");
    let mut wide_says = asking_for(&["main"]);
    wide_says.name = "example_events".into();
    let said: Said = [
        ("example".to_string(), asking_for(&["events_main"])),
        ("example_events".to_string(), wide_says),
    ]
    .into();

    let refused = config.databases_can_be_provided(&said).unwrap_err();
    assert_eq!(
        refused,
        InvalidDatabase::Taken {
            service: "example_events".into(),
            name: "main".into(),
            database: "example_events_main".into(),
            other: "example".into(),
        }
    );

    // The coordination server's database is one of the hub's too.
    let mut own = build_hub_config(&HubConfigOptions {
        coord_server: LOCAL_COORD_SERVER.into(),
        services: Some(vec![ServiceId::named("lok_main_twin")]),
        ..Default::default()
    });
    assert_eq!(own.running_lok().unwrap().db, "lok_main");
    own.set_service_image("lok_main_twin", "twin:1");
    assert_eq!(own.databases_can_be_provided(&Said::new()), Ok(()));
}

/// An image from before databases had names says `database: true`. That is not a way of
/// saying `databases: [main]`: the block is closed, and such a description is not read.
#[test]
fn an_image_that_still_says_database_is_not_read() {
    let old = r#"{"contract": 2, "name": "example", "identifier": "org.example",
                  "needs": {"database": true, "redis": true}}"#;
    let refused = serde_json::from_str::<Description>(old).unwrap_err();
    assert!(refused.to_string().contains("database"), "{refused}");
}

/// A service's name is the first half of every one of its databases' names, so it is held
/// to the same rule: no hyphen.
#[test]
fn a_hyphenated_service_name_is_refused() {
    assert!(ServiceId::parse("omero-ark").is_err());
    assert_eq!(
        ServiceId::parse("omero_ark").map(ServiceId::as_str),
        Ok("omero_ark")
    );
    // And one that reached a block anyway — a profile edited by hand — makes no database.
    assert!(database_name("omero-ark", "main").is_err());
}
