pub mod backup;
pub mod catalog;
pub mod compose;
pub mod compose_file;
pub mod config;
pub mod connect;
pub mod contract;
pub mod coord;
pub mod create;
pub mod credentials;
pub mod defaults;
pub mod deregister;
pub mod destroy;
pub mod docker;
pub mod engine;
pub mod engine_probe;
pub mod freeze;
pub mod gateway_check;
pub mod generate;
pub mod generations;
pub mod git;
pub mod health;
pub mod hosts;
pub mod hubhealth;
pub mod lock;
pub mod migrate;
pub mod overrides;
pub mod owned;
pub mod paths;
pub mod pins;
pub mod process;
pub mod profile;
pub mod ready;
pub mod reclaim;
pub mod redact;
pub mod registry;
pub mod remedy;
pub mod report;
pub mod restore;
pub mod rollback;
pub mod secrets;
pub mod services;
pub mod shutdown;
pub mod start;
pub mod status;
pub mod templates;
pub mod updates;

// The tests' own knowledge of the services (`tests/support`), for the unit tests here as
// well: nothing in the library says what a service needs, so a test that wants a hub with
// its buckets and keys says it from there. The alias lets that one file name the crate
// the way an integration test does.
#[cfg(test)]
extern crate self as konstruktor_core;
#[cfg(test)]
#[path = "../tests/support/mod.rs"]
pub(crate) mod support;
