//! Taking the credentials out of text that is about to leave this machine.
//!
//! A hub's logs are the most useful thing a bug report can carry and the most dangerous:
//! Django prints its settings on a crash, Postgres echoes its connection string, and a
//! stack trace through the datalayer carries the MinIO keys with it. So nothing is
//! published from a deployment folder without going through here first.
//!
//! The approach is deliberately *not* "look for things that look like secrets". A hub's
//! secrets are known exactly — they are written in its own files — so they are collected
//! from those files and matched literally, which cannot miss one and cannot be fooled by
//! a value that happens to look ordinary. The pattern scrubs at the end are for the
//! second class of secret: the ones a *service* minted at runtime, which are in no file
//! here — a bearer token, a JWT, a private key block.

use std::collections::BTreeSet;
use std::path::Path;

use serde_norway::Value;

/// One value that must never appear in published text, and the key it was found under —
/// the key is what the marker names, so a reader knows what was taken out.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Secret {
    pub key: String,
    pub value: String,
}

/// What a redaction did, so a preview can say "14 values were removed" rather than
/// leaving the user to take it on trust.
#[derive(Debug, Clone)]
pub struct Redaction {
    pub text: String,
    /// How many replacements were made. Occurrences, not distinct values: it is a
    /// number for a person deciding whether to trust the preview, and "3 things were
    /// taken out of this log" is what that person is counting.
    pub removed: usize,
}

/// Key names that make a value a credential whatever it looks like.
const SECRET_KEYS: [&str; 10] = [
    "password", "secret", "token", "auth_key", "access_key", "private", "credential",
    "passphrase", "salt", "fernet",
];

/// A value under a secret-sounding key is a credential at almost any length: a hub whose
/// database password is `omero` is exactly the one that must not have it published. Only
/// the values too short to match anything meaningfully are skipped, plus the placeholders
/// below — a log with every `true` replaced would be unreadable and protect nothing.
const KEYED_MIN: usize = 4;
/// Words that are a *setting*, never a credential, however they are keyed.
const PLACEHOLDERS: [&str; 6] = ["none", "null", "true", "false", "auto", "unset"];
/// A value under an ordinary key has to *look* generated to be taken for a credential.
/// `generate_alpha_numeric_string(40)` is what the config is full of.
const SHAPED_MIN: usize = 20;

fn key_is_secret(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    // `key` alone is too common to use whole — `secret_key` and `auth_key` are caught by
    // their own entries, while `db_key` or `key` on its own is usually a name.
    SECRET_KEYS.iter().any(|needle| lower.contains(needle))
}

/// The shape of a generated credential: one long run of letters and digits, with both.
///
/// Deliberately narrow. Anything with a dot, slash, colon or space is a hostname, an
/// image reference, a URL or a sentence, and redacting those turns a log into a puzzle —
/// the point of this branch is to catch a `generate_alpha_numeric_string` sitting under
/// a key nobody thought to name `password`.
fn looks_generated(value: &str) -> bool {
    value.len() >= SHAPED_MIN
        && value.chars().all(|c| c.is_ascii_alphanumeric())
        && value.chars().any(|c| c.is_ascii_digit())
        && value.chars().any(|c| c.is_ascii_alphabetic())
}

/// Every credential in one parsed document, wherever it sits in it.
pub fn secrets_in(document: &Value) -> Vec<Secret> {
    let mut found = BTreeSet::new();
    walk(document, "", &mut found);
    found.into_iter().collect()
}

fn walk(value: &Value, key: &str, found: &mut BTreeSet<Secret>) {
    match value {
        Value::String(text) => {
            let keyed = key_is_secret(key)
                && text.len() >= KEYED_MIN
                && !PLACEHOLDERS.contains(&text.to_ascii_lowercase().as_str());
            if keyed || looks_generated(text) {
                found.insert(Secret {
                    key: if key.is_empty() { "value".into() } else { key.into() },
                    value: text.clone(),
                });
            }
        }
        Value::Sequence(items) => {
            for item in items {
                // A list keeps its parent's key: `allowed_hosts` entries are still
                // `allowed_hosts`, and that is what a marker should say.
                walk(item, key, found);
            }
        }
        Value::Mapping(map) => {
            for (name, child) in map {
                let name = name.as_str().unwrap_or(key);
                walk(child, name, found);
            }
        }
        _ => {}
    }
}

/// Every credential a deployment folder holds.
///
/// The profile, the generated service configs, and the compose file — which is
/// hand-editable in this app, and in the wild carries inline `POSTGRES_PASSWORD`s the
/// profile has never seen. Then the files that are nothing but credentials: the grant
/// (`hub_credentials.json`), a reporter state kept in the folder (`reporter.json`; the
/// containerised reporter keeps its own in a volume, out of reach here), the mesh key
/// (`mesh.env`) and the key files under `secrets/`. Anything unreadable is skipped rather than failing the report: a folder missing its
/// configs is exactly the broken state somebody is trying to report.
pub fn secrets_in_deployment(dir: &Path) -> Vec<Secret> {
    let mut found = BTreeSet::new();

    let mut read = |path: &Path| {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(document) = serde_norway::from_str::<Value>(&text) {
                walk(&document, "", &mut found);
            }
        }
    };

    read(&crate::profile::profile_path(dir));
    for name in ["docker-compose.yaml", "docker-compose.yml"] {
        read(&dir.join(name));
    }
    if let Ok(entries) = std::fs::read_dir(dir.join("configs")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "yaml" || e == "yml") {
                read(&path);
            }
        }
    }

    // The grant (access and refresh tokens, client secrets, the mesh key) and the
    // reporter's rotated refresh token: JSON, walked like the rest.
    for name in [
        crate::credentials::CREDENTIALS_FILENAME,
        crate::hubhealth::STATE_FILENAME,
    ] {
        if let Ok(text) = std::fs::read_to_string(dir.join(name)) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Ok(document) = serde_norway::to_value(json) {
                    walk(&document, "", &mut found);
                }
            }
        }
    }

    // Files that hold nothing *but* secrets, taken whole whatever they look like: a
    // `tskey-auth-…` has dashes, so it would not pass for generated, and `TS_AUTHKEY`
    // names no secret-sounding key.
    if let Ok(text) = std::fs::read_to_string(dir.join(crate::config::mesh::MESH_ENV_FILE)) {
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                insert_whole(&mut found, key.trim(), value.trim());
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(dir.join("secrets")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(text) = std::fs::read_to_string(&path) {
                let name = format!("secrets/{}", entry.file_name().to_string_lossy());
                insert_whole(&mut found, &name, text.trim());
            }
        }
    }

    found.into_iter().collect()
}

/// A value from a file that holds only secrets: taken as long as it is not empty.
fn insert_whole(found: &mut BTreeSet<Secret>, key: &str, value: &str) {
    let value = value.trim_matches(|c| c == '"' || c == '\'');
    if value.len() >= KEYED_MIN {
        found.insert(Secret {
            key: key.to_string(),
            value: value.to_string(),
        });
    }
}

/// Replace every known credential, then everything that looks like one a service minted
/// at runtime.
pub fn redact(text: &str, secrets: &[Secret]) -> Redaction {
    // Longest first: a short secret that happens to be a prefix of a long one must not
    // chop the long one into a marker plus a readable tail.
    let mut ordered: Vec<&Secret> = secrets.iter().collect();
    ordered.sort_by(|a, b| b.value.len().cmp(&a.value.len()));

    let mut out = text.to_string();
    let mut removed = 0;
    for secret in ordered {
        if secret.value.is_empty() || !out.contains(&secret.value) {
            continue;
        }
        // A short password is often an ordinary word — `admin`, `omero` — and replacing
        // it wherever those letters occur turns `ensureadmin` into `ensure[redacted]`
        // and a log into a rebus. Short values are only taken as whole words; a long
        // generated one cannot collide with anything, so it goes wherever it appears.
        let (replaced, count) = if secret.value.len() < WHOLE_WORD_BELOW {
            replace_whole_words(&out, &secret.value, &marker(&secret.key))
        } else {
            (
                out.replace(&secret.value, &marker(&secret.key)),
                out.matches(&secret.value).count(),
            )
        };
        removed += count;
        out = replaced;
    }

    let (out, patterned) = scrub_patterns(&out);
    Redaction {
        text: out,
        removed: removed + patterned,
    }
}

/// Below this, a value is matched as a whole word only — see `redact`.
const WHOLE_WORD_BELOW: usize = 12;

/// `text.replace`, but only where the needle is not glued to a letter or digit on either
/// side. Written out because a boundary-aware replace is the one thing `str::replace`
/// cannot do, and pulling in a regex engine to express `\b` would be a poor trade.
fn replace_whole_words(text: &str, needle: &str, with: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut replaced = 0;
    let mut rest = text;
    while let Some(at) = rest.find(needle) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + needle.len()..].chars().next();
        let glued = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
        out.push_str(&rest[..at]);
        if glued(before) || glued(after) {
            out.push_str(needle);
        } else {
            out.push_str(with);
            replaced += 1;
        }
        rest = &rest[at + needle.len()..];
    }
    out.push_str(rest);
    (out, replaced)
}

fn marker(key: &str) -> String {
    format!("[redacted: {key}]")
}

/// The secrets no file here has ever seen: keys and tokens a service minted while it ran.
///
/// Only shapes that cannot be anything else. A log line is prose, and a scrub that
/// guesses turns the report it was meant to protect into something nobody can read.
fn scrub_patterns(text: &str) -> (String, usize) {
    let mut removed = 0;
    let mut out = String::with_capacity(text.len());

    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches('\n');
        let newline = &line[body.len()..];

        // A key block, however it is wrapped. Everything from the marker to the end of
        // the line goes: on a one-line block that is the key itself, and on a wrapped one
        // the base64 body lines that follow are caught below.
        if let Some(at) = body.find("-----BEGIN") {
            out.push_str(&body[..at]);
            out.push_str(&marker("private key"));
            out.push_str(newline);
            removed += 1;
            continue;
        }

        for word in body.split_inclusive(is_word_break) {
            let trimmed = word.trim_end_matches(is_word_break);
            if looks_like_token(trimmed) {
                out.push_str(&marker("token"));
                removed += 1;
            } else {
                out.push_str(trimmed);
            }
            out.push_str(&word[trimmed.len()..]);
        }
        out.push_str(newline);
    }

    (out, removed)
}

fn is_word_break(c: char) -> bool {
    c.is_whitespace() || matches!(c, ',' | ';' | '"' | '\'' | ')' | ']' | '}')
}

/// A JWT, or a run of base64 long enough that it can only be key material.
///
/// Three dot-separated base64url runs is a JWT and nothing else. A bare run has to be
/// long enough *and* mixed case: that is what separates the body line of a PEM block from
/// a container id, an image digest or a long identifier, all of which are one case and
/// all of which a maintainer needs to be able to read.
fn looks_like_token(word: &str) -> bool {
    let body = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    if body.starts_with("eyJ") && body.matches('.').count() == 2 {
        return body
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(is_base64url));
    }
    body.len() >= BASE64_RUN_MIN
        && body.chars().all(is_base64)
        && body.chars().any(|c| c.is_ascii_uppercase())
        && body.chars().any(|c| c.is_ascii_lowercase())
        && body.chars().any(|c| c.is_ascii_digit())
}

/// Long enough that nothing which is merely long — a class name, a bucket path, a digest
/// — reaches it. A PEM body line is 64.
const BASE64_RUN_MIN: usize = 40;

fn is_base64url(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '='
}

fn is_base64(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '-' | '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(yaml: &str) -> Value {
        serde_norway::from_str(yaml).unwrap()
    }

    #[test]
    fn collects_by_key_and_by_shape() {
        let found = secrets_in(&doc(
            "db:\n  postgres_password: hunter2hunter2\n  postgres_user: flakyviolet\n\
             minio:\n  access_key: Xk39fjA02mfkD91ksla02mfkD91ksla0\n",
        ));
        let values: Vec<&str> = found.iter().map(|s| s.value.as_str()).collect();
        // Named as a password, so its length is all that matters.
        assert!(values.contains(&"hunter2hunter2"));
        // Generated-looking, and would be a credential under any key.
        assert!(values.contains(&"Xk39fjA02mfkD91ksla02mfkD91ksla0"));
        // A short ordinary value under an ordinary key is left alone.
        assert!(!values.contains(&"flakyviolet"));
    }

    /// The failure that would matter: a hostname or an image turned into `[redacted]`
    /// leaves a log nobody can read, and reads as a bug in the app.
    #[test]
    fn leaves_ordinary_long_values_alone() {
        let found = secrets_in(&doc(
            "image: jhnnsrs/rekuest:next\nhost: jhnnsrs-lab.hyena-sole.ts.net\n\
             url: https://go.arkitekt.live/lok\n",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn replaces_every_occurrence_and_names_the_key() {
        let secrets = vec![Secret {
            key: "postgres_password".into(),
            value: "8b13613b7518e1b293133d68622635ad".into(),
        }];
        let out = redact(
            "connecting as omero:8b13613b7518e1b293133d68622635ad@db\n\
             PGPASSWORD=8b13613b7518e1b293133d68622635ad\n",
            &secrets,
        );
        assert!(!out.text.contains("8b13613b7518e1b293133d68622635ad"));
        assert_eq!(out.text.matches("[redacted: postgres_password]").count(), 2);
        // Occurrences, not distinct values — see `Redaction::removed`.
        assert_eq!(out.removed, 2);
    }

    /// A short secret that is a prefix of a long one must not cut the long one in half
    /// and leave the rest of it readable.
    #[test]
    fn replaces_the_longest_first() {
        let secrets = vec![
            Secret { key: "short".into(), value: "abcd1234abcd".into() },
            Secret { key: "long".into(), value: "abcd1234abcdEFGH5678".into() },
        ];
        let out = redact("token=abcd1234abcdEFGH5678 end", &secrets);
        assert!(out.text.contains("[redacted: long]"));
        assert!(!out.text.contains("EFGH5678"));
    }

    #[test]
    fn scrubs_a_jwt_nothing_here_has_ever_seen() {
        let out = redact(
            "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.dBjftJeZ4CVP\n",
            &[],
        );
        assert!(!out.text.contains("eyJhbGciOiJIUzI1NiJ9"));
        assert!(out.text.contains("[redacted: token]"));
        // The line around it survives — a report has to stay readable.
        assert!(out.text.contains("Authorization: Bearer"));
    }

    #[test]
    fn scrubs_a_private_key_block() {
        let out = redact("key: -----BEGIN PRIVATE KEY-----MIIEv-----END PRIVATE KEY-----", &[]);
        assert!(out.text.contains("[redacted: private key]"));
        assert!(!out.text.contains("MIIEv"));
    }

    /// A weak password is the one most worth taking out, so length is not what decides
    /// it — but `secret_key: none` is a setting, and redacting the word `none` everywhere
    /// would protect nothing and cost the whole log.
    #[test]
    fn takes_short_passwords_but_not_placeholders() {
        let found = secrets_in(&doc("db:\n  password: omero\n  secret_key: none\n"));
        let values: Vec<&str> = found.iter().map(|s| s.value.as_str()).collect();
        assert_eq!(values, vec!["omero"]);
    }

    /// Kuvert's Fernet key is URL-safe base64 — `-`, `_` and `=` — so it does not look
    /// generated; it is caught by its name instead.
    #[test]
    fn takes_kuverts_fernet_key() {
        let key = "q2L-3n_vX0bYt8Qe7rJmW4sZk1uHc9pA6dFgTiNoV5E=";
        let found = secrets_in(&doc(&format!("kuvert:\n  fernet_key: {key}\n")));
        let values: Vec<&str> = found.iter().map(|s| s.value.as_str()).collect();
        assert_eq!(values, vec![key]);
    }

    /// The collateral a short password would otherwise cause: `admin` as a password must
    /// not turn every word that contains those letters into a marker.
    #[test]
    fn matches_a_short_secret_as_a_whole_word_only() {
        let secrets = vec![Secret { key: "password".into(), value: "admin".into() }];
        let out = redact("Unknown command: 'ensureadmin'\nlogin as admin failed\n", &secrets);
        assert!(out.text.contains("ensureadmin"));
        assert!(out.text.contains("login as [redacted: password] failed"));
    }

    /// The shape this actually takes in a log: compose prefixes every line, so the key
    /// as written in the config — one string with newlines in it — matches nothing, and
    /// only the body lines themselves can save it.
    #[test]
    fn scrubs_a_key_block_wrapped_across_prefixed_lines() {
        let log = "lok-1  | private_key: -----BEGIN PRIVATE KEY-----\n\
                   lok-1  | MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQCx7Kd9\n\
                   lok-1  | 8fH2mQpLZzV0rTyXbNc4WkEjHgFqAsDl3MnPoIuYtRxVbGh5JcKm2Nq7\n\
                   lok-1  | -----END PRIVATE KEY-----\n";
        let out = redact(log, &[]);
        assert!(!out.text.contains("MIIEvQIBADANBgkqhkiG9w0"));
        assert!(!out.text.contains("8fH2mQpLZzV0rTyXbNc4WkEj"));
        // The prefixes survive, so the block is still recognisable as what it was.
        assert!(out.text.contains("lok-1  | [redacted: token]"));
    }

    /// What must *not* be taken for key material: a digest and a container id are one
    /// case, they are long, and a maintainer reads them.
    #[test]
    fn leaves_digests_and_container_ids_alone() {
        let log = "rekuest-1 | image sha256:9f2ab3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f80\n\
                   rekuest-1 | container 3a4b5c6d7e8f90112233445566778899aabbccddeeff00112233445566778899\n";
        let out = redact(log, &[]);
        assert_eq!(out.text, log);
    }

    /// The grant, the reporter's state, the mesh key and the key files are all secrets a
    /// hub keeps outside its YAML — and every one of them has turned up in a log.
    #[test]
    fn collects_from_the_grant_the_reporter_the_mesh_key_and_the_key_files() {
        let dir = std::env::temp_dir().join(format!("konstruktor-redact-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("secrets")).unwrap();
        std::fs::write(
            dir.join("hub_credentials.json"),
            r#"{"version":1,"envelope":{"access_token":"at-7f3a9c","refresh_token":"rt-91ab44",
               "clients":{"mikro":{"client_secret":"cs-55e1d0"}},
               "mesh":{"ionscale_auth_key":"tskey-auth-kX1-grant"}}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("reporter.json"),
            r#"{"refresh_token":"rt-rotated-2"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("mesh.env"),
            "TS_AUTHKEY=tskey-auth-kQ2mZ7CNTRL-abcDEF123\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("secrets/kuvert.fernet"),
            "q2L-3n_vX0bYt8Qe7rJmW4sZk1uHc9pA6dFgTiNoV5E=\n",
        )
        .unwrap();

        let found = secrets_in_deployment(&dir);
        let values: Vec<&str> = found.iter().map(|s| s.value.as_str()).collect();
        for expected in [
            "at-7f3a9c",
            "rt-91ab44",
            "cs-55e1d0",
            "tskey-auth-kX1-grant",
            "rt-rotated-2",
            "tskey-auth-kQ2mZ7CNTRL-abcDEF123",
            "q2L-3n_vX0bYt8Qe7rJmW4sZk1uHc9pA6dFgTiNoV5E=",
        ] {
            assert!(values.contains(&expected), "{expected} not in {values:?}");
        }

        let out = redact(
            "tailscale-1 | TS_AUTHKEY=tskey-auth-kQ2mZ7CNTRL-abcDEF123 login\n",
            &found,
        );
        assert!(!out.text.contains("kQ2mZ7CNTRL"), "{}", out.text);
        assert!(out.text.contains("[redacted: TS_AUTHKEY]"), "{}", out.text);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn leaves_a_clean_log_untouched() {
        let text = "rekuest-1  | INFO Listening on 0.0.0.0:80\nrekuest-1  | GET /ht 200\n";
        let out = redact(text, &[]);
        assert_eq!(out.text, text);
        assert_eq!(out.removed, 0);
    }
}
