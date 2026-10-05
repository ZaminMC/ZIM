//! Protocol conformance fixtures (protocol spec §10): recorded exchanges
//! asserted against the typed model. Additive changes must keep these
//! passing; breaking changes must update them intentionally.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};
use zamin_protocol::envelope::{IncomingMessage, RequestId};
use zamin_protocol::error::ErrorCode;
use zamin_protocol::handshake::HelloResult;
use zamin_protocol::jobs::JobOutcome;
use zamin_protocol::methods;
use zamin_protocol::server::{LifecycleResult, ServerState};
use zamin_protocol::streams::{
    CoreEvent, StreamCursor, StreamKind, StreamNotification, StreamPayload,
};
use zamin_protocol::ProtocolError;

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.json"));
    serde_json::from_str(&fs::read_to_string(path).expect("fixture readable"))
        .expect("fixture json")
}

fn parse(name: &str) -> IncomingMessage {
    IncomingMessage::parse(&fixture(name)).expect("fixture classifies as an incoming message")
}

/// Typed round trip: serialize the parsed message and compare semantic JSON
/// equality (field order and skipped empty fields must not lose data that
/// the fixture carried).
fn round_trip(msg: &IncomingMessage, original: &Value) {
    let re = match msg {
        IncomingMessage::Request(r) => serde_json::to_value(r).unwrap(),
        IncomingMessage::Notification(n) => serde_json::to_value(n).unwrap(),
        IncomingMessage::Response(r) => serde_json::to_value(r).unwrap(),
    };
    assert_eq!(re, *original, "round trip must preserve the fixture");
}

#[test]
fn hello_request() {
    let original = fixture("hello-request");
    let msg = parse("hello-request");
    round_trip(&msg, &original);

    let IncomingMessage::Request(req) = msg else {
        panic!("expected request");
    };
    assert_eq!(req.method, methods::DAEMON_HELLO);
    let params = req
        .parse_params::<zamin_protocol::handshake::HelloParams>()
        .unwrap();
    assert_eq!(params.protocol, zamin_protocol::PROTOCOL_VERSION);
    assert_eq!(params.client.name, "zamin-cli");
    assert!(params.auth.is_none());
}

#[test]
fn hello_response() {
    let msg = parse("hello-response");
    let IncomingMessage::Response(resp) = msg else {
        panic!("expected response");
    };
    let result: HelloResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    assert_eq!(result.protocol, 1);
    assert_eq!(result.protocol_min, 1);
    assert_eq!(result.protocol_max, 1);
    assert_eq!(result.daemon.name, "zamind");
    assert!(result.capabilities.contains(&"streams".to_owned()));
}

#[test]
fn version_mismatch_error() {
    let msg = parse("version-mismatch-error");
    let IncomingMessage::Response(resp) = msg else {
        panic!("expected response");
    };
    let err = resp.error.expect("expected error object");
    assert_eq!(err.code, ErrorCode::ProtocolVersionUnsupported);
}

#[test]
fn start_request_and_lifecycle_response() {
    let original = fixture("start-request");
    let msg = parse("start-request");
    round_trip(&msg, &original);

    let IncomingMessage::Request(req) = msg else {
        panic!("expected request");
    };
    assert_eq!(req.method, methods::SERVER_START);
    let params = req
        .parse_params::<zamin_protocol::server::ServerIdParams>()
        .unwrap();
    assert_eq!(params.server_id, "production");

    let lifecycle: LifecycleResult =
        serde_json::from_value(fixture("lifecycle-response")["result"].clone()).unwrap();
    assert_eq!(lifecycle.state, ServerState::Starting);
}

#[test]
fn port_in_use_error_carries_context_and_remediation() {
    let msg = parse("port-in-use-error");
    let IncomingMessage::Response(resp) = msg else {
        panic!("expected response");
    };
    let err: ProtocolError = resp.error.expect("expected error object");
    assert_eq!(err.code, ErrorCode::PortInUse);
    assert_eq!(err.context["port"], json!(25565));
    assert_eq!(err.context["heldByServerId"], json!("production"));
    assert_eq!(
        err.remediation,
        ["choose_another_port", "stop_managed_server"]
    );
}

#[test]
fn state_changed_notification() {
    let msg = parse("state-changed-notification");
    let IncomingMessage::Notification(note) = msg else {
        panic!("expected notification");
    };
    assert_eq!(note.method, methods::STREAMS_NOTIFICATION);
    let n: StreamNotification = note.parse_params().unwrap();
    assert_eq!(n.stream, StreamKind::Events);
    assert_eq!(n.server_id.as_deref(), Some("production"));
    let StreamPayload::Event { event } = n.payload else {
        panic!("expected event payload");
    };
    let CoreEvent::ServerStateChanged {
        server_id,
        from,
        to,
        reason,
        ..
    } = event
    else {
        panic!("expected state changed event");
    };
    assert_eq!(server_id, "production");
    assert_eq!(from, ServerState::Starting);
    assert_eq!(to, ServerState::Running);
    assert_eq!(reason.as_deref(), Some("startup-validated"));
}

#[test]
fn logs_notification_with_batch() {
    let msg = parse("logs-notification");
    let IncomingMessage::Notification(note) = msg else {
        panic!("expected notification");
    };
    let n: StreamNotification = note.parse_params().unwrap();
    let StreamPayload::Logs { batch } = n.payload else {
        panic!("expected log payload");
    };
    assert_eq!(batch.len(), 2);
    assert_eq!(batch[0].level, zamin_protocol::streams::LogLevel::Info);
    assert_eq!(batch[0].thread.as_deref(), Some("Server thread"));
    assert_eq!(batch[1].level, zamin_protocol::streams::LogLevel::Warn);
    assert!(batch[1].thread.is_none());
    assert!(batch[0].line.starts_with("Done ("));
}

#[test]
fn missed_marker_notification() {
    let msg = parse("missed-notification");
    let IncomingMessage::Notification(note) = msg else {
        panic!("expected notification");
    };
    let n: StreamNotification = note.parse_params().unwrap();
    assert!(matches!(n.payload, StreamPayload::Missed { missed: 128 }));
}

#[test]
fn subscribe_response_with_snapshot_and_cursor() {
    let msg = parse("subscribe-response");
    let IncomingMessage::Response(resp) = msg else {
        panic!("expected response");
    };
    let result: zamin_protocol::streams::SubscribeResult =
        serde_json::from_value(resp.result.unwrap()).unwrap();
    assert_eq!(result.subscription_id, "s-1");
    assert!(!result.cursor_invalid);
    assert_eq!(result.cursor, Some(StreamCursor::Events { seq: 48210 }));
    let snapshot = result
        .snapshot
        .expect("events subscription carries a snapshot");
    assert_eq!(snapshot.servers.len(), 1);
    assert_eq!(snapshot.servers[0].state, ServerState::Running);
}

#[test]
fn job_completed_notification() {
    let msg = parse("job-completed-notification");
    let IncomingMessage::Notification(note) = msg else {
        panic!("expected notification");
    };
    let n: StreamNotification = note.parse_params().unwrap();
    let StreamPayload::Event { event } = n.payload else {
        panic!("expected event payload");
    };
    let CoreEvent::JobCompleted { outcome, .. } = event else {
        panic!("expected job completed event");
    };
    assert_eq!(outcome, JobOutcome::Succeeded);
}

#[test]
fn unknown_fields_are_ignored() {
    // Forward compatibility rule (protocol spec §1): unknown fields, at any
    // depth, must not break parsing.
    let msg = parse("unknown-field-tolerance");
    let IncomingMessage::Request(req) = msg else {
        panic!("expected request");
    };
    assert_eq!(req.id, RequestId::Number(42));
    assert_eq!(req.method, methods::SERVER_GET);
    let params = req
        .parse_params::<zamin_protocol::server::GetServerParams>()
        .unwrap();
    assert_eq!(params.server_id, "production");
}
