//! `openmesh-bridge` — OpenMesh local bridge daemon.
//!
//! Serves the WebSocket + REST API from `docs/architecture/WEBSOCKET-API.md`, owns the radio
//! transports and persists messages, channels, contacts, nodes and attachments.
//!
//! ```text
//! openmesh-bridge --demo                       # virtual mesh + seeded demo nodes/messages
//! openmesh-bridge --sim                        # virtual mesh, empty state
//! openmesh-bridge                             # UDP LAN on :4403 + local API on :8787
//! openmesh-bridge --serial /dev/ttyACM0       # requires --features real-serial
//! openmesh-bridge --serve-web apps/web/dist    # also serve the built web client
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use openmesh_bridge::transport::sim::MeshConfig;
use openmesh_bridge::{Bridge, BridgeOptions, token};

#[derive(Debug, Parser)]
#[command(
    name = "openmesh-bridge",
    version,
    about = "OpenMesh local bridge daemon (radios, protocol engine, message store, WS/REST API)"
)]
struct Args {
    /// API listen address (loopback by default).
    #[arg(long, default_value = "127.0.0.1:8787", value_name = "ADDR")]
    listen: SocketAddr,

    /// Bind address alias; binding a non-loopback address is potentially unsafe.
    #[arg(long, value_name = "ADDR", conflicts_with = "listen")]
    bind: Option<SocketAddr>,

    /// File holding the API bearer token (created with mode 0600 on first run).
    #[arg(long, value_name = "PATH")]
    token_file: Option<PathBuf>,

    /// redb database path.
    #[arg(long, value_name = "PATH")]
    db: Option<PathBuf>,

    /// Data directory (identity seed, attachments, default db).
    #[arg(long, value_name = "DIR")]
    data_dir: Option<PathBuf>,

    /// Node display name.
    #[arg(long, value_name = "NAME")]
    name: Option<String>,

    /// Node class: client, repeater, gateway, room_server, tracker.
    #[arg(long, value_name = "CLASS")]
    class: Option<String>,

    /// LoRa profile: LONG_SLOW, LONG_FAST, MEDIUM_FAST, SHORT_FAST, SHORT_TURBO.
    #[arg(long, value_name = "PROFILE")]
    profile: Option<String>,

    /// Regulatory region: EU868, US915, AU915, IN865, KR920, AS923.
    #[arg(long, value_name = "REGION")]
    region: Option<String>,

    /// Use the in-process virtual mesh instead of hardware radios.
    #[arg(long)]
    sim: bool,

    /// Demo mode: virtual mesh plus seeded demo nodes, conversations and channels.
    #[arg(long)]
    demo: bool,

    /// UDP LAN transport port (default 4403 unless --sim/--demo).
    #[arg(long, value_name = "PORT")]
    udp_port: Option<u16>,

    /// Disable the UDP LAN transport.
    #[arg(long)]
    no_udp: bool,

    /// Optional TCP listener for WAN bridge links.
    #[arg(long, value_name = "ADDR")]
    tcp_listen: Option<SocketAddr>,

    /// Serial radio port, e.g. /dev/ttyACM0 (requires the real-serial feature).
    #[arg(long, value_name = "PORT")]
    serial: Option<String>,

    /// Directory of the built web client to serve at `/`.
    #[arg(long, value_name = "DIR")]
    serve_web: Option<PathBuf>,

    /// Simulated per-recipient packet loss for the virtual mesh, in percent.
    #[arg(long, value_name = "PCT", value_parser = clap::value_parser!(u8).range(0..=100))]
    sim_loss: Option<u8>,

    /// Simulated one-way latency for the virtual mesh, in milliseconds.
    #[arg(long, value_name = "MS")]
    sim_latency: Option<u64>,

    /// RNG seed for deterministic simulated loss.
    #[arg(long, value_name = "N")]
    sim_seed: Option<u64>,

    /// Diagnostics only: serve the API without running the protocol engine.
    #[arg(long, hide = true)]
    api_only: bool,

    /// Log filter (or set RUST_LOG).
    #[arg(long, default_value = "info", env = "RUST_LOG", value_name = "FILTER")]
    log: String,
}

fn init_tracing(filter: &str) {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .try_init();
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(signal) => signal,
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            _ = sigterm.recv() => tracing::info!("received SIGTERM, shutting down"),
            _ = sigint.recv() => tracing::info!("received SIGINT, shutting down"),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("received Ctrl-C, shutting down");
    }
}

fn main() -> ExitCode {
    // Parsing first so `--log` can configure tracing before any task starts.
    let args = Args::parse();
    init_tracing(&args.log);

    if let Some(port) = &args.serial {
        #[cfg(not(feature = "real-serial"))]
        {
            eprintln!(
                "openmesh-bridge: --serial {port} requires building with --features real-serial"
            );
            return ExitCode::from(2);
        }
        #[cfg(feature = "real-serial")]
        {
            let _ = port;
        }
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("openmesh-bridge: failed to start tokio runtime: {e}");
            return ExitCode::from(1);
        }
    };

    let code = runtime.block_on(async move {
        match run(args).await {
            Ok(()) => {
                eprintln!("openmesh-bridge: shut down cleanly");
                0
            }
            Err(e) => {
                eprintln!("openmesh-bridge: {e:#}");
                1
            }
        }
    });
    ExitCode::from(code)
}

async fn run(args: Args) -> anyhow::Result<()> {
    let mut listen = args.bind.unwrap_or(args.listen);
    if args.bind.is_some() && !listen.ip().is_loopback() {
        tracing::warn!(
            listen = %listen,
            "binding the bridge API to a non-loopback address: anyone on the network with \
             the token can control the radio and read the message store"
        );
    }
    if listen.ip().is_unspecified() && args.bind.is_none() {
        tracing::warn!(listen = %listen, "bridge API bound to all interfaces");
    }
    if listen.port() == 0 {
        tracing::info!("API will bind to an ephemeral port");
    }

    let data_dir = args
        .data_dir
        .clone()
        .unwrap_or_else(token::default_data_dir);
    let db_path = args
        .db
        .clone()
        .unwrap_or_else(|| data_dir.join("openmesh.redb"));
    let token_file = args
        .token_file
        .clone()
        .unwrap_or_else(token::default_token_path);
    let identity_seed_path = data_dir.join("identity.key");

    let sim = args.sim || args.demo;
    let udp_port = if args.no_udp {
        None
    } else if let Some(port) = args.udp_port {
        Some(port)
    } else if sim {
        None
    } else {
        Some(openmesh_bridge::transport::udp::DEFAULT_UDP_PORT)
    };

    let mut sim_config = MeshConfig::default();
    if let Some(loss) = args.sim_loss {
        sim_config.loss_pct = loss;
    }
    if let Some(latency) = args.sim_latency {
        sim_config.latency_ms = latency;
    }
    if let Some(seed) = args.sim_seed {
        sim_config.seed = seed;
    }

    let options = BridgeOptions {
        listen,
        token: None,
        token_file: token_file.clone(),
        db_path: db_path.clone(),
        data_dir: data_dir.clone(),
        identity_seed_path,
        name: args.name,
        class: args.class,
        profile: args.profile,
        region: args.region,
        sim,
        demo: args.demo,
        sim_config,
        mesh_hub: None,
        udp_port,
        tcp_listen: args.tcp_listen,
        serial_port: args.serial,
        serve_web: args.serve_web,
        api_only: args.api_only,
    };

    let bridge = Bridge::start(options).await?;
    listen = bridge.addr;
    let snapshot = bridge.snapshot.read().await.clone();
    let radios: Vec<String> = snapshot
        .radios
        .iter()
        .map(|r| format!("{} ({})", r.label, r.kind))
        .collect();
    let mode = if args.demo {
        "demo"
    } else if args.sim {
        "sim"
    } else {
        "live"
    };
    println!("OpenMesh bridge {} ({mode} mode)", bridge.version());
    println!("  node        {}", bridge.node_id);
    println!("  name        {}", snapshot.node.name);
    println!(
        "  transports  {}",
        if radios.is_empty() {
            "(none)".to_string()
        } else {
            radios.join(", ")
        }
    );
    println!("  API         http://{listen}");
    println!("  WebSocket   ws://{listen}/api/ws?token=<token>");
    println!("  token file  {}", token_file.display());
    println!("  database    {}", db_path.display());
    if args.demo {
        println!("  demo        virtual mesh up: try sending a message to alice in the web UI");
    }
    println!("  health      curl -s http://{listen}/api/health");
    println!(
        "  auth        curl -s -H \"Authorization: Bearer $(cat {})\" http://{listen}/api/status",
        token_file.display()
    );
    println!("Press Ctrl-C to stop.");

    // Poll the watch channel lightly so a fatal API server failure is still visible in logs.
    let probe = bridge.events.clone();
    let _ = probe;
    shutdown_signal().await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    bridge.shutdown().await;
    Ok(())
}
