use konstruktor_core::secrets::{
    build_ed25519_key_pair, generate_alpha_numeric_string, generate_django_secret_key,
};

/// The known-answer vector carried over verbatim from the TypeScript suite
/// (`src/deployment/__tests__/hub-config.test.ts`). It is the one fully deterministic
/// cross-language check in the whole port: the same seed has to produce the same PEM
/// bytes, or every hub Konstruktor writes signs provenance with a key its own services
/// would not recognise.
#[test]
fn the_ed25519_pair_matches_the_typescript_generator_byte_for_byte() {
    let pair = build_ed25519_key_pair(&[7u8; 32]);

    assert_eq!(pair.key_type, "Ed25519");
    assert_eq!(
        pair.private_key,
        "-----BEGIN PRIVATE KEY-----\n\
         MC4CAQAwBQYDK2VwBCIEIAcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcH\n\
         -----END PRIVATE KEY-----\n"
    );
    assert_eq!(
        pair.public_key,
        "-----BEGIN PUBLIC KEY-----\n\
         MCowBQYDK2VwAyEA6kpsY+KcUgq+9VB7Ey7F+ZVHdq6+vnuSQh7qaRRG0iw=\n\
         -----END PUBLIC KEY-----\n"
    );
}

/// `hub-config.test.ts` pins the shape of the generated secrets, not their values — any
/// uniform sampler over the same alphabet conforms. The alphabets themselves are the part
/// that must not drift.
#[test]
fn generated_secrets_have_the_shape_the_python_cli_produces() {
    let django = generate_django_secret_key();
    assert_eq!(django.chars().count(), 50);
    assert!(
        django
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "!@#$%^&*(-_=+)".contains(c)),
        "unexpected character in {django}"
    );

    let alnum = generate_alpha_numeric_string(40);
    assert_eq!(alnum.chars().count(), 40);
    assert!(alnum
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
}

/// Rejection sampling has a loop in it; a bad `limit` would either bias the output or
/// spin. Draw enough to be confident it terminates and stays in the alphabet.
#[test]
fn the_sampler_terminates_and_covers_its_alphabet() {
    let mut seen = std::collections::HashSet::new();
    for _ in 0..200 {
        for c in generate_django_secret_key().chars() {
            seen.insert(c);
        }
    }
    // 50 characters * 200 draws over a 50-symbol alphabet: every symbol should appear.
    assert_eq!(seen.len(), 50, "alphabet not fully covered: {seen:?}");
}
