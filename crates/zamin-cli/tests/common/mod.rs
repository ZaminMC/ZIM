//! The CLI end-to-end harness: the real `zamin` binary driving the real
//! `zamind` binary over the real protocol. Shared by every integration
//! test in this crate; `spawn_with` lets a test hand the daemon extra
//! flags (e.g. `--modrinth-url` for the catalog tests).

#![allow(dead_code)]

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Workspace binaries share the target dir with this test binary (which
/// lives at target/<profile>/deps/). `CARGO_BIN_EXE_` only covers a
/// package's own bins, so walk the ancestors for the rest. Failing loudly
/// beats silently skipping — run `cargo test --workspace`.
pub fn workspace_bin(name: &str) -> PathBuf {
    let suffix = std::env::consts::EXE_SUFFIX;
    let mut dir = std::env::current_exe().expect("current exe");
    while let Some(parent) = dir.parent() {
        let candidate = parent.join(format!("{name}{suffix}"));
        if candidate.exists() {
            return candidate;
        }
        dir = parent.to_path_buf();
    }
    panic!("{name} not found next to the test binary; run `cargo test --workspace`");
}

/// Owns the daemon process. Tree kill on drop: the daemon deliberately
/// spawns servers that survive its own death (ADR-0001), so killing the
/// daemon alone leaks the fake server and its inherited stdout pipe.
pub struct TestDaemon {
    pub child: Child,
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = Command::new("taskkill")
                .args(["/PID", &self.child.id().to_string(), "/T", "/F"])
                .creation_flags(0x0800_0000)
                .status();
        }
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.wait();
    }
}

pub fn scoped_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zamin-cli-e2e-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

pub fn endpoint_arg(endpoint: &zamin_ipc::Endpoint) -> String {
    match endpoint {
        zamin_ipc::Endpoint::WindowsPipe(name) => name.clone(),
        zamin_ipc::Endpoint::UnixSocket(path) => path.to_string_lossy().into_owned(),
    }
}

/// Spawn the daemon in its own process group and prepare a fake-server
/// config so `zamin start` runs the real supervision path. Extra args go
/// to the daemon verbatim (catalog URLs, endpoint overrides, ...).
pub struct Harness {
    pub _daemon: TestDaemon,
    pub endpoint: String,
    pub root: PathBuf,
    pub zamin: PathBuf,
}

impl Harness {
    pub fn spawn(tag: &str) -> Harness {
        Harness::spawn_with(tag, &[])
    }

    pub fn spawn_with(tag: &str, daemon_args: &[String]) -> Harness {
        let data_dir = scoped_dir(&format!("{tag}-data"));
        let root = scoped_dir(&format!("{tag}-root"));
        std::fs::write(root.join("eula.txt"), "eula=true\n").expect("eula");
        std::fs::write(root.join("server.jar"), b"fake jar bytes").expect("jar");
        let config_dir = data_dir.join("servers").join("demo");
        std::fs::create_dir_all(&config_dir).expect("server runtime dir");
        let java_path = workspace_bin("fake-mc-server");
        std::fs::write(
            config_dir.join("config.toml"),
            format!(
                "[settings]\njavaPath = \"{}\"\n",
                java_path.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .expect("config");

        let endpoint = zamin_ipc::Endpoint::unique_for_test(tag);
        let endpoint = endpoint_arg(&endpoint);
        let child = {
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                let mut command = Command::new(workspace_bin("zamind"));
                command
                    .args([
                        "--data-dir",
                        data_dir.to_str().unwrap(),
                        "--endpoint",
                        &endpoint,
                    ])
                    .args(daemon_args)
                    .process_group(0);
                command.spawn().expect("zamind spawns")
            }
            #[cfg(windows)]
            {
                Command::new(workspace_bin("zamind"))
                    .args([
                        "--data-dir",
                        data_dir.to_str().unwrap(),
                        "--endpoint",
                        &endpoint,
                    ])
                    .args(daemon_args)
                    .spawn()
                    .expect("zamind spawns")
            }
        };
        Harness {
            _daemon: TestDaemon { child },
            endpoint,
            root,
            zamin: workspace_bin("zamin"),
        }
        .wait_ready()
    }

    /// Block until the daemon accepts a connection; the first CLI call
    /// must never race the bind.
    fn wait_ready(self) -> Harness {
        assert!(
            wait_until(Duration::from_secs(10), || {
                Command::new(&self.zamin)
                    .arg("--endpoint")
                    .arg(&self.endpoint)
                    .args(["--json", "daemon"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            }),
            "daemon never became ready"
        );
        self
    }

    /// Run `zamin` with the harness endpoint; captures stdout/stderr.
    pub fn zamin(&self, args: &[&str]) -> std::process::Output {
        Command::new(&self.zamin)
            .arg("--endpoint")
            .arg(&self.endpoint)
            .args(args)
            .output()
            .expect("zamin runs")
    }

    /// Run `zamin` with output discarded; asserts success.
    pub fn zamin_quiet(&self, args: &[&str]) {
        let status = Command::new(&self.zamin)
            .arg("--endpoint")
            .arg(&self.endpoint)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("zamin runs");
        assert!(status.success(), "zamin {args:?} failed");
    }

    pub fn zamin_json(&self, args: &[&str]) -> serde_json::Value {
        let mut all: Vec<&str> = vec!["--json"];
        all.extend_from_slice(args);
        let output = self.zamin(&all);
        assert!(
            output.status.success(),
            "zamin {args:?} failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        serde_json::from_slice(&output.stdout).expect("zamin prints JSON")
    }
}

pub fn wait_until(deadline: Duration, mut check: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + deadline;
    while Instant::now() < end {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    false
}

pub fn spawn_daemon(data_dir: &std::path::Path, endpoint: &zamin_ipc::Endpoint) -> TestDaemon {
    let endpoint = endpoint_arg(endpoint);
    let child = {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let mut command = Command::new(workspace_bin("zamind"));
            command
                .args([
                    "--data-dir",
                    data_dir.to_str().unwrap(),
                    "--endpoint",
                    &endpoint,
                ])
                .process_group(0);
            command.spawn().expect("zamind spawns")
        }
        #[cfg(windows)]
        {
            Command::new(workspace_bin("zamind"))
                .args([
                    "--data-dir",
                    data_dir.to_str().unwrap(),
                    "--endpoint",
                    &endpoint,
                ])
                .spawn()
                .expect("zamind spawns")
        }
    };
    TestDaemon { child }
}

pub async fn connect_with_retry(endpoint: zamin_ipc::Endpoint) -> zamin_cli::Client {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if Instant::now() > deadline {
            panic!("daemon never accepted a connection");
        }
        match zamin_cli::Client::connect(endpoint.clone()).await {
            Ok(client) => return client,
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}
