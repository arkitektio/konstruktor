//! What a new hub is, when nobody says otherwise — in one place, so the wizard and the CLI
//! cannot drift apart on it.
//!
//! The CLI reads these directly; the desktop app asks for them through the `defaults`
//! command, and fills the wizard's first state from the answer.

use serde::Serialize;

use crate::catalog::ServiceId;
use crate::config::hub::StorageMode;
use crate::create::MeshMode;
use crate::hosts::ReachPresetId;

/// The coordination server a hub authorizes against unless told otherwise.
pub const COORDINATION_SERVER: &str = "go.arkitekt.live";
/// The ports a hub takes on this machine, in one run starting at 2754 — A-R-K-I on a
/// phone keypad. Away from the 3000s, 5000s, 7000s and 8000s that development servers
/// crowd into, below every system's ephemeral range, and the default of no common open
/// source project (their IANA entries are old proprietary registrations).
pub const HTTP_PORT: u16 = 2754;
pub const HTTPS_PORT: u16 = 2755;
/// LiveKit's signalling, as the gateway serves it. Not LiveKit's own 7880: a second
/// LiveKit on the machine — any other stack that brings one — would already hold it.
pub const LIVEKIT_SIGNAL_PORT: u16 = 2756;
/// LiveKit's media, over TCP and over UDP. Published as they are; see
/// [`crate::config::hub::LivekitBlock`].
pub const LIVEKIT_RTC_TCP_PORT: u16 = 2757;
pub const LIVEKIT_RTC_UDP_PORT: u16 = 2758;
/// How far a hub is advertised: the addresses others on this network can use.
pub const REACH: ReachPresetId = ReachPresetId::ThisNetwork;
/// Join the organization's mesh, keeping the LAN addresses too.
pub const MESH_MODE: MeshMode = MeshMode::Coordination;
pub const MESH_ONLY: bool = false;
pub const STORAGE: StorageMode = StorageMode::DockerVolumes;
/// Start the stack once it is written. The mesh key a hub is granted expires fifteen
/// minutes after it was issued, so a hub that waits to be started may never join.
pub const START: bool = true;

pub fn services() -> Vec<ServiceId> {
    crate::catalog::default_services()
}

/// Everything above, for a front end that cannot read Rust constants.
#[derive(Debug, Clone, Serialize)]
pub struct Defaults {
    pub coordination_server: &'static str,
    pub services: Vec<ServiceId>,
    pub http_port: u16,
    pub https_port: u16,
    pub reach: ReachPresetId,
    pub mesh_mode: MeshMode,
    pub mesh_only: bool,
    pub storage: StorageMode,
    pub start: bool,
}

pub fn all() -> Defaults {
    Defaults {
        coordination_server: COORDINATION_SERVER,
        services: services(),
        http_port: HTTP_PORT,
        https_port: HTTPS_PORT,
        reach: REACH,
        mesh_mode: MESH_MODE,
        mesh_only: MESH_ONLY,
        storage: STORAGE,
        start: START,
    }
}
