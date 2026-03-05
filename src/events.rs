use tokio::sync::mpsc;

use crate::chat::RunMetadata;

/// A request to deliver a system message to a session and trigger a
/// continuation chat turn.
#[derive(Debug, Clone)]
pub struct SessionEvent {
    /// Target session ID
    pub session_id: String,
    /// System message to inject before triggering continuation
    pub system_message: String,
    /// Optional metadata for Discord presentation
    pub discord: Option<DiscordPayload>,
}

#[derive(Debug, Clone)]
pub struct DiscordPayload {
    /// Agent name for summary embed
    pub agent_name: Option<String>,
    /// Agent run metadata (tool counts, etc.)
    pub agent_metadata: Option<RunMetadata>,
    /// Agent findings snippet
    pub agent_findings: Option<String>,
}

pub type SessionEventSender = mpsc::UnboundedSender<SessionEvent>;
pub type SessionEventReceiver = mpsc::UnboundedReceiver<SessionEvent>;

pub fn channel() -> (SessionEventSender, SessionEventReceiver) {
    mpsc::unbounded_channel()
}
