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
        listen: DEFAULT_LISTEN.parse().unwrap_or_else(|_| {
            // A constant that fails to parse is a build error, not a usage
            // error — but exiting with a MESSAGE beats a raw panic either
            // way.
            eprintln!("internal error: the default listen address does not parse");
            std::process::exit(2);
        }),
        endpoint: Endpoint::default_endpoint(),
        data_dir: zamin_core::platform::paths::data_dir().join("agent"),
    };
    // The OS's own bytes: `env::args()` PANICS on a non-UTF-8 argument
    // before the usage message can exist. Flags are matched lossily (a
    // garbage argv entry reaches the "unknown argument" exit, not a
    // panic); --data-dir keeps its OsString — a path is not text, and a
    // lossy decode would invent a directory that does not exist.
    let mut args = std::env::args_os().skip(1);
    // One error path for every malformed invocation: a message and exit
    // code 2, never a panic (exit 101 + backtrace was the old answer to
    // a missing --listen value).
    let usage = |message: String| -> ! {
        eprintln!("{message}");
        eprintln!(
            "usage: zaminagent [--listen <addr>] [--endpoint <endpoint>] [--data-dir <dir>] [--version]"
        );
        std::process::exit(2);
    };
    while let Some(arg) = args.next() {
        match arg.to_string_lossy().as_ref() {
            "--listen" => {
                let Some(value) = args.next() else {
                    usage("--listen needs a value (e.g. 127.0.0.1:7443)".to_owned());
                };
                let Some(text) = value.to_str() else {
                    usage("--listen value is not valid Unicode".to_owned());
                };
                config.listen = text.parse().unwrap_or_else(|e| {
                    usage(format!("--listen {text} is not a valid address: {e}"))
                });
            }
            "--endpoint" => {
                let Some(value) = args.next() else {
                    usage("--endpoint needs a value".to_owned());
                };
                let Some(text) = value.to_str() else {
                    usage("--endpoint value is not valid Unicode".to_owned());
                };
                config.endpoint = Endpoint::from_daemon_arg(text);
            }
            "--data-dir" => {
                let Some(value) = args.next() else {
                    usage("--data-dir needs a value".to_owned());
                };
                config.data_dir = PathBuf::from(value);
            }
            "--version" => {
                println!("{AGENT_NAME} {AGENT_VERSION}");
                std::process::exit(0);
            }
            other => {
                usage(format!("unknown argument {other}"));
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
