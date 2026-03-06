use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;

/// Message sent into a running tool loop to steer or stop it.
#[derive(Debug)]
pub enum Interrupt {
    /// Inject a user message between tool calls.
    Steer { message: String },
    /// Stop the tool loop gracefully after the current tool finishes.
    Stop,
}

pub type InterruptSender = mpsc::UnboundedSender<Interrupt>;
pub type InterruptReceiver = mpsc::UnboundedReceiver<Interrupt>;

pub fn channel() -> (InterruptSender, InterruptReceiver) {
    mpsc::unbounded_channel()
}

/// Tracks which sessions have a running tool loop.
/// Key: session_id, Value: sender to interrupt that loop.
pub type ActiveSessions = Arc<DashMap<String, InterruptSender>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steer_and_stop_send_correctly() {
        let (tx, mut rx) = channel();
        tx.send(Interrupt::Steer {
            message: "change direction".into(),
        })
        .unwrap();
        tx.send(Interrupt::Stop).unwrap();

        match rx.try_recv().unwrap() {
            Interrupt::Steer { message } => assert_eq!(message, "change direction"),
            Interrupt::Stop => panic!("expected Steer"),
        }
        match rx.try_recv().unwrap() {
            Interrupt::Stop => {}
            Interrupt::Steer { .. } => panic!("expected Stop"),
        }
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn sender_error_when_receiver_dropped() {
        let (tx, rx) = channel();
        drop(rx);
        assert!(tx.send(Interrupt::Stop).is_err());
    }
}
