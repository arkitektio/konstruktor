use serde::{Deserialize, Serialize};

/// Mesh membership for a hub.
///
/// A hub only reachable at LAN addresses is only useful from that LAN. The mesh is the
/// way out: an ionscale tailnet — Tailscale's protocol against the coordination server's
/// own control plane — that every member of the organization is already on. The hub joins
/// through a Tailscale sidecar, and the gateway is published inside that container's
/// network namespace, so the hub answers on the tailnet under its own name.
///
/// The credential is a single-use pre-authorized key, minted as a side effect of the
/// authorization the wizard already performs (`request_auth_key` on the hub manifest).
///
/// Nothing here is written unless the mesh is switched on: a hub without it generates
/// byte for byte what it generated before the mesh existed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeshBlock {
    pub enabled: bool,
    /// The sidecar's service name in the compose file.
    pub host: String,
    pub image: String,
    /// The name the hub takes on the tailnet.
    pub hostname: String,
    /// Single-use pre-authorized key. A secret, and it lands in docker-compose.yaml.
    pub auth_key: String,
    /// The control server to log in to. `None` means Tailscale's own coordination
    /// service, which is what a hand-supplied `tskey-…` from a personal tailnet expects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coord_url: Option<String>,
    pub volume_name: String,
    /// The hub is reached *only* over the mesh: the gateway publishes no port on the
    /// host, and the manifest advertises the tailnet node and the in-network gateway,
    /// nothing on the LAN. Kept here so a re-authorization advertises the same thing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mesh_only: bool,
    /// The login the key was minted for (`sub-organization-hub`). The sidecar keeps its
    /// node state under it, so a hub accepted by another user, into another organization
    /// or as another hub starts from a fresh node instead of a revoked one. Absent on keys
    /// from before servers said, which keep the state directory they always had.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login: Option<String>,
}

impl MeshBlock {
    /// Where the sidecar keeps its node state: one directory per login.
    pub fn state_dir(&self) -> String {
        match &self.login {
            Some(login) => format!("{MESH_STATE_DIR}/{login}"),
            None => MESH_STATE_DIR.to_string(),
        }
    }

    /// What every sidecar is started with, a hub's or an engine's.
    ///
    /// The key and the control server, and nothing that advertises tags: the server tags
    /// a node by the key it was minted for, and an advertised tag would be accepted as
    /// given — putting the node into groups it was never meant to be in.
    pub fn sidecar_environment(&self) -> Vec<(&'static str, String)> {
        let mut environment = vec![
            ("TS_AUTHKEY", self.auth_key.clone()),
            ("TS_HOSTNAME", self.hostname.clone()),
            ("TS_STATE_DIR", self.state_dir()),
            // The kernel networking path; userspace mode would not carry the traffic of
            // the containers that share the sidecar's namespace.
            ("TS_USERSPACE", "false".to_string()),
        ];
        if let Some(coord) = &self.coord_url {
            environment.push(("TS_EXTRA_ARGS", format!("--login-server={coord}")));
        }
        environment
    }
}

/// Where the sidecar keeps its node identity, so a restart is not a new machine.
pub const MESH_STATE_DIR: &str = "/var/lib/tailscale";
/// Where the sidecar puts its LocalAPI socket, on a volume the health reporter mounts.
pub const MESH_SOCKET_DIR: &str = "/var/run/tailscale";
pub const MESH_SOCKET: &str = "/var/run/tailscale/tailscaled.sock";
pub const MESH_SOCKET_VOLUME: &str = "tailscale_socket";
pub const MESH_IMAGE: &str = "tailscale/tailscale:latest";

#[derive(Debug, Clone, Default)]
pub struct MeshOptions {
    pub hostname: String,
    pub auth_key: String,
    pub coord_url: Option<String>,
    /// See [`MeshBlock::login`].
    pub login: Option<String>,
}

pub fn build_mesh_block(options: &MeshOptions) -> MeshBlock {
    MeshBlock {
        enabled: true,
        host: "tailscale".to_string(),
        image: MESH_IMAGE.to_string(),
        hostname: options.hostname.trim().to_string(),
        auth_key: options.auth_key.trim().to_string(),
        coord_url: options
            .coord_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        volume_name: "tailscale_state".to_string(),
        mesh_only: false,
        login: options.login.clone(),
    }
}

/// A tailnet machine name. Tailscale lowercases and folds anything else into dashes when
/// it registers a node, so the name is normalised here rather than surprising the user
/// with a different one on the tailnet than the one they typed.
pub fn mesh_hostname(identifier: &str) -> String {
    // A *run* of disallowed characters folds to a single dash, matching the regex this
    // replaces (`[^a-z0-9-]+`). Dashes are allowed, so a name that already contains
    // "--" keeps it — collapsing those too would rename the node on the tailnet.
    let lowered = identifier.trim().to_lowercase();
    let mut folded = String::with_capacity(lowered.len());
    let mut in_run = false;
    for c in lowered.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' {
            folded.push(c);
            in_run = false;
        } else if !in_run {
            folded.push('-');
            in_run = true;
        }
    }

    folded.trim_matches('-').chars().take(63).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_an_identifier_into_a_tailnet_name() {
        assert_eq!(mesh_hostname("MyHub"), "myhub");
        assert_eq!(mesh_hostname("My Lab Hub"), "my-lab-hub");
        assert_eq!(mesh_hostname("  lab.hub_2  "), "lab-hub-2");
        assert_eq!(mesh_hostname("--edges--"), "edges");
        assert_eq!(mesh_hostname(""), "");
        // Dashes are allowed characters, so a run of them inside the name survives —
        // collapsing them would give the node a different name on the tailnet than the
        // one the user was shown.
        assert_eq!(mesh_hostname("a--b"), "a--b");
        // But a run of *disallowed* characters still folds to one.
        assert_eq!(mesh_hostname("a  !  b"), "a-b");
    }

    #[test]
    fn caps_the_name_at_a_dns_label() {
        assert_eq!(mesh_hostname(&"a".repeat(100)).len(), 63);
    }

    /// An empty control server means Tailscale's own, and must not be written as a key.
    #[test]
    fn an_empty_control_server_is_absent_not_blank() {
        let block = build_mesh_block(&MeshOptions {
            hostname: "hub".into(),
            auth_key: "tskey-x".into(),
            coord_url: Some("   ".into()),
            login: None,
        });
        assert_eq!(block.coord_url, None);
        let yaml = serde_norway::to_string(&block).unwrap();
        assert!(!yaml.contains("coord_url"), "{yaml}");
    }
}
