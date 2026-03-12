use tokio::sync::oneshot;

/// Style hint for confirmation options. Interfaces choose rendering.
#[derive(Debug, Clone)]
pub enum OptionStyle {
    Primary,
    Secondary,
    Danger,
}

/// A single option in a confirmation dialogue.
#[derive(Debug, Clone)]
pub struct ConfirmationOption {
    pub id: String,
    pub label: String,
    pub style: OptionStyle,
}

/// A confirmation dialogue that a tool can send to ask the OPERATOR
/// for approval before proceeding. Interface-agnostic — each interface
/// decides how to render and collect the response.
#[derive(Debug, Clone)]
pub struct Confirmation {
    pub prompt: String,
    pub context: Option<String>,
    pub options: Vec<ConfirmationOption>,
}

/// A confirmation request sent by a tool. Contains the dialogue to show
/// and a oneshot sender to return the OPERATOR's chosen option ID.
pub struct ConfirmationRequest {
    pub confirmation: Confirmation,
    pub response_tx: oneshot::Sender<String>,
    /// Which interface channel originated the request (e.g. Discord channel ID).
    pub channel_id: Option<String>,
}

/// Sender half — stored in `ToolContext` so tools can send confirmation requests.
pub type ConfirmationSender = tokio::sync::mpsc::UnboundedSender<ConfirmationRequest>;
/// Receiver half — held by the interface (Discord, CLI) to render and respond.
pub type ConfirmationReceiver = tokio::sync::mpsc::UnboundedReceiver<ConfirmationRequest>;

pub fn channel() -> (ConfirmationSender, ConfirmationReceiver) {
    tokio::sync::mpsc::unbounded_channel()
}
