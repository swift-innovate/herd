//! Gateway `/status` probing.
//!
//! Parsing is a free function over a JSON string and the network call sits
//! behind the [`StatusProbe`] trait, so tests exercise the shape-handling with
//! canned bodies and never touch the network.

use crate::state::PollResult;
use std::time::Duration;

/// Probes a gateway for its health. Behind a trait so the event loop can be
/// driven by a fake in tests.
pub trait StatusProbe: Send {
    fn probe(&self, gateway: &str) -> PollResult;
}

/// Count healthy backends from a `/status` JSON body.
///
/// Prefers the additive scalar `healthy_backend_count` (added to the gateway for
/// exactly this use); falls back to the length of the `healthy_backends` array
/// for older gateways. A body that parses but has neither → `0` healthy.
pub fn parse_healthy_count(body: &str) -> usize {
    let json: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    if let Some(n) = json.get("healthy_backend_count").and_then(|v| v.as_u64()) {
        return n as usize;
    }
    json.get("healthy_backends")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0)
}

/// Real probe: blocking `GET {gateway}/status` with a short timeout. Any
/// transport error or non-success status is treated as `Unreachable`.
pub struct ReqwestProbe {
    client: reqwest::blocking::Client,
}

impl ReqwestProbe {
    pub fn new() -> Self {
        // A short timeout keeps the 5s poll cadence honest even when the gateway
        // is wedged; `build()` only fails on TLS backend init, so fall back to
        // the default client rather than propagating.
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap_or_default();
        Self { client }
    }
}

impl Default for ReqwestProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusProbe for ReqwestProbe {
    fn probe(&self, gateway: &str) -> PollResult {
        let url = format!("{}/status", gateway.trim_end_matches('/'));
        match self.client.get(&url).send() {
            Ok(resp) if resp.status().is_success() => match resp.text() {
                Ok(body) => PollResult::Up {
                    healthy: parse_healthy_count(&body),
                },
                Err(_) => PollResult::Unreachable,
            },
            _ => PollResult::Unreachable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_count_is_preferred() {
        let body = r#"{"healthy_backend_count": 4, "healthy_backends": []}"#;
        assert_eq!(parse_healthy_count(body), 4);
    }

    #[test]
    fn falls_back_to_array_length() {
        let body = r#"{"healthy_backends": [{"name":"a"},{"name":"b"}]}"#;
        assert_eq!(parse_healthy_count(body), 2);
    }

    #[test]
    fn zero_healthy_reads_as_zero() {
        assert_eq!(
            parse_healthy_count(r#"{"healthy_backend_count":0,"healthy_backends":[]}"#),
            0
        );
    }

    #[test]
    fn missing_fields_is_zero_not_panic() {
        assert_eq!(parse_healthy_count(r#"{"routing_strategy":"Scored"}"#), 0);
    }

    #[test]
    fn malformed_json_is_zero() {
        assert_eq!(parse_healthy_count("not json at all"), 0);
        assert_eq!(parse_healthy_count(""), 0);
    }

    /// A canned probe lets state-machine tests run without a gateway.
    struct FakeProbe(PollResult);
    impl StatusProbe for FakeProbe {
        fn probe(&self, _gateway: &str) -> PollResult {
            self.0
        }
    }

    #[test]
    fn trait_object_is_drivable_by_a_fake() {
        let probe: Box<dyn StatusProbe> = Box::new(FakeProbe(PollResult::Up { healthy: 2 }));
        assert_eq!(probe.probe("http://x"), PollResult::Up { healthy: 2 });
    }
}
