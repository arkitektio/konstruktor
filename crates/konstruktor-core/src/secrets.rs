use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use ed25519_dalek::SigningKey;
use rand::RngCore;
use serde::{Deserialize, Serialize};

/// Port of `arkitekt_next/server/config/utils.py`.
///
/// Konstruktor authors the deployment profile itself, so every secret the Python CLI used
/// to generate has to be generated here — with the same alphabets and lengths, so a config
/// written by Konstruktor is indistinguishable from one written by `init`.
///
/// Randomness comes from the OS CSPRNG.
fn random_bytes(length: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; length];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes
}

/// `secrets.choice` over an alphabet. Rejection sampling keeps the distribution flat: a
/// plain modulo would bias the first `256 % alphabet.len()` characters.
fn choose(alphabet: &[u8], length: usize) -> String {
    let limit = (256 / alphabet.len()) * alphabet.len();
    let mut out = String::with_capacity(length);
    while out.len() < length {
        for byte in random_bytes(length) {
            if (byte as usize) >= limit {
                continue;
            }
            out.push(alphabet[byte as usize % alphabet.len()] as char);
            if out.len() == length {
                break;
            }
        }
    }
    out
}

const DJANGO_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*(-_=+)";
const ALPHA_NUMERIC: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// A 50-character Django `SECRET_KEY`.
pub fn generate_django_secret_key() -> String {
    choose(DJANGO_ALPHABET, 50)
}

pub fn generate_alpha_numeric_string(length: usize) -> String {
    choose(ALPHA_NUMERIC, length)
}

// The word lists are copied verbatim from `config/utils.py`; names generated here end up
// in docker network names and MinIO root users, so they must stay in the same shape.
pub const ADJECTIVES: [&str; 66] = [
    "ancient",
    "autumn",
    "billowing",
    "bold",
    "brave",
    "bright",
    "calm",
    "clever",
    "cool",
    "crimson",
    "curly",
    "damp",
    "dawn",
    "delicate",
    "divine",
    "dry",
    "empty",
    "falling",
    "floral",
    "fragrant",
    "frosty",
    "gentle",
    "green",
    "happy",
    "hidden",
    "holy",
    "icy",
    "jolly",
    "late",
    "lingering",
    "little",
    "lively",
    "long",
    "lucky",
    "misty",
    "morning",
    "muddy",
    "nameless",
    "noisy",
    "old",
    "patient",
    "polished",
    "proud",
    "purple",
    "quiet",
    "rapid",
    "restless",
    "rough",
    "shiny",
    "shy",
    "silent",
    "small",
    "snowy",
    "solitary",
    "sparkling",
    "spring",
    "still",
    "summer",
    "twilight",
    "wandering",
    "weathered",
    "wild",
    "winter",
    "wispy",
    "withered",
    "young",
];

pub const NOUNS: [&str; 67] = [
    "badger",
    "bird",
    "breeze",
    "brook",
    "bush",
    "butterfly",
    "cherry",
    "cloud",
    "darkness",
    "dawn",
    "dew",
    "dream",
    "dust",
    "feather",
    "field",
    "fire",
    "firefly",
    "flower",
    "fog",
    "forest",
    "frog",
    "frost",
    "glade",
    "glitter",
    "grass",
    "haze",
    "hill",
    "lake",
    "leaf",
    "lion",
    "log",
    "meadow",
    "moon",
    "morning",
    "mountain",
    "night",
    "otter",
    "owl",
    "paper",
    "pine",
    "pond",
    "rain",
    "resonance",
    "river",
    "sea",
    "shadow",
    "shape",
    "silence",
    "sky",
    "smoke",
    "snow",
    "snowflake",
    "sound",
    "star",
    "sun",
    "sunset",
    "surf",
    "thunder",
    "tree",
    "violet",
    "voice",
    "water",
    "waterfall",
    "wave",
    "wildflower",
    "wind",
    "wood",
];

/// `adjective-noun`, as `generate_name` upstream.
pub fn generate_name() -> String {
    format!("{}-{}", pick(&ADJECTIVES), pick(&NOUNS))
}

/// One item, chosen without modulo bias — the same rejection sampling `choose` uses.
fn pick(items: &[&str]) -> String {
    let limit = (256 / items.len()) * items.len();
    loop {
        for byte in random_bytes(8) {
            if (byte as usize) < limit {
                return items[byte as usize % items.len()].to_string();
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyPair {
    pub key_type: String,
    pub public_key: String,
    pub private_key: String,
}

/// PEM: base64 wrapped at 64 columns, with a trailing newline after the footer.
fn pem(label: &str, der: &[u8]) -> String {
    let body = BASE64.encode(der);
    let mut out = format!("-----BEGIN {label}-----\n");
    // Chunk rather than insert-every-64th: a body that is an exact multiple of 64
    // characters would otherwise end up with a blank line before the footer.
    for chunk in body.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(chunk).expect("base64 is ASCII"));
        out.push('\n');
    }
    out.push_str(&format!("-----END {label}-----\n"));
    out
}

/// Ed25519 keys have a fixed-length DER encoding, so the ASN.1 wrapper is a constant
/// prefix rather than something worth running an encoder for.
///
/// This is deliberately *not* `ed25519_dalek::to_pkcs8_pem`. That emits PKCS#8 **v2**,
/// which sets `version = 1` and attaches the public key in an `[1]` context tag; Python's
/// `cryptography` — and therefore every key the Arkitekt CLI ever wrote — emits **v1**,
/// private key only. The two differ from the fourth byte onward, so a v2 key would be a
/// silent incompatibility with the provenance the rest of the platform verifies.
const PKCS8_V1_PREFIX: [u8; 16] = [
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];
const SPKI_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

fn wrap(prefix: &[u8], tail: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + tail.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(tail);
    out
}

/// The Ed25519 pair Rekuest signs provenance attestations with — a PKCS#8 v1 private key
/// and a SubjectPublicKeyInfo public key, both PEM.
pub fn build_ed25519_key_pair(seed: &[u8; 32]) -> KeyPair {
    let signing = SigningKey::from_bytes(seed);

    KeyPair {
        key_type: "Ed25519".to_string(),
        private_key: pem("PRIVATE KEY", &wrap(&PKCS8_V1_PREFIX, seed)),
        public_key: pem(
            "PUBLIC KEY",
            &wrap(&SPKI_PREFIX, signing.verifying_key().as_bytes()),
        ),
    }
}

/// A fresh pair, for a hub that is being created rather than reproduced in a test.
pub fn generate_ed25519_key_pair() -> KeyPair {
    let mut seed = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut seed);
    build_ed25519_key_pair(&seed)
}
