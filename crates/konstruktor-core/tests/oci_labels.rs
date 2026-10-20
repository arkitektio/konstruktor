//! What a service is, read from its image's label in the registry — without pulling it.
//!
//! A stand-in registry on this machine answers the way Docker Hub does: a `HEAD` that wants a
//! token first, an index with one manifest per platform, then the image's config. The label
//! the release build stamped there is the description an installer reads.

use std::path::PathBuf;

use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A digest of one repeated hex digit: each test serves its own, since what is read at a
/// digest is kept by it.
fn digest(digit: char) -> String {
    format!("sha256:{}", digit.to_string().repeat(64))
}

const AMD64: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const ARM64: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const CONFIG: &str = "sha256:4444444444444444444444444444444444444444444444444444444444444444";

/// Labels read from a registry are kept in the data directory: a scratch one here.
fn isolate_data_dir() -> PathBuf {
    use std::sync::OnceLock;
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("konstruktor-oci-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("a scratch data directory");
        std::env::set_var(konstruktor_core::registry::DATA_DIR_ENV, &root);
        root
    })
    .clone()
}

fn description() -> serde_json::Value {
    json!({
        "contract": 2,
        "name": "mikro",
        "identifier": "live.arkitekt.mikro",
        "summary": "Images and their metadata.",
        "serve": ["arkitekt-service", "serve"]
    })
}

/// A registry holding `repository:tag` as a two-platform index whose images carry `labels`.
/// `GET`s of manifests are expected `manifest_gets` times: a second read comes from the cache.
async fn registry(
    repository: &str,
    tag: &str,
    index: &str,
    labels: serde_json::Value,
    manifest_gets: u64,
) -> MockServer {
    let server = MockServer::start().await;
    let base = format!("/v2/{repository}");
    let challenge = format!(
        "Bearer realm=\"{}/token\",service=\"test\",scope=\"repository:{repository}:pull\"",
        server.uri()
    );

    Mock::given(method("GET"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"token": "anonymous"})))
        .mount(&server)
        .await;
    // Anonymous first, as an engine asks: the answer says where to get a token.
    Mock::given(method("HEAD"))
        .and(path(format!("{base}/manifests/{tag}")))
        .and(header("authorization", "Bearer anonymous"))
        .respond_with(ResponseTemplate::new(200).insert_header("docker-content-digest", index))
        .mount(&server)
        .await;
    Mock::given(method("HEAD"))
        .and(path(format!("{base}/manifests/{tag}")))
        .respond_with(
            ResponseTemplate::new(401).insert_header("www-authenticate", challenge.as_str()),
        )
        .mount(&server)
        .await;

    let (_, architecture) = host_architecture();
    let (mine, other) = if architecture == "arm64" {
        (ARM64, AMD64)
    } else {
        (AMD64, ARM64)
    };
    Mock::given(method("GET"))
        .and(path(format!("{base}/manifests/{index}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "manifests": [
                {"digest": other, "platform": {"os": "linux", "architecture": if architecture == "arm64" { "amd64" } else { "arm64" }}},
                {"digest": mine, "platform": {"os": "linux", "architecture": architecture}},
            ]
        })))
        .expect(manifest_gets)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{base}/manifests/{mine}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schemaVersion": 2,
            "config": {"digest": CONFIG, "mediaType": "application/vnd.oci.image.config.v1+json"},
        })))
        .expect(manifest_gets)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{base}/blobs/{CONFIG}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "architecture": architecture,
            "os": "linux",
            "config": {"Labels": labels},
        })))
        .mount(&server)
        .await;
    server
}

fn host_architecture() -> (&'static str, &'static str) {
    let architecture = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        _ => "amd64",
    };
    ("linux", architecture)
}

fn image(server: &MockServer, repository: &str, tag: &str) -> String {
    format!("{}/{repository}:{tag}", server.address())
}

#[tokio::test]
async fn a_service_is_described_by_its_label_without_being_pulled() {
    isolate_data_dir();
    let labels = json!({
        "live.arkitekt.service": description().to_string(),
        "org.opencontainers.image.version": "9.0.0",
    });
    let server = registry("arkitektio/mikro", "9", &digest('1'), labels, 1).await;
    let image = image(&server, "arkitektio/mikro", "9");

    let said = konstruktor_core::contract::describe(&image)
        .await
        .expect("the label describes the service");
    assert_eq!(said.name, "mikro");
    assert_eq!(said.identifier, "live.arkitekt.mikro");
    assert_eq!(said.serve, ["arkitekt-service", "serve"]);

    // Asked again, the tag is resolved with one HEAD and the rest comes from what was kept:
    // the manifests are fetched once (`expect(1)` above, checked when the server drops).
    let again = konstruktor_core::oci::remote_labels(&image)
        .await
        .expect("labels");
    assert_eq!(again["org.opencontainers.image.version"], "9.0.0");
}

#[tokio::test]
async fn an_image_without_the_label_has_none_to_read() {
    isolate_data_dir();
    let server = registry(
        "arkitektio/other",
        "1",
        &digest('5'),
        json!({"org.opencontainers.image.version": "1"}),
        1,
    )
    .await;
    let image = image(&server, "arkitektio/other", "1");

    assert_eq!(konstruktor_core::contract::labelled(&image).await, None);
    // Nor is it run to be asked: without the label, it is no service's.
    assert!(konstruktor_core::contract::describe(&image).await.is_none());
}

#[tokio::test]
async fn an_image_not_built_for_this_machine_says_so() {
    isolate_data_dir();
    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/v2/arkitektio/foreign/manifests/1"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("docker-content-digest", digest('6')),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/v2/arkitektio/foreign/manifests/{}",
            digest('6')
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "manifests": [{"digest": AMD64, "platform": {"os": "windows", "architecture": "amd64"}}]
        })))
        .mount(&server)
        .await;

    let error = konstruktor_core::oci::remote_labels(&image(&server, "arkitektio/foreign", "1"))
        .await
        .expect_err("nothing for linux here");
    assert!(error.contains("is not built for linux/"), "{error}");
}

/// Against Docker Hub itself: its token flow, its indexes, its blob redirects. Needs the
/// network, so it runs only when asked (`cargo test --test oci_labels -- --ignored`).
#[tokio::test]
#[ignore]
async fn docker_hub_answers_the_way_the_stand_in_does() {
    isolate_data_dir();
    let labels = konstruktor_core::oci::remote_labels("jhnnsrs/mikro:9")
        .await
        .expect("Docker Hub has mikro 9");
    assert!(
        labels.contains_key("org.opencontainers.image.version"),
        "{labels:?}"
    );
}
