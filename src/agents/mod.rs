pub mod definition;
pub mod error;
pub mod nudges;
pub mod runner;
pub mod watcher;

pub use definition::{ProgressRule, TaskDefinition, TaskInfo, discover_tasks, load_task};
pub use error::TaskError;
pub use nudges::{ContextPressureConfig, ProgressGateConfig, RecencyConfig, TemporalConfig};
pub use runner::TaskRunner;
