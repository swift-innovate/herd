use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Closed,
    Open,
    HalfOpen,
}

pub struct CircuitBreaker {
    failure_threshold: u32,
    timeout: Duration,
    recovery_time: Duration,
    state: Arc<RwLock<CircuitBreakerState>>,
}

struct CircuitBreakerState {
    state: State,
    failure_count: u32,
    last_failure: Option<Instant>,
    last_state_change: Instant,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, timeout: Duration, recovery_time: Duration) -> Self {
        Self {
            failure_threshold,
            timeout,
            recovery_time,
            state: Arc::new(RwLock::new(CircuitBreakerState {
                state: State::Closed,
                failure_count: 0,
                last_failure: None,
                last_state_change: Instant::now(),
            })),
        }
    }

    pub async fn is_closed(&self) -> bool {
        let state = self.state.read().await;
        match state.state {
            State::Closed => true,
            State::Open => {
                // Check if recovery time has passed
                if state.last_state_change.elapsed() > self.recovery_time {
                    drop(state);
                    self.transition_to_half_open().await;
                    true
                } else {
                    false
                }
            }
            State::HalfOpen => true,
        }
    }

    pub async fn record_success(&self) {
        let mut state = self.state.write().await;
        state.failure_count = 0;
        if state.state == State::HalfOpen {
            state.state = State::Closed;
            state.last_state_change = Instant::now();
            tracing::info!("Circuit breaker closed after successful request");
        }
    }

    pub async fn record_failure(&self) {
        let mut state = self.state.write().await;
        state.failure_count += 1;
        state.last_failure = Some(Instant::now());

        if state.failure_count >= self.failure_threshold {
            if state.state != State::Open {
                state.state = State::Open;
                state.last_state_change = Instant::now();
                tracing::warn!(
                    "Circuit breaker opened after {} failures",
                    state.failure_count
                );
            }
        }
    }

    async fn transition_to_half_open(&self) {
        let mut state = self.state.write().await;
        state.state = State::HalfOpen;
        state.last_state_change = Instant::now();
        tracing::info!("Circuit breaker in half-open state");
    }
}