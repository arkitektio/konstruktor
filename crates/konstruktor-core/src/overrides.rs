//! What an operator set for one hub, kept apart from what is generated.
//!
//! A service's config is generated: written again from the profile on every update, so
//! that a release reads what it expects. Anything set by hand in the generated file is
//! gone the next time. But a hub has settings of its own that no generator knows — an
//! upload quota, the roles that may upload, a model to use — and those have to outlive
//! every update.
//!
//! They live in `overrides/<service>.yaml`, a file this installer never writes on its own.
//! When a service's image writes its config ([`crate::contract`]) the override is laid
//! over what it wrote, key by key — a mapping is merged into, anything else replaces what
//! was there — and the image judges the result as it does its own: an override that names
//! a key the release does not read stops there, by that key's name, instead of being
//! silently ignored.

use std::path::{Path, PathBuf};

use serde_norway::{Mapping, Value};

pub const OVERRIDES_DIR: &str = "overrides";

pub fn path(dir: &Path, service: &str) -> PathBuf {
    dir.join(OVERRIDES_DIR).join(format!("{service}.yaml"))
}

/// What the operator set for `service`: a mapping, or nothing.
pub fn read(dir: &Path, service: &str) -> Option<Value> {
    let text = std::fs::read_to_string(path(dir, service)).ok()?;
    serde_norway::from_str::<Value>(&text)
        .ok()
        .filter(Value::is_mapping)
}

/// Lays `over` on `base`: a mapping is merged into key by key, anything else replaces.
pub fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Mapping(base), Value::Mapping(over)) => {
            for (key, value) in over {
                match base.get_mut(key) {
                    Some(existing) => merge(existing, value),
                    None => {
                        base.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base, over) => *base = over.clone(),
    }
}

fn write(dir: &Path, service: &str, overrides: &Value) -> std::io::Result<()> {
    let target = path(dir, service);
    let empty = overrides.as_mapping().is_none_or(Mapping::is_empty);
    if empty {
        return match std::fs::remove_file(&target) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(target, crate::generate::dump(overrides))
}

/// Sets `key` — dotted, `datalayer.quotas.default` — to `value` in `service`'s overrides.
/// The value is YAML: `10`, `true`, `[a, b]`, `some text`.
pub fn set(dir: &Path, service: &str, key: &str, value: &str) -> std::io::Result<()> {
    let value: Value =
        serde_norway::from_str(value).unwrap_or_else(|_| Value::String(value.to_string()));
    let mut overrides = read(dir, service).unwrap_or_else(|| Value::Mapping(Mapping::new()));
    let mut at = &mut overrides;
    let parts: Vec<&str> = key.split('.').collect();
    for part in &parts[..parts.len() - 1] {
        let mapping = at
            .as_mapping_mut()
            .expect("only mappings are descended into");
        let next = mapping
            .entry(Value::String(part.to_string()))
            .or_insert_with(|| Value::Mapping(Mapping::new()));
        // A scalar in the way of a deeper key gives way to it.
        if !next.is_mapping() {
            *next = Value::Mapping(Mapping::new());
        }
        at = next;
    }
    at.as_mapping_mut()
        .expect("only mappings are descended into")
        .insert(Value::String(parts[parts.len() - 1].to_string()), value);
    write(dir, service, &overrides)
}

/// Takes `key` out of `service`'s overrides, and every mapping that leaves empty. Whether
/// it was there.
pub fn unset(dir: &Path, service: &str, key: &str) -> std::io::Result<bool> {
    fn remove(at: &mut Value, parts: &[&str]) -> bool {
        let Some(mapping) = at.as_mapping_mut() else {
            return false;
        };
        let name = Value::String(parts[0].to_string());
        if parts.len() == 1 {
            return mapping.remove(&name).is_some();
        }
        let Some(inner) = mapping.get_mut(&name) else {
            return false;
        };
        let removed = remove(inner, &parts[1..]);
        if inner.as_mapping().is_some_and(Mapping::is_empty) {
            mapping.remove(&name);
        }
        removed
    }
    let Some(mut overrides) = read(dir, service) else {
        return Ok(false);
    };
    let parts: Vec<&str> = key.split('.').collect();
    let removed = remove(&mut overrides, &parts);
    if removed {
        write(dir, service, &overrides)?;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "konstruktor-overrides-{tag}-{}-{}",
            std::process::id(),
            crate::lock::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn yaml(text: &str) -> Value {
        serde_norway::from_str(text).unwrap()
    }

    #[test]
    fn an_override_is_laid_over_what_is_generated_key_by_key() {
        let mut config = yaml(
            "django: {debug: false, hosts: ['*']}\ndatalayer: {host: rustfs, media: {bucket: m}}\n",
        );
        merge(
            &mut config,
            &yaml("django: {debug: true}\ndatalayer: {quotas: {default: 10}, media: {subpath: lab}}\nnew: 1\n"),
        );
        assert_eq!(
            config,
            yaml("django: {debug: true, hosts: ['*']}\ndatalayer: {host: rustfs, media: {bucket: m, subpath: lab}, quotas: {default: 10}}\nnew: 1\n")
        );
        // A list is a value: it replaces, it is not appended to.
        merge(&mut config, &yaml("django: {hosts: [lab.example]}"));
        assert_eq!(config["django"]["hosts"], yaml("[lab.example]"));
    }

    #[test]
    fn set_and_unset_keep_a_file_of_only_what_was_set() {
        let dir = scratch("set");
        set(&dir, "mikro", "datalayer.quotas.default", "10").unwrap();
        set(&dir, "mikro", "datalayer.upload_roles", "[admin, uploader]").unwrap();
        set(&dir, "mikro", "django.log_level", "DEBUG").unwrap();
        assert_eq!(
            read(&dir, "mikro").unwrap(),
            yaml("datalayer: {quotas: {default: 10}, upload_roles: [admin, uploader]}\ndjango: {log_level: DEBUG}\n")
        );
        assert_eq!(read(&dir, "fluss"), None);

        assert!(unset(&dir, "mikro", "datalayer.quotas.default").unwrap());
        assert!(!unset(&dir, "mikro", "datalayer.quotas.default").unwrap());
        assert_eq!(
            read(&dir, "mikro").unwrap(),
            yaml("datalayer: {upload_roles: [admin, uploader]}\ndjango: {log_level: DEBUG}\n")
        );
        // The last key gone, so is the file.
        unset(&dir, "mikro", "datalayer.upload_roles").unwrap();
        unset(&dir, "mikro", "django.log_level").unwrap();
        assert!(!path(&dir, "mikro").exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
