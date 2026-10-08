//! The address resolver's registry half (P0 §13–§14, ADR-0027's layered
//! config over it): the fleet summary carries each server's configured
//! address — the layered port override else the boot authority in
//! `server.properties`, plus the bind address — because `0:25565` in the
//! address bar must reach the server that owns the port. A server id is
//! an identifier, never an address.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{
    connect_daemon, endpoint_for, make_server_root, register_server, scoped_dir, spawn_daemon,
};
use serde_json::json;
use std::path::Path;

fn write_properties(root: &Path, body: &str) {
    std::fs::write(root.join("server.properties"), body).unwrap();
}

async fn find_entry(client: &mut common::Client, server_id: &str) -> serde_json::Value {
    let list = client
        .request(zamin_protocol::methods::SERVER_LIST, json!({}))
        .await
        .unwrap();
    list["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["serverId"] == server_id)
        .cloned()
        .unwrap_or_else(|| panic!("missing {server_id} in {list:?}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn the_fleet_summary_carries_the_address_half() {
    let dir = scoped_dir("data-address");
    let endpoint = endpoint_for(&dir);
    let data_dir = dir.join("data");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    // Properties-bound: the boot authority is server.properties.
    let bound = make_server_root("root-address-bound");
    write_properties(
        &bound,
        "motd=bound\nserver-port=25565\nserver-ip=192.168.100.2\n",
    );
    register_server(&mut client, "bound", &bound).await;

    // Override-driven: the layered config names the port; the properties
    // file says nothing.
    let overridden = make_server_root("root-address-overridden");
    register_server(&mut client, "overridden", &overridden).await;
    common::write_server_config(&data_dir, "overridden", "port = 25577\n");

    let bound_entry = find_entry(&mut client, "bound").await;
    assert_eq!(bound_entry["port"], 25565);
    assert_eq!(bound_entry["bindAddress"], "192.168.100.2");

    let overridden_entry = find_entry(&mut client, "overridden").await;
    assert_eq!(overridden_entry["port"], 25577);
    // No server-ip in its properties: absent means all interfaces.
    assert!(overridden_entry["bindAddress"].is_null());
}
