//! The kinds of hub there are: a name for a set of services, so "a personal hub" is one
//! word rather than a list somebody has to know.
//!
//! A template only says which services a new hub starts with. Everything after that is the
//! same hub — `hub services add|remove` changes the set later, whatever it began as.
//!
//! Adding one is adding an entry to [`templates`]: the CLI's `--template`, its error for an
//! unknown one, `hub templates` and the installers all read this list.

use serde::Serialize;

use crate::catalog::{self, ServiceId};

/// The template a hub is created from when nobody names one.
pub const DEFAULT: &str = "default";

#[derive(Debug, Clone, Serialize)]
pub struct Template {
    /// What `--template` takes.
    pub id: &'static str,
    pub name: &'static str,
    /// One line: who this hub is for.
    pub description: &'static str,
    pub services: Vec<ServiceId>,
}

pub fn templates() -> Vec<Template> {
    vec![
        Template {
            id: DEFAULT,
            name: "Default",
            description:
                "A lab hub: images, workflows, apps, a knowledge graph and language models",
            // The catalog's own pre-ticks, so the two cannot drift.
            services: catalog::default_services(),
        },
        Template {
            id: "personal",
            name: "Personal",
            description:
                "A hub for one person: bank accounts, mail, documents and a location timeline",
            // Rekuest runs the periodic actions of Bank and Kuvert, so it comes with them.
            services: vec![
                ServiceId::Rekuest,
                ServiceId::Bank,
                ServiceId::Kuvert,
                ServiceId::Dokuments,
                ServiceId::Lokate,
            ],
        },
    ]
}

pub fn find(id: &str) -> Option<Template> {
    let id = id.trim();
    templates().into_iter().find(|template| template.id == id)
}

/// The ids, as an error or a help line lists them.
pub fn ids() -> Vec<&'static str> {
    templates().iter().map(|template| template.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support;

    #[test]
    fn the_default_template_is_the_catalogs_defaults() {
        let default = find(DEFAULT).expect("the default template exists");
        assert_eq!(default.services, catalog::default_services());
    }

    #[test]
    fn every_id_names_one_template() {
        let ids = ids();
        for id in &ids {
            assert_eq!(ids.iter().filter(|other| *other == id).count(), 1, "{id}");
            assert_eq!(
                *id,
                id.trim().to_ascii_lowercase(),
                "an id is typed on a command line"
            );
        }
        assert!(find("nope").is_none());
    }

    /// A template is a set of services a hub can actually run: each one offered, none
    /// twice, and Rekuest wherever something hooks into it.
    #[test]
    fn every_template_is_a_hub_that_runs() {
        let offered: Vec<ServiceId> = catalog::catalog()
            .into_iter()
            .filter(|service| service.emitted)
            .map(|service| service.id)
            .collect();
        for template in templates() {
            assert!(!template.services.is_empty(), "{}", template.id);
            for (i, service) in template.services.iter().enumerate() {
                assert!(offered.contains(service), "{}: {service:?}", template.id);
                assert!(
                    !template.services[..i].contains(service),
                    "{}: {service:?} twice",
                    template.id
                );
            }
            // By what the services say of themselves: nothing here lists the hooked ones.
            let said = support::said();
            if template
                .services
                .iter()
                .any(|s| said[s.as_str()].hooked_by_rekuest())
            {
                assert!(
                    template.services.contains(&ServiceId::Rekuest),
                    "{} needs rekuest",
                    template.id
                );
            }
        }
    }
}
