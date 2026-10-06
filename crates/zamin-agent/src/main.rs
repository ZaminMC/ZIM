//! zaminagent — the remote bridge (ADR-0011). Runs on the machine that
//! hosts the daemon: a TLS listener for remote protocol clients, each
//! authenticated by the token and relayed to the local per-user daemon.
//! The agent never spawns the daemon — on a headless host the daemon's
//! service unit owns its lifetime.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use zamin_agent::auth::{self, DEFAULT_TOKEN_FILE};
use zamin_agent::server::AgentServer;
use zamin_agent::tls;
use zamin_ipc::Endpoint;

const AGENT_NAME: &str = "zaminagent";
const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Loopback by default: exposing the agent to the network is an explicit
/// operator decision (`--listen 0.0.0.0:7443` behind a firewall it owns).
const DEFAULT_LISTEN: &str = "127.0.0.1:7443";

#[derive(Debug)]
struct AgentConfig {
    listen: SocketAddr,
    endpoint: Endpoint,
    /// Token file and TLS material live under the agent data directory:
    /// `<data>/agent/token`, `<data>/agent/tls/`.
    data_dir: PathBuf,
}

fn parse_args() -> AgentConfig {
    let mut config = AgentConfig {
        listen: DEFAULT_LISTEN
            .parse()
            .unwrap_or_else(|_| panic!("default listen address parses")),
        endpoint: Endpoint::default_endpoint(),
        data_dir: zamin_core::platform::paths::data_dir().join("agent"),
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--listen" => {
                let value = args
                    .next()
                    .unwrap_or_else(|| panic!("--listen needs a value"));
                config.listen = value
                    .parse()
                    .unwrap_or_else(|e| panic!("--listen {value}: {e}"));
            }
            "--endpoint" => {
                let value = args
                    .next()
                    .unwrap_or_else(|| panic!("--endpoint needs a value"));
                config.endpoint = Endpoint::from_daemon_arg(&value);
            }
            "--data-dir" => {
                let value = args
                    .next()
                    .unwrap_or_else(|| panic!("--data-dir needs a value"));
                config.data_dir = PathBuf::from(value);
            }
            "--version" => {
                println!("{AGENT_NAME} {AGENT_VERSION}");
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

async fn run(config: AgentConfig) {
    // Process boundaries: an agent that cannot establish its credential or
    // its TLS identity has nothing to fall back to.
    #[allow(clippy::expect_used)]
    let token = auth::load_or_generate_token(&config.data_dir.join(DEFAULT_TOKEN_FILE))
        .expect("agent token is loadable or generatable");
    #[allow(clippy::expect_used)]
    let material = tls::TlsMaterial::load_or_generate(&config.data_dir.join("tls"))
        .expect("agent tls material is loadable or generatable");
    #[allow(clippy::expect_used)]
    let tls_config = Arc::new(
        material
            .server_config()
            .expect("agent tls server config builds"),
    );
    let fingerprint = material.fingerprint_hex();
    let fingerprint_path = config.data_dir.join("tls").join(tls::CERT_FILE);
    let token_path = config.data_dir.join(DEFAULT_TOKEN_FILE);

    let endpoint = config.endpoint.clone();
    let endpoint_display = format!("{endpoint:?}");
    #[allow(clippy::expect_used)]
    let server = AgentServer::bind(config.listen, tls_config, endpoint, token)
        .await
        .expect("agent listener binds");

    println!("{AGENT_NAME} {AGENT_VERSION}");
    println!("listening on {}", server.local_addr());
    println!("daemon endpoint {endpoint_display}");
    println!(
        "certificate fingerprint (pin this on remote clients):\n  {}",
        tls::fingerprint_display(&fingerprint)
    );
    println!("certificate file {fingerprint_path:?}");
    println!("token file {token_path:?}");

    if let Err(e) = server.run().await {
        tracing::error!("listener failed: {e}");
        std::process::exit(1);
    }
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
