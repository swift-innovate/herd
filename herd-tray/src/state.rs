//! Pure tray state machine — no I/O, no GUI, fully unit-tested.
//!
//! The tao/tray-icon glue in `main.rs` stays deliberately thin: every decision
//! about *which icon* and *which menu items* is made here, so the logic is
//! testable without a display server or a running gateway.

/// The tray icon tint. `Gray` is the pre-first-poll bootstrap state and is never
/// returned by [`next_state`] — it is the initial value the event loop holds
/// until the first poll resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconState {
    Gray,
    Green,
    Amber,
    Red,
}

/// Outcome of one `/status` poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollResult {
    /// Gateway did not answer (connection refused, timeout, non-2xx).
    Unreachable,
    /// Gateway answered; `healthy` = number of healthy backends.
    Up { healthy: usize },
}

/// How the tray relates to the gateway process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Gateway was already running at launch — the tray only observes it.
    Attach,
    /// The tray spawned `herd serve` and supervises that child.
    Supervise,
}

/// Which gateway-control menu item to surface (spec D8: only when supervising).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayControl {
    /// Attach mode — the tray never touches a gateway it did not start.
    None,
    /// Supervised child is down — offer "Start gateway".
    ShowStart,
    /// Supervised child is alive — offer "Stop gateway".
    ShowStop,
}

/// Resolve the icon tint from a poll result.
///
/// A dead supervised child takes precedence over the poll: if we launched the
/// gateway and it has exited, we are `Red` even if a stale in-flight poll still
/// reports `Up` (the poll thread and the supervisor observe the world at
/// slightly different instants).
pub fn next_state(poll: PollResult, supervising: bool, child_alive: bool) -> IconState {
    if supervising && !child_alive {
        return IconState::Red;
    }
    match poll {
        PollResult::Unreachable => IconState::Red,
        PollResult::Up { healthy } if healthy > 0 => IconState::Green,
        PollResult::Up { .. } => IconState::Amber,
    }
}

/// Derive the gateway-control menu item from the mode and child liveness.
pub fn gateway_control(mode: Mode, child_alive: bool) -> GatewayControl {
    match mode {
        Mode::Attach => GatewayControl::None,
        Mode::Supervise if child_alive => GatewayControl::ShowStop,
        Mode::Supervise => GatewayControl::ShowStart,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── next_state ────────────────────────────────────────────────────────

    #[test]
    fn up_with_healthy_backends_is_green() {
        assert_eq!(
            next_state(PollResult::Up { healthy: 3 }, false, true),
            IconState::Green
        );
    }

    #[test]
    fn up_with_zero_healthy_is_amber() {
        assert_eq!(
            next_state(PollResult::Up { healthy: 0 }, false, true),
            IconState::Amber
        );
    }

    #[test]
    fn unreachable_is_red() {
        assert_eq!(
            next_state(PollResult::Unreachable, false, true),
            IconState::Red
        );
    }

    #[test]
    fn dead_supervised_child_is_red_even_if_poll_stale_up() {
        // Race: poll thread still reports Up, but the child we launched has died.
        assert_eq!(
            next_state(PollResult::Up { healthy: 5 }, true, false),
            IconState::Red
        );
    }

    #[test]
    fn live_supervised_child_follows_poll() {
        assert_eq!(
            next_state(PollResult::Up { healthy: 1 }, true, true),
            IconState::Green
        );
        assert_eq!(
            next_state(PollResult::Up { healthy: 0 }, true, true),
            IconState::Amber
        );
        assert_eq!(
            next_state(PollResult::Unreachable, true, true),
            IconState::Red
        );
    }

    #[test]
    fn attach_mode_ignores_child_alive_flag() {
        // In attach mode child_alive is meaningless; the poll alone decides.
        assert_eq!(
            next_state(PollResult::Up { healthy: 2 }, false, false),
            IconState::Green
        );
    }

    // ── gateway_control ───────────────────────────────────────────────────

    #[test]
    fn attach_shows_no_gateway_control() {
        assert_eq!(gateway_control(Mode::Attach, true), GatewayControl::None);
        assert_eq!(gateway_control(Mode::Attach, false), GatewayControl::None);
    }

    #[test]
    fn supervise_alive_shows_stop() {
        assert_eq!(
            gateway_control(Mode::Supervise, true),
            GatewayControl::ShowStop
        );
    }

    #[test]
    fn supervise_dead_shows_start() {
        assert_eq!(
            gateway_control(Mode::Supervise, false),
            GatewayControl::ShowStart
        );
    }
}
