//! A real Lok, driven as the person who would otherwise sit in front of it: logged in, and
//! accepting the hubs and apps that ask to be accepted. Shared by the end-to-end tests that
//! connect something to a coordination server.
#![allow(dead_code)]

use konstruktor_core::create::CreateEvent;
use serde_json::{json, Value};
use tokio::sync::mpsc;

pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("a free port")
}

/// An address of this machine that its containers reach it at, too.
pub fn host_address() -> String {
    if let Ok(address) = std::env::var("KONSTRUKTOR_E2E_HOST_ADDRESS") {
        return address;
    }
    let output = konstruktor_core::docker::command()
        .args([
            "network",
            "inspect",
            "bridge",
            "--format",
            "{{(index .IPAM.Config 0).Gateway}}",
        ])
        .output()
        .expect("docker runs");
    let address = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(
        !address.is_empty(),
        "docker's bridge has no gateway address"
    );
    address
}

/// Lok's management API, as the person who would otherwise click "accept": a session of
/// the seeded user, its CSRF token sent back the way the SPA sends it.
pub struct Lok {
    base: String,
    client: reqwest::Client,
    cookies: Vec<(String, String)>,
}

impl Lok {
    pub async fn login(base: &str, user: &str, password: &str) -> Self {
        let client = reqwest::Client::builder()
            // The session cookie comes with the redirect, which is not to be followed.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("a client");
        let mut lok = Self {
            base: base.trim_end_matches('/').to_string(),
            client,
            cookies: Vec::new(),
        };
        let login = format!("{}/lok/accounts/login/", lok.base);
        let page = lok.client.get(&login).send().await.expect("the login page");
        lok.keep(&page);
        let form = [
            ("csrfmiddlewaretoken", lok.cookie("csrftoken")),
            ("login", user.to_string()),
            ("password", password.to_string()),
        ];
        let answered = lok
            .client
            .post(&login)
            .header("Cookie", lok.cookie_header())
            .header("Referer", &login)
            .form(&form)
            .send()
            .await
            .expect("the login answers");
        lok.keep(&answered);
        assert!(
            !lok.cookie("sessionid").is_empty(),
            "{user} could not log in to {base} ({})",
            answered.status()
        );
        lok
    }

    fn keep(&mut self, response: &reqwest::Response) {
        for header in response.headers().get_all("set-cookie") {
            let Some((name, value)) = header
                .to_str()
                .ok()
                .and_then(|v| v.split(';').next())
                .and_then(|pair| pair.split_once('='))
            else {
                continue;
            };
            self.cookies.retain(|(kept, _)| kept != name);
            self.cookies.push((name.to_string(), value.to_string()));
        }
    }

    fn cookie(&self, name: &str) -> String {
        self.cookies
            .iter()
            .find(|(kept, _)| kept == name)
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    }

    fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub async fn graphql(&self, query: &str, variables: Value) -> Value {
        let answer: Value = self
            .client
            .post(format!("{}/lok/managementgraphql/", self.base))
            .header("Cookie", self.cookie_header())
            .header("X-CSRFToken", self.cookie("csrftoken"))
            .header("Referer", format!("{}/", self.base))
            .json(&json!({"query": query, "variables": variables}))
            .send()
            .await
            .expect("lok answers")
            .json()
            .await
            .expect("lok answers JSON");
        assert!(answer["errors"].is_null(), "{query}: {answer}");
        answer["data"].clone()
    }

    /// Accepts the hub that displayed `code` into the organization `slug`.
    pub async fn accept_hub(&self, code: &str, slug: &str) {
        let pending = self
            .graphql(
                "query($c: String!) { hubDeviceCodeByCode(code: $c) { id } }",
                json!({"c": code}),
            )
            .await;
        let organizations = self
            .graphql("{ organizations { id slug } }", json!({}))
            .await;
        let organization = organizations["organizations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["slug"] == slug)
            .unwrap_or_else(|| panic!("no organization {slug}: {organizations}"))["id"]
            .clone();
        self.graphql(
            "mutation($i: AcceptHubDeviceCodeInput!) { acceptHubDeviceCode(input: $i) { id } }",
            json!({"i": {
                "deviceCode": pending["hubDeviceCodeByCode"]["id"],
                "code": code,
                "organization": organization,
                "allowIonscale": false,
            }}),
        )
        .await;
    }

    /// Accepts the app that displayed `code` into the hub named `hub`.
    pub async fn accept_app(&self, code: &str, hub: &str) {
        let pending = self
            .graphql(
                "query($c: String!) { deviceCodeByCode(deviceCode: $c) { id } }",
                json!({"c": code}),
            )
            .await;
        let hubs = self.graphql("{ hubs { id name } }", json!({})).await;
        let hub_id = hubs["hubs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["name"] == hub)
            .unwrap_or_else(|| panic!("no hub {hub}: {hubs}"))["id"]
            .clone();
        self.graphql(
            "mutation($i: AcceptDeviceCodeInput!) { acceptDeviceCode(input: $i) { id } }",
            json!({"i": {
                "deviceCode": pending["deviceCodeByCode"]["id"],
                "code": code,
                "hub": hub_id,
                "allowIonscale": false,
            }}),
        )
        .await;
    }
}

/// The device code an acceptance displays, as a channel the approver waits on.
pub fn codes() -> (impl Fn(CreateEvent) + Sync, mpsc::UnboundedReceiver<String>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let on = move |event: CreateEvent| {
        if let CreateEvent::Staged { user_code, .. } = &event {
            let _ = sender.send(user_code.clone());
        }
        eprintln!("{event:?}");
    };
    (on, receiver)
}
