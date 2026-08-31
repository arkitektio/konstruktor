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
const SECRET_KEYS: [&str; 9] = [
    "password", "secret", "token", "auth_key", "access_key", "private", "credential",
    "passphrase", "salt",
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
/// Three sources, because the containers' environment comes from all three: the profile,
/// the generated service configs, and the compose file — which is hand-editable in this
/// app, and in the wild carries inline `POSTGRES_PASSWORD`s the profile has never seen.
/// Anything unreadable is skipped rather than failing the report: a folder missing its
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

    found.into_iter().collect()
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

    #[test]
    fn leaves_a_clean_log_untouched() {
        let text = "rekuest-1  | INFO Listening on 0.0.0.0:80\nrekuest-1  | GET /ht 200\n";
        let out = redact(text, &[]);
        assert_eq!(out.text, text);
        assert_eq!(out.removed, 0);
    }
}
