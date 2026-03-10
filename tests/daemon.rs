//! Daemon-level e2e tests — boot the real daemon, send messages, assert on state.
#![cfg(feature = "live-tests")]

mod common;

#[path = "daemon/helpers.rs"]
mod helpers;
#[path = "daemon/ark_nova.rs"]
mod ark_nova;
#[path = "daemon/scripting.rs"]
mod scripting;
