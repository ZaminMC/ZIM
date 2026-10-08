//! Extensions e2e (founder §56/§57, ADR-0031): the daemon's inventory
//! answers for every folder under its extensions dir — valid manifests
//! with their declared permissions, broken ones as named problems, an
//! empty dir as an honest empty list, and `contributionsActive` false
//! until the execution model lands.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{connect_daemon, endpoint_for, scoped_dir, spawn_daemon};
use serde_json::json;
use std::path::Path;

fn write_extension(data: &Path, name: &str, manifest: Option<&str>) {
    let dir = data.join("extensions").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    if let Some(body) = manifest {
        std::fs::write(dir.join("zamin-extension.toml"), body).unwrap();
    }
}

const GOOD: &str = r#"
id = "export-config"
name = "Export configuration"
version = "0.3.0"
description = "Adds an export action to the server context menu."
permissions = ["contribution:context-menu", "data:files.read"]
"#;

#[tokio::test(flavor = "multi_thread")]
async fn the_inventory_answers_for_every_folder_it_saw() {
    let data_dir = scoped_dir("data-extensions");
    let data = data_dir.join("data");
    let endpoint = endpoint_for(&data_dir);
    let _daemon = spawn_daemon(&data, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    write_extension(&data, "export-config", Some(GOOD));
    write_extension(&data, "broken", Some("id = 7\n"));
    write_extension(&data, "nomanifest", None);

    let result = client
        .request(methods::EXTENSIONS_LIST, json!({}))
        .await
        .expect("extensions.list");

    // The answer says where it looked — the daemon's own extensions dir.
    assert!(result["directory"]
        .as_str()
        .map(|d| d.ends_with("extensions"))
        .unwrap_or(false));

    let extensions = result["extensions"].as_array().unwrap();
    assert_eq!(
        extensions.len(),
        1,
        "only the valid manifest: {extensions:?}"
    );
    let ext = &extensions[0];
    assert_eq!(ext["id"], "export-config");
    assert_eq!(ext["name"], "Export configuration");
    assert_eq!(ext["version"], "0.3.0");
    assert_eq!(
        ext["permissions"].as_array().unwrap(),
        &[json!("contribution:context-menu"), json!("data:files.read")]
    );
    assert_eq!(ext["directory"], "export-config");

    let problems = result["problems"].as_array().unwrap();
    assert_eq!(problems.len(), 2, "problems: {problems:?}");
    assert!(problems
        .iter()
        .any(|p| p["directory"] == "broken" && p["reason"].as_str().unwrap().contains("parse")));
    assert!(problems
        .iter()
        .any(|p| p["directory"] == "nomanifest"
            && p["reason"].as_str().unwrap().contains("unreadable")));

    // The honest room marker: nothing contributes yet.
    assert_eq!(result["contributionsActive"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn no_extensions_dir_is_an_honest_empty_list() {
    let data_dir = scoped_dir("data-extensions-empty");
    let endpoint = endpoint_for(&data_dir);
    let _daemon = spawn_daemon(&data_dir.join("data"), &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    let result = client
        .request(methods::EXTENSIONS_LIST, json!({}))
        .await
        .expect("extensions.list on an empty dir");
    assert_eq!(result["extensions"].as_array().unwrap().len(), 0);
    assert_eq!(result["problems"].as_array().unwrap().len(), 0);
    assert_eq!(result["contributionsActive"], false);
}

use zamin_protocol::methods;
