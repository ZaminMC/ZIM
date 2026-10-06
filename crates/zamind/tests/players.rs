//! `players.list` end-to-end: the Server List Ping result plus the log
//! roster. The fake-mc-server simulates joins and leaves over stdin
//! (`join <name>` / `leave <name>` emit the vanilla log lines), so the
//! full path is real: server stdout → log pumps → hub roster → protocol
//! reply → (in the panel) the Players view.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use serde_json::{json, Value};
use zamin_protocol::methods;
use zamin_protocol::server::ServerState;

mod common;

use common::{
    connect_daemon, make_server_root, register_server, scoped_dir, spawn_daemon, start_server,
    subscribe_events, wait_for_state, write_server_config, Client,
};

/// Roster from a players.list reply, as sorted names.
fn roster_of(reply: &Value) -> Vec<String> {
    reply["roster"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

async fn stdin(client: &mut Client, line: &str) {
    client
        .request(
            methods::SERVER_STDIN,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "test",
                "line": line,
            }),
        )
        .await
        .expect("stdin accepted");
}

/// The roster travels through the log pump's 50 ms batcher, so assertions
/// poll for the expected shape instead of sleeping.
async fn roster_reaches(client: &mut Client, expected: &[&str]) -> Vec<String> {
    let expected: Vec<String> = expected.iter().map(|s| (*s).to_owned()).collect();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let reply: Value = client
            .request(methods::PLAYERS_LIST, json!({ "serverId": "test" }))
            .await
            .expect("players.list answers");
        let last = roster_of(&reply);
        if last == expected {
            return last;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "roster never reached {expected:?}; last shape {last:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn log_roster_follows_joins_and_leaves_and_dies_with_the_process() {
    let data_dir = scoped_dir("players-roster");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("players-roster");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    subscribe_events(&mut client, Some("test")).await;

    // port = 0: nothing listens, so the ping half reports the honest
    // "unreachable" shape while the roster half still works.
    let root = make_server_root("players-roster");
    register_server(&mut client, "test", &root).await;
    write_server_config(&data_dir, "test", "port = 0\n");

    start_server(&mut client, "test")
        .await
        .expect("start accepted");
    wait_for_state(&mut client, ServerState::Running, Duration::from_secs(15))
        .await
        .expect("server reaches running");

    // Empty room before anyone joins.
    roster_reaches(&mut client, &[]).await;

    // Two joins land on the roster, sorted for stable display.
    stdin(&mut client, "join Steve").await;
    stdin(&mut client, "join Alex").await;
    assert_eq!(
        roster_reaches(&mut client, &["Alex", "Steve"]).await,
        vec!["Alex".to_owned(), "Steve".to_owned()]
    );

    // A leave removes exactly that player.
    stdin(&mut client, "leave Steve").await;
    assert_eq!(
        roster_reaches(&mut client, &["Alex"]).await,
        vec!["Alex".to_owned()]
    );

    // A chat line that happens to contain the phrase never joins: the
    // output prefix breaks the username charset.
    stdin(&mut client, "say Notch joined the game").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        roster_reaches(&mut client, &["Alex"]).await,
        vec!["Alex".to_owned()]
    );

    // The roster dies with the process: a stopped server is an empty room.
    client
        .request(
            methods::SERVER_STOP,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "test",
            }),
        )
        .await
        .expect("stop accepted");
    wait_for_state(&mut client, ServerState::Stopped, Duration::from_secs(15))
        .await
        .expect("server stops");
    let reply: Value = client
        .request(methods::PLAYERS_LIST, json!({ "serverId": "test" }))
        .await
        .expect("players.list answers");
    assert_eq!(
        roster_of(&reply),
        Vec::<String>::new(),
        "the roster must not outlive the server process"
    );
}
