use serde::Deserialize;

/// Blocks EndTurn until TODO checklist is complete (opt-in).
///
/// When present, the agent cannot end its turn without a completed TODO
/// list. `no_todo` fires when no TODO exists at all, `incomplete` fires
/// when items remain unfinished. `{incomplete}` is interpolated with the
/// count.
#[derive(Debug, Clone, Deserialize)]
pub struct ProgressGateConfig {
    pub no_todo: String,
    pub incomplete: String,
}

/// Wall-clock timer nudge, fires once after `after_seconds` (opt-in).
///
/// `{minutes}` is interpolated into `message`.
#[derive(Debug, Clone, Deserialize)]
pub struct TemporalConfig {
    pub after_seconds: u64,
    pub message: String,
}

/// Tool-not-used-recently nudge, fires periodically (opt-in).
///
/// Checks the last `window` assistant turns for usage of `tool`. If
/// absent, injects `message`.
#[derive(Debug, Clone, Deserialize)]
pub struct RecencyConfig {
    pub tool: String,
    pub window: usize,
    pub message: String,
}

/// Context-size threshold nudge, fires once (opt-in).
///
/// When total conversation content exceeds `threshold_chars`, injects
/// `message`.
#[derive(Debug, Clone, Deserialize)]
pub struct ContextPressureConfig {
    pub threshold_chars: usize,
    pub message: String,
}
