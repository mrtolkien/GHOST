use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
struct CircuitState {
    consecutive_failures: u32,
    open_until: Option<Instant>,
}

#[derive(Debug)]
pub struct CircuitBreaker {
    failure_threshold: u32,
    cooldown: Duration,
    states: Mutex<HashMap<String, CircuitState>>,
}

impl CircuitBreaker {
    #[must_use]
    pub fn new(failure_threshold: u32, cooldown: Duration) -> Self {
        Self {
            failure_threshold,
            cooldown,
            states: Mutex::new(HashMap::new()),
        }
    }

    #[tracing::instrument(skip_all, level = "debug", fields(model = %model))]
    pub fn check(&self, model: &str) -> Option<u64> {
        let now = Instant::now();
        let mut states = self.states.lock().expect("circuit breaker mutex poisoned");
        let state = states.get_mut(model)?;

        if let Some(open_until) = state.open_until {
            if open_until > now {
                return Some((open_until - now).as_secs().max(1));
            }

            state.open_until = None;
            state.consecutive_failures = 0;
            logfire::info!("provider circuit breaker closed", model = model.to_string());
        }

        None
    }

    #[tracing::instrument(skip_all, level = "debug", fields(model = %model))]
    pub fn record_success(&self, model: &str) {
        let mut states = self.states.lock().expect("circuit breaker mutex poisoned");
        if let Some(state) = states.get_mut(model)
            && (state.consecutive_failures > 0 || state.open_until.is_some())
        {
            state.consecutive_failures = 0;
            state.open_until = None;
            logfire::info!("provider failure streak reset", model = model.to_string());
        }
    }

    #[tracing::instrument(skip_all, level = "debug", fields(model = %model))]
    pub fn record_failure(&self, model: &str) {
        let now = Instant::now();
        let mut states = self.states.lock().expect("circuit breaker mutex poisoned");
        let state = states.entry(model.to_string()).or_insert(CircuitState {
            consecutive_failures: 0,
            open_until: None,
        });

        state.consecutive_failures += 1;
        if state.consecutive_failures >= self.failure_threshold {
            state.open_until = Some(now + self.cooldown);
            state.consecutive_failures = 0;
            logfire::warn!(
                "provider circuit breaker opened",
                model = model.to_string(),
                cooldown_secs = self.cooldown.as_secs()
            );
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(3, Duration::from_secs(60))
    }
}
