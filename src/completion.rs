use tokio::sync::mpsc;

/// Events emitted when background tasks complete.
#[derive(Debug, Clone)]
pub enum CompletionEvent {
    ShellCommand { session_id: String, command: String },
}

pub type CompletionSender = mpsc::UnboundedSender<CompletionEvent>;
pub type CompletionReceiver = mpsc::UnboundedReceiver<CompletionEvent>;

pub fn channel() -> (CompletionSender, CompletionReceiver) {
    mpsc::unbounded_channel()
}
