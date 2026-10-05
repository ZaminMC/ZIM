//! State machine transition matrix — the tested truth of the lifecycle
//! (ADR-0005). Every accepted transition and the load-bearing rejections.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use zamin_protocol::server::CrashPhase;

use crate::supervisor::state::{InvalidTransition, ServerState as S, StateMachine};
use crate::supervisor::LifecycleCommand as Cmd;

fn apply_ok(state: S, command: Cmd) -> S {
    let mut machine = StateMachine::new(state);
    match machine.apply(command).expect("transition accepted") {
        crate::supervisor::Transition::To(next) => next,
        crate::supervisor::Transition::CrashReported(_) => S::Crashed,
    }
}

#[test]
fn start_paths_lead_to_starting() {
    for state in [S::NotRunning, S::Stopped, S::Crashed, S::FailedPreflight] {
        assert_eq!(apply_ok(state, Cmd::Start), S::Starting, "from {state:?}");
    }
}

#[test]
fn starting_resolves_to_running_or_crash() {
    assert_eq!(apply_ok(S::Starting, Cmd::Spawned), S::Starting);
    assert_eq!(apply_ok(S::Starting, Cmd::StartupValidated), S::Running);

    let mut machine = StateMachine::new(S::Starting);
    match machine
        .apply(Cmd::StartupFailed { exit_code: 1 })
        .expect("startup failure is a crash")
    {
        crate::supervisor::Transition::CrashReported(report) => {
            assert_eq!(report.phase, CrashPhase::Startup);
            assert_eq!(report.exit_code, Some(1));
        }
        _ => panic!("expected crash report"),
    }
}

#[test]
fn preflight_failure_is_not_a_crash() {
    assert_eq!(
        apply_ok(
            S::Starting,
            Cmd::PreflightFailed {
                error: "eula".into()
            }
        ),
        S::FailedPreflight
    );
}

#[test]
fn running_stops_or_crashes_runtime() {
    assert_eq!(apply_ok(S::Running, Cmd::Stop), S::Stopping);

    let mut machine = StateMachine::new(S::Running);
    match machine
        .apply(Cmd::Crashed { exit_code: 137 })
        .expect("runtime crash")
    {
        crate::supervisor::Transition::CrashReported(report) => {
            assert_eq!(report.phase, CrashPhase::Runtime);
            assert_eq!(report.exit_code, Some(137));
        }
        _ => panic!("expected crash report"),
    }
}

#[test]
fn stopping_completes_gracefully_or_crashes_shutdown() {
    assert_eq!(apply_ok(S::Stopping, Cmd::StoppedGracefully), S::Stopped);

    let mut machine = StateMachine::new(S::Stopping);
    match machine
        .apply(Cmd::Crashed { exit_code: 1 })
        .expect("shutdown crash")
    {
        crate::supervisor::Transition::CrashReported(report) => {
            assert_eq!(report.phase, CrashPhase::Shutdown);
        }
        _ => panic!("expected crash report"),
    }
}

#[test]
fn adoption_verifies_before_running() {
    assert_eq!(apply_ok(S::Adopting, Cmd::AdoptVerified), S::Running);
    assert_eq!(apply_ok(S::Adopting, Cmd::AdoptForeign), S::Unknown);
}

#[test]
fn reset_returns_terminal_states_to_not_running() {
    for state in [S::Stopped, S::Crashed, S::FailedPreflight, S::Unknown] {
        assert_eq!(apply_ok(state, Cmd::Reset), S::NotRunning, "from {state:?}");
    }
}

#[test]
fn dangerous_transitions_are_rejected() {
    let rejections: Vec<(S, Cmd)> = vec![
        // No double start.
        (S::Starting, Cmd::Start),
        (S::Running, Cmd::Start),
        (S::Starting, Cmd::Stop),
        (S::NotRunning, Cmd::Stop),
        // Nothing while stopping except completion.
        (S::Stopping, Cmd::Start),
        (S::Stopping, Cmd::Stop),
        (S::Stopping, Cmd::StartupValidated),
        // Adoption happens only from adopting.
        (S::Running, Cmd::AdoptVerified),
        (S::NotRunning, Cmd::AdoptVerified),
        // Terminal states do not accept lifecycle commands without reset.
        (S::Stopped, Cmd::Stop),
        (S::Crashed, Cmd::Stop),
        (S::FailedPreflight, Cmd::Stop),
        (S::Unknown, Cmd::Start),
        // Preflight failures only exist in starting.
        (
            S::Running,
            Cmd::PreflightFailed {
                error: String::new(),
            },
        ),
    ];
    for (state, command) in rejections {
        let mut machine = StateMachine::new(state);
        let err: InvalidTransition = machine.apply(command).expect_err("must reject");
        assert_eq!(err.state, state);
    }
}

#[test]
fn full_lifecycle_walk() {
    let mut machine = StateMachine::new(S::NotRunning);
    let walk: Vec<Cmd> = vec![
        Cmd::Start,
        Cmd::Spawned,
        Cmd::StartupValidated,
        Cmd::Stop,
        Cmd::StoppedGracefully,
        Cmd::Reset,
    ];
    let expected: Vec<S> = vec![
        S::Starting,
        S::Starting,
        S::Running,
        S::Stopping,
        S::Stopped,
        S::NotRunning,
    ];
    for (command, want) in walk.into_iter().zip(expected) {
        machine.apply(command).expect("walk is valid");
        assert_eq!(machine.state(), want);
    }
}
