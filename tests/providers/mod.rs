#[path = "../common.rs"]
pub mod common;

mod registry;

#[cfg(feature = "live-tests")]
mod cache_live;
#[cfg(feature = "live-tests")]
mod kimi_code_live;
#[cfg(feature = "live-tests")]
mod openai_oauth_live;
#[cfg(feature = "live-tests")]
mod openrouter_live;
#[cfg(feature = "live-tests")]
mod tool_use_live;
