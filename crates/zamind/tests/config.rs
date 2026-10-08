//! Config & network e2e (founder §37–39, ADR-0019): the layered config
//! model (ADR-0007) over the real protocol — config.get answers the
//! effective view with per-field provenance, config.set patches with
//! tri-state semantics (absent keeps, null clears to the global default,
//! a value sets the override) and refuses nonsense with the field named,
//! and network.status reports the desired port, the server.properties
//! authority, a live bind-test, and cross-server conflicts.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use serde_json::json;
use zamin_protocol::methods;

use common::{connect_daemon, make_server_root, register_server, scoped_dir, spawn_daemon};

#[tokio::test]
async fn config_get_defaults_and_provenance() {
    let dir = scoped_dir("config-get");
    let endpoint = common::endpoint_for(&dir);
    let data_dir = dir.join("data");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    let root = make_server_root("config-get-root");
    register_server(&mut client, "demo", &root).await;

    let got = client
        .request(methods::CONFIG_GET, json!({ "serverId": "demo" }))
        .await
        .expect("config.get");

    // Built-in defaults, nothing overridden: timeouts come from the
    // global file's built-in defaults, provenance answers global.
    assert_eq!(got["serverId"], "demo");
    assert_eq!(got["effective"]["stopTimeoutSecs"], 60);
    assert_eq!(got["effective"]["startupTimeoutSecs"], 120);
    assert_eq!(got["effective"]["backupKeep"], 10);
    assert!(got["effective"]["port"].is_null(), "no port configured yet");
    assert!(got["jar"].is_null(), "no jar override yet");
    assert_eq!(got["provenance"]["stopTimeoutSecs"], "global");
    assert_eq!(got["provenance"]["port"], "global");

    // Unknown server: typed, not a silent empty answer.
    let err = client
        .request(methods::CONFIG_GET, json!({ "serverId": "ghost" }))
        .await
        .unwrap_err();
    assert_eq!(err["code"], "SERVER_NOT_FOUND");
}

#[tokio::test]
async fn config_set_tri_state_patch_layering_and_clearing() {
    let dir = scoped_dir("config-set");
    let endpoint = common::endpoint_for(&dir);
    let data_dir = dir.join("data");

    // A global default the per-server file can override and clear. The
    // daemon reads the global file fresh on every config call, so writing
    // it before the daemon even starts is the deterministic setup.
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::write(
        data_dir.join("config.toml"),
        "schemaVersion = 1\n\n[defaults]\nport = 25570\n",
    )
    .unwrap();

    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    let root = make_server_root("config-set-root");
    register_server(&mut client, "demo", &root).await;

    // 1. A value sets the override; the answer is the fresh effective
    //    view (no second read needed) with custom provenance.
    let set = client
        .request(
            methods::CONFIG_SET,
            json!({
                "serverId": "demo",
                "settings": { "port": 25566, "minMemoryMb": 512, "maxMemoryMb": 2048 },
                "jar": "fabric/server.jar",
            }),
        )
        .await
        .expect("config.set value");
    assert_eq!(set["effective"]["port"], 25566);
    assert_eq!(set["effective"]["minMemoryMb"], 512);
    assert_eq!(set["effective"]["maxMemoryMb"], 2048);
    assert_eq!(set["jar"], "fabric/server.jar");
    assert_eq!(set["provenance"]["port"], "custom");
    assert_eq!(set["provenance"]["minMemoryMb"], "custom");

    // 2. Absent keeps: a patch that names only the stop timeout leaves
    //    the port override alone.
    let set = client
        .request(
            methods::CONFIG_SET,
            json!({ "serverId": "demo", "settings": { "stopTimeoutSecs": 45 } }),
        )
        .await
        .expect("config.set keep");
    assert_eq!(set["effective"]["port"], 25566, "kept");
    assert_eq!(set["effective"]["stopTimeoutSecs"], 45);
    assert_eq!(set["provenance"]["stopTimeoutSecs"], "custom");

    // 3. null clears: the port falls back to the global default (25570).
    let set = client
        .request(
            methods::CONFIG_SET,
            json!({ "serverId": "demo", "settings": { "port": null } }),
        )
        .await
        .expect("config.set clear");
    assert_eq!(set["effective"]["port"], 25570, "cleared to global");
    assert_eq!(set["provenance"]["port"], "global");

    // 4. The empty patch is a typed no-op refusal, not a silent success.
    let err = client
        .request(
            methods::CONFIG_SET,
            json!({ "serverId": "demo", "settings": {} }),
        )
        .await
        .unwrap_err();
    assert_eq!(err["code"], "PROTOCOL_INVALID_REQUEST");

    // 5. Nonsense is refused with the field named, file untouched.
    let err = client
        .request(
            methods::CONFIG_SET,
            json!({ "serverId": "demo", "settings": { "minMemoryMb": 4096, "maxMemoryMb": 2048 } }),
        )
        .await
        .unwrap_err();
    assert_eq!(err["code"], "CONFIG_INVALID");
    assert!(err["message"].as_str().unwrap().contains("minMemoryMb"));

    let err = client
        .request(
            methods::CONFIG_SET,
            json!({ "serverId": "demo", "settings": { "stopTimeoutSecs": 0 } }),
        )
        .await
        .unwrap_err();
    assert_eq!(err["code"], "CONFIG_INVALID");

    let err = client
        .request(
            methods::CONFIG_SET,
            json!({ "serverId": "demo", "jar": "../escape.jar" }),
        )
        .await
        .unwrap_err();
    assert_eq!(err["code"], "CONFIG_INVALID");

    // 6. The display name rides the same method; a blank name is refused.
    let set = client
        .request(
            methods::CONFIG_SET,
            json!({ "serverId": "demo", "displayName": "Box Demo" }),
        )
        .await
        .expect("config.set name");
    assert_eq!(set["displayName"], "Box Demo");

    let err = client
        .request(
            methods::CONFIG_SET,
            json!({ "serverId": "demo", "displayName": "   " }),
        )
        .await
        .unwrap_err();
    assert_eq!(err["code"], "CONFIG_INVALID");

    // The listing shows the rename (one registry, not two stores).
    let listed = client
        .request(
            methods::SERVER_LIST,
            json!({ "requestId": uuid::Uuid::now_v7().to_string() }),
        )
        .await
        .expect("server.list");
    assert_eq!(listed["servers"][0]["displayName"], "Box Demo");
}

#[tokio::test]
async fn network_status_probe_properties_and_conflicts() {
    let dir = scoped_dir("network-status");
    let endpoint = common::endpoint_for(&dir);
    let data_dir = dir.join("data");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    // Two servers, same desired port; each will report the other as a
    // conflict (§37's pre-start conflict check).
    let root_a = make_server_root("net-root-a");
    let root_b = make_server_root("net-root-b");
    register_server(&mut client, "alpha", &root_a).await;
    register_server(&mut client, "beta", &root_b).await;

    // server.properties carries the boot authority: beta names 25580
    // with a bind address; alpha has no properties file yet.
    std::fs::write(
        root_b.join("server.properties"),
        "motd=hello\nserver-port=25580\nserver-ip=127.0.0.1\n",
    )
    .unwrap();

    // Hold a port with a real listener so the bind-test has teeth.
    let listener = std::net::TcpListener::bind("0.0.0.0:25581").unwrap();

    // Both servers desire 25580 (set through config.set). 25580 itself
    // is not held, so the bind-test says available — but each conflict
    // list names the other (§37's pre-start conflict check).
    for server in ["alpha", "beta"] {
        client
            .request(
                methods::CONFIG_SET,
                json!({ "serverId": server, "settings": { "port": 25580 } }),
            )
            .await
            .expect("set desired port");
    }

    let status = client
        .request(methods::NETWORK_STATUS, json!({ "serverId": "alpha" }))
        .await
        .expect("network.status alpha");
    assert_eq!(status["desiredPort"], 25580);
    assert!(
        status["propertiesPort"].is_null(),
        "alpha has no server.properties yet"
    );
    assert_eq!(
        status["portAvailable"], true,
        "25580 is not bound by anyone"
    );
    let conflicts = status["conflicts"].as_array().expect("conflict list");
    assert_eq!(conflicts.len(), 1, "beta claims the same port");
    assert_eq!(conflicts[0], "beta");

    // beta: the properties authority is visible, bind address read —
    // never written by the panel (ADR-0007: server.properties is
    // Minecraft's file).
    let status = client
        .request(methods::NETWORK_STATUS, json!({ "serverId": "beta" }))
        .await
        .expect("network.status beta");
    assert_eq!(status["desiredPort"], 25580);
    assert_eq!(status["propertiesPort"], 25580);
    assert_eq!(status["bindAddress"], "127.0.0.1");
    assert_eq!(
        status["conflicts"].as_array().expect("conflict list").len(),
        1,
        "alpha claims the same port"
    );

    // alpha: point it at the held port; the bind-test must say in use.
    client
        .request(
            methods::CONFIG_SET,
            json!({ "serverId": "alpha", "settings": { "port": 25581 } }),
        )
        .await
        .expect("set held port");
    let status = client
        .request(methods::NETWORK_STATUS, json!({ "serverId": "alpha" }))
        .await
        .expect("network.status held");
    assert_eq!(status["portAvailable"], false, "25581 is really held");
    assert!(
        status["conflicts"].is_null(),
        "no other server desires 25581 (empty lists serialize absent)"
    );

    drop(listener);

    // A server with no port anywhere: no probe, honestly null.
    client
        .request(
            methods::CONFIG_SET,
            json!({ "serverId": "alpha", "settings": { "port": null } }),
        )
        .await
        .expect("clear alpha port");
    let status = client
        .request(methods::NETWORK_STATUS, json!({ "serverId": "alpha" }))
        .await
        .expect("network.status no port");
    assert!(status["desiredPort"].is_null());
    assert!(status["portAvailable"].is_null(), "nothing to probe");
}
