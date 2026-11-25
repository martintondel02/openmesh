//! `openmesh-bridge` — OpenMesh local bridge daemon and backbone server/client.
//!
//! Serves the WebSocket + REST API from `docs/architecture/WEBSOCKET-API.md`, owns the radio
//! transports, persists messages/attachments, and implements Backbone mode
//! (`docs/architecture/BACKBONE.md`).
//!
//! ```text
//! openmesh-bridge --demo                        # virtual mesh + seeded demo nodes/messages
//! openmesh-bridge --sim                         # virtual mesh, empty state
//! openmesh-bridge                              # UDP LAN on :4403 + local API on :8787
//! openmesh-bridge --serial /dev/ttyACM0        # requires --features real-serial
//! openmesh-bridge --serve-web apps/web/dist     # also serve the built web client
//! openmesh-bridge backbone                     # always-on authenticated OBB/1 backbone server
//! openmesh-bridge backbone --print-setup        # print onboarding payload and exit
//! openmesh-bridge join '<openmesh://backbone?...>'   # outbound link to a backbone
//! openmesh-bridge install-service --backbone   # hardened systemd unit + enable commands
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use openmesh_bridge::transport::backbone::{self, ConfigOverrides, JoinTarget};
use openmesh_bridge::transport::sim::MeshConfig;
use openmesh_bridge::{Bridge, BridgeOptions, token};

#[derive(Debug, Parser)]
#[command(
    name = "openmesh-bridge",
    version,
    about = "OpenMesh local bridge daemon and OBB/1 backbone server/client",
    after_help = "Subcommands: backbone (always-on authed relay), join (outbound link), install-service (systemd unit)."
)]
#[derive(Clone)]
struct Args {
    /// API listen address (loopback by default).
    #[arg(
        long,
        global = true,
        default_value = "127.0.0.1:8787",
        value_name = "ADDR"
    )]
    listen: SocketAddr,

    /// Bind address alias for the local API; binding a non-loopback address is unsafe.
    #[arg(long, value_name = "ADDR", conflicts_with = "listen")]
    bind: Option<SocketAddr>,

    /// File holding the local API bearer token (created with mode 0600 on first run).
    #[arg(long, global = true, value_name = "PATH")]
    token_file: Option<PathBuf>,

    /// Local API bearer token value (overrides --token-file; for provisioning/tests).
    #[arg(long, global = true, value_name = "TOKEN")]
    token: Option<String>,

    /// redb database path.
    #[arg(long, global = true, value_name = "PATH")]
    db: Option<PathBuf>,

    /// Data directory (identity seed, attachments, default db, backbone.json).
    #[arg(long, global = true, value_name = "DIR")]
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

    /// Optional TCP listener for pre-v0.2 raw WAN bridge links.
    #[arg(long, value_name = "ADDR")]
    tcp_listen: Option<SocketAddr>,

    /// Serial radio port, e.g. /dev/ttyACM0 (requires the real-serial feature).
    #[arg(long, value_name = "PORT")]
    serial: Option<String>,

    /// Directory of the built web client to serve at `/`.
    #[arg(long, global = true, value_name = "DIR")]
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
    #[arg(
        long,
        global = true,
        default_value = "info",
        env = "RUST_LOG",
        value_name = "FILTER"
    )]
    log: String,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
enum Command {
    /// Run an always-on authenticated OBB/1 backbone server (public TCP relay + panel).
    Backbone(BackboneArgs),
    /// Dial a backbone and keep an outbound link, reconnecting with backoff.
    Join(JoinArgs),
    /// Write a hardened systemd unit for the bridge/backbone daemon and print enable commands.
    InstallService(InstallServiceArgs),
    /// Run the Model Context Protocol server on stdio (Claude Desktop, Cursor, DSH, ...).
    Mcp,
}

#[derive(Debug, Clone, clap::Args)]
struct BackboneArgs {
    /// Backbone TCP bind address (default 0.0.0.0 in backbone mode; prints a warning).
    #[arg(long, default_value = "0.0.0.0", value_name = "ADDR")]
    pub bind: String,

    /// Backbone TCP port (OBB/1 listener).
    #[arg(long, default_value_t = 9443, value_name = "PORT")]
    pub port: u16,

    /// Disable token authentication (trusted LANs only; shown as a red warning).
    #[arg(long)]
    pub open: bool,

    /// Maximum concurrently connected peers (1..=256).
    #[arg(long, value_name = "N")]
    pub max_peers: Option<usize>,

    /// Comma-separated hex node ids allowed to connect; empty means anyone with the token.
    #[arg(long, value_name = "ID,ID")]
    pub allowlist: Option<String>,

    /// Backbone display name shown in peer tables and the setup payload.
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,

    /// Public hostname used in the shareable connection string (falls back to the detected IP).
    #[arg(long, value_name = "HOST")]
    pub public_host: Option<String>,

    /// Disable store-and-forward on the backbone engine.
    #[arg(long)]
    pub no_store_forward: bool,

    /// Print the onboarding payload and exit without starting the server.
    #[arg(long)]
    pub print_setup: bool,

    /// Provision the OBB/1 token directly (base64url 32 bytes); stored in backbone.json 0600.
    #[arg(long, value_name = "TOKEN")]
    pub backbone_token: Option<String>,

    /// Read the OBB/1 token from a file (created with a fresh token if missing).
    #[arg(long, value_name = "PATH")]
    pub backbone_token_file: Option<PathBuf>,
}

#[derive(Debug, Clone, clap::Args)]
struct JoinArgs {
    /// `openmesh://backbone?host=...&port=...&token=...` connection string.
    #[arg(value_name = "URL")]
    pub url: String,
}

#[derive(Debug, Clone, clap::Args)]
struct InstallServiceArgs {
    /// Write the backbone service unit instead of the plain bridge unit.
    #[arg(long)]
    pub backbone: bool,

    /// Write a system unit under /etc/systemd/system (requires root).
    #[arg(long)]
    pub system: bool,

    /// Backbone TCP port embedded in ExecStart (ignored without --backbone).
    #[arg(long, default_value_t = 9443, value_name = "PORT")]
    pub port: u16,
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
        let result = match args.command.clone() {
            Some(Command::Backbone(backbone_args)) => run_backbone(args, backbone_args).await,
            Some(Command::Join(join_args)) => run_join(args, join_args).await,
            Some(Command::InstallService(install_args)) => {
                install_service(args, install_args).await
            }
            Some(Command::Mcp) => openmesh_bridge::mcp::run_stdio().await,
            None => run_bridge(args).await,
        };
        match result {
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

/// Build the shared [`BridgeOptions`] from the top-level flags.
fn base_options(args: &Args) -> BridgeOptions {
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

    BridgeOptions {
        listen: args.listen,
        token: args.token.clone(),
        token_file,
        db_path,
        data_dir,
        identity_seed_path,
        name: args.name.clone(),
        class: args.class.clone(),
        profile: args.profile.clone(),
        region: args.region.clone(),
        sim,
        demo: args.demo,
        sim_config,
        mesh_hub: None,
        udp_port,
        tcp_listen: args.tcp_listen,
        serial_port: args.serial.clone(),
        serve_web: args.serve_web.clone(),
        api_only: args.api_only,
        backbone_enabled: false,
        backbone_overrides: ConfigOverrides::default(),
        backbone_join: None,
    }
}

async fn run_bridge(mut args: Args) -> anyhow::Result<()> {
    let mut listen = args.bind.unwrap_or(args.listen);
    if args.bind.is_some() && !listen.ip().is_loopback() {
        tracing::warn!(
            listen = %listen,
            "binding the bridge API to a non-loopback address: anyone on the network with \
             the token can control the radio and read the message store"
        );
    }
    let options = base_options(&args);
    let bridge = Bridge::start(options).await?;
    listen = bridge.addr;
    args.listen = listen;

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
    let token_file = args
        .token_file
        .clone()
        .unwrap_or_else(token::default_token_path);
    let data_dir = bridge.shared.store.path().parent().map(|p| p.to_path_buf());
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
    if let Some(dir) = &data_dir {
        println!("  database    {}", dir.join("openmesh.redb").display());
    }
    if args.demo {
        println!("  demo        virtual mesh up: try sending a message to alice in the web UI");
    }
    println!("  health      curl -s http://{listen}/api/health");
    println!(
        "  auth        curl -s -H \"Authorization: Bearer $(cat {})\" http://{listen}/api/status",
        token_file.display()
    );
    println!("Press Ctrl-C to stop.");

    shutdown_signal().await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    bridge.shutdown().await;
    Ok(())
}

async fn run_backbone(args: Args, bb: BackboneArgs) -> anyhow::Result<()> {
    let data_dir = args
        .data_dir
        .clone()
        .unwrap_or_else(token::default_data_dir);
    let config_existed = data_dir.join("backbone.json").exists();
    let mut overrides = ConfigOverrides {
        enabled: Some(true),
        bind: Some(bb.bind.clone()),
        port: Some(bb.port),
        max_peers: bb.max_peers,
        name: bb.name.clone(),
        public_host: bb.public_host.clone(),
        store_forward: bb.no_store_forward.then_some(false),
        auth: bb.open.then(|| "open".to_string()),
        ..ConfigOverrides::default()
    };
    if let Some(list) = &bb.allowlist {
        overrides.allowlist = Some(
            list.split(',')
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
                .collect(),
        );
    }
    // OBB/1 token provisioning (extension flags; the §9 `--token-file` is the API token).
    if let Some(token) = &bb.backbone_token {
        backbone::wire::token_key(token).map_err(anyhow::Error::msg)?;
        overrides.token = Some(token.clone());
    } else if let Some(path) = &bb.backbone_token_file {
        let token = match std::fs::read_to_string(path) {
            Ok(token) if !token.trim().is_empty() => token.trim().to_string(),
            _ => {
                let token = backbone::wire::generate_token();
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(path, format!("{token}\n"))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
                }
                println!("Generated backbone token in {}", path.display());
                token
            }
        };
        backbone::wire::token_key(&token).map_err(anyhow::Error::msg)?;
        overrides.token = Some(token);
    }
    if !args.listen.ip().is_loopback() {
        tracing::warn!(
            listen = %args.listen,
            "backbone API is not loopback-only; keep the panel port firewalled"
        );
    }
    if bb.bind == "0.0.0.0" || bb.bind == "::" {
        tracing::warn!(
            bind = %bb.bind,
            port = bb.port,
            "backbone listener is reachable from the network; this is expected for a backbone, \
             keep only the OBB/1 port exposed and use TLS/WireGuard on hostile networks"
        );
    }

    let mut options = base_options(&args);
    options.backbone_enabled = true;
    options.backbone_overrides = overrides;
    options.udp_port = if args.no_udp { None } else { args.udp_port };
    let bridge = Bridge::start(options).await?;

    let setup = bridge.backbone.setup().await;
    print_setup(&setup, !config_existed);
    if bb.print_setup {
        bridge.shutdown().await;
        return Ok(());
    }

    let token_file = args
        .token_file
        .clone()
        .unwrap_or_else(token::default_token_path);
    println!();
    println!(
        "Backbone API panel: http://{}/backbone   (API token file: {})",
        bridge.addr,
        token_file.display()
    );
    println!(
        "Backbone listener:  {}    config: {}",
        setup.get("listen").and_then(|v| v.as_str()).unwrap_or("?"),
        bridge.backbone.config_path().display()
    );
    if !config_existed {
        println!();
        println!(
            "First run: the OBB/1 token is only printed above/once. It is stored in {} (mode 0600)",
            bridge.backbone.config_path().display()
        );
        println!("and can be rotated from the panel or `openmesh-cli backbone rotate-token`.");
    }
    println!("Press Ctrl-C to stop.");

    shutdown_signal().await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    bridge.shutdown().await;
    Ok(())
}

async fn run_join(args: Args, join: JoinArgs) -> anyhow::Result<()> {
    let target = JoinTarget::from_url(&join.url).map_err(anyhow::Error::msg)?;
    let remote = target.connection.authority();
    let mut options = base_options(&args);
    options.backbone_join = Some(target.with_backoff(1_000, 30_000));
    options.udp_port = if args.no_udp { None } else { args.udp_port };
    let bridge = Bridge::start(options).await?;
    let token_file = args
        .token_file
        .clone()
        .unwrap_or_else(token::default_token_path);
    println!(
        "OpenMesh bridge {} joining backbone {}",
        bridge.version(),
        remote
    );
    println!("  node        {}", bridge.node_id);
    println!("  API         http://{}", bridge.addr);
    println!("  token file  {}", token_file.display());
    println!("  backoff     1s..30s (exponential, jittered)");
    println!(
        "  inspect     openmesh-cli backbone info / peers  (token from {})",
        token_file.display()
    );
    println!("Press Ctrl-C to stop.");
    shutdown_signal().await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    bridge.shutdown().await;
    Ok(())
}

fn print_setup(setup: &serde_json::Value, first_run: bool) {
    println!();
    if setup.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
        println!("Backbone mode is not configured yet.");
        return;
    }
    if first_run {
        println!("=== OpenMesh backbone first-run onboarding ===");
    } else {
        println!("=== OpenMesh backbone setup ===");
    }
    for key in ["listen", "public_host", "auth"] {
        if let Some(value) = setup.get(key) {
            println!(
                "  {key:<12} {}",
                value.as_str().unwrap_or(&value.to_string())
            );
        }
    }
    if let Some(connection) = setup.get("connection_string").and_then(|v| v.as_str()) {
        println!("  connection   {connection}");
        println!("  (the token in the connection string is a bearer secret — share privately)");
    }
    if let Some(steps) = setup.get("steps").and_then(|v| v.as_array()) {
        println!();
        for (index, step) in steps.iter().enumerate() {
            let title = step.get("title").and_then(|v| v.as_str()).unwrap_or("");
            println!("  {}. {title}", index + 1);
            if let Some(commands) = step.get("commands").and_then(|v| v.as_array()) {
                for command in commands {
                    if let Some(command) = command.as_str() {
                        println!("       $ {command}");
                    }
                }
            }
            if let Some(note) = step.get("note").and_then(|v| v.as_str()) {
                println!("       {note}");
            }
        }
    }
    if let Some(firewall) = setup.get("firewall") {
        println!();
        println!("  Firewall:");
        if let Some(commands) = firewall.get("firewalld").and_then(|v| v.as_array()) {
            for command in commands {
                if let Some(command) = command.as_str() {
                    println!("    $ {command}");
                }
            }
        }
    }
    if let Some(notes) = setup.get("notes").and_then(|v| v.as_array()) {
        println!();
        for note in notes {
            if let Some(note) = note.as_str() {
                println!("  note: {note}");
            }
        }
    }
    println!();
    println!("  systemd: openmesh-bridge install-service --backbone   (unit body below)");
    if let Some(unit) = setup.get("systemd_unit").and_then(|v| v.as_str()) {
        for line in unit.lines() {
            println!("    {line}");
        }
    }
}

fn user_unit_path(backbone: bool) -> anyhow::Result<PathBuf> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| {
            anyhow::anyhow!("cannot determine the user config directory (HOME unset)")
        })?;
    Ok(config_home.join("systemd").join("user").join(if backbone {
        "openmesh-backbone.service"
    } else {
        "openmesh-bridge.service"
    }))
}

async fn install_service(args: Args, install: InstallServiceArgs) -> anyhow::Result<()> {
    let executable = std::env::current_exe()?;
    let data_dir = args
        .data_dir
        .clone()
        .unwrap_or_else(token::default_data_dir);
    let unit = backbone::systemd_unit(
        install.backbone,
        &executable,
        &data_dir,
        args.listen,
        install.port,
        args.serve_web.as_deref(),
        args.token_file.as_deref(),
        install.system,
    );
    let service_name = if install.backbone {
        "openmesh-backbone.service"
    } else {
        "openmesh-bridge.service"
    };
    if install.system {
        let path = PathBuf::from("/etc/systemd/system").join(service_name);
        match std::fs::write(&path, &unit) {
            Ok(()) => {
                println!("Wrote {path}", path = path.display());
            }
            Err(e) => {
                println!(
                    "Could not write {path} ({e}); run as root or use:",
                    path = path.display()
                );
                println!();
                println!("sudo tee {path} >/dev/null <<'EOF'", path = path.display());
                print!("{unit}");
                println!("EOF");
                println!();
            }
        }
        println!("Then enable it:");
        println!("  sudo systemctl daemon-reload");
        println!("  sudo systemctl enable --now {service_name}");
        println!("  sudo systemctl status {service_name}");
    } else {
        let path = user_unit_path(install.backbone)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, &unit)?;
        println!("Wrote {}", path.display());
        println!();
        println!("Enable it with:");
        println!("  systemctl --user daemon-reload");
        println!("  systemctl --user enable --now {service_name}");
        println!("  loginctl enable-linger $USER");
        println!();
        println!("Service file:");
        print!("{unit}");
    }
    Ok(())
}
