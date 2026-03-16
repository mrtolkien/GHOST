pub mod accessibility;
pub mod cdp;
pub mod connection;
pub mod discovery;
pub mod error;
pub mod manager;
pub mod tab;
pub mod url_check;

pub use self::cdp::ScrollDirection;
pub use self::error::BrowserError;
pub use self::manager::BrowserManager;

pub(crate) const MAX_SNAPSHOT_NODES: usize = 500;
pub(crate) const MAX_SNAPSHOT_DEPTH: usize = 15;
