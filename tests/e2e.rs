//! End-to-end tests — boot the real daemon, send messages, assert on state.
#![cfg(feature = "live-tests-llms")]

mod common;

#[path = "e2e/ark_nova.rs"]
mod ark_nova;
#[path = "e2e/cron_agent.rs"]
mod cron_agent;
#[path = "e2e/helpers.rs"]
mod helpers;
#[path = "e2e/scripting.rs"]
mod scripting;
