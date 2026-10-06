//! zamind — the resident daemon (ADR-0001). One per user per machine; owns
//! every server process; serves the Zamin Protocol over the local
//! transport. Clients spawn it transparently; it never stops servers as a
//! side effect of exiting.

mod actor;
mod engine;
mod files;
mod hub;
mod jobs;
mod session;

use std::path::PathBuf;

use zamin_ipc::IpcServer;

const DAEMON_NAME: &str = "zamind";
const DAEMON_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
struct DaemonConfig {
    endpoint: zamin_ipc::Endpoint,
    data_dir: PathBuf,
}

fn parse_args() -> DaemonConfig {
    let mut config = DaemonConfig {
        endpoint: zamin_ipc::Endpoint::default_endpoint(),
        data_dir: zamin_core::platform::paths::data_dir(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--endpoint" => {
                let value = args
                    .next()
                    .unwrap_or_else(|| panic!("--endpoint needs a value"));
                config.endpoint = zamin_ipc::Endpoint::from_daemon_arg(&value);
            }
            "--data-dir" => {
                let value = args
                    .next()
                    .unwrap_or_else(|| panic!("--data-dir needs a value"));
                config.data_dir = PathBuf::from(value);
            }
            "--version" => {
                println!("{DAEMON_NAME} {DAEMON_VERSION}");
                std::process::exit(0);
            }
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    config
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();
}

fn main() {
    init_tracing();
    let config = parse_args();

    // Process boundary: a daemon that cannot build its runtime has nothing
    // to fall back to. expect-with-context is the documented exception.
    #[allow(clippy::expect_used)]
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime builds");
    runtime.block_on(run(config));
}

async fn run(config: DaemonConfig) {
    let engine = engine::Engine::new(config.data_dir.clone()).await;

    let mut server = match IpcServer::bind(config.endpoint.clone()).await {
        Ok(server) => server,
        Err(zamin_ipc::IpcError::AlreadyRunning) => {
            tracing::error!("another daemon instance already owns this endpoint; exiting");
            std::process::exit(1);
        }
        Err(e) => {
            tracing::error!("cannot bind endpoint {:?}: {e}", config.endpoint);
            std::process::exit(1);
        }
    };

    // Adoption pass, AFTER the single-instance bind: a second daemon must
    // exit without doing adoption work against a hub it is about to throw
    // away. Servers the previous daemon left running come back as
    // `adopting` and are verified against their recorded process identity
    // (ADR-0005). No process is ever killed here.
    engine.adopt_existing_servers().await;

    tracing::info!(
        "daemon {:?} listening (protocol {}, data dir {:?})",
        config.endpoint,
        zamin_protocol::PROTOCOL_VERSION,
        config.data_dir
    );

    loop {
        match server.accept().await {
            Ok(connection) => {
                let engine = engine.clone();
                tokio::spawn(async move {
                    if let Err(e) = session::serve(connection, engine).await {
                        tracing::warn!("session ended: {e}");
                    }
                });
            }
            Err(e) => {
                tracing::error!("accept failed: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}
