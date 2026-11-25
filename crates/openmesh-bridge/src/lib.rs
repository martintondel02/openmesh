//! # openmesh-bridge
//!
//! Local desktop/mobile bridge daemon: owns the radio transports, the protocol engine, the
//! persistent message store and the WebSocket + REST API documented in
//! `docs/architecture/WEBSOCKET-API.md`.
//!
//! The binary is a thin wrapper around [`Bridge::start`]; the library API exists so integration
//! tests can run a full daemon (including demo virtual nodes) on an ephemeral port.
//!
//! ```no_run
//! # async fn run() -> anyhow::Result<()> {
//! use openmesh_bridge::{Bridge, BridgeOptions};
//! let bridge = Bridge::start(BridgeOptions { demo: true, ..Default::default() }).await?;
//! println!("API on http://{}", bridge.addr);
//! bridge.shutdown().await;
//! # Ok(()) }
//! ```

pub mod attachments;
pub mod demo;
pub mod http;
pub mod json;
pub mod runtime;
pub mod store;
pub mod token;
pub mod transport;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use anyhow::{Context, Result};
use tokio::sync::{RwLock, broadcast, mpsc, watch};

use openmesh_core::{Engine, Identity, util};

use attachments::AttachmentStore;
use http::HttpState;
use json::{NodeSelfJson, RadioJson, StoreCountsJson};
use runtime::{CoreRuntime, EngineInput, RuntimeConfig, Shared, Snapshot};
use store::Store;
use transport::TransportHandle;
use transport::backbone::{BackboneManager, BackboneStartup, ConfigOverrides, JoinTarget};
use transport::sim::MeshConfig;

/// Options for [`Bridge::start`]. Use the CLI for the usual daemon entry point.
#[derive(Clone)]
pub struct BridgeOptions {
    pub listen: SocketAddr,
    /// Explicit token (tests); otherwise the token file is loaded or created.
    pub token: Option<String>,
    pub token_file: PathBuf,
    pub db_path: PathBuf,
    pub data_dir: PathBuf,
    pub identity_seed_path: PathBuf,
    pub name: Option<String>,
    pub class: Option<String>,
    pub profile: Option<String>,
    pub region: Option<String>,
    /// Run a virtual mesh radio instead of (or alongside) real transports.
    pub sim: bool,
    /// Demo: sim mesh + seeded virtual peers, messages and a group channel.
    pub demo: bool,
    pub sim_config: MeshConfig,
    /// Virtual-mesh hub to join. Defaults to the process-global hub so several bridge
    /// instances (and demo peers) in one process can talk to each other; tests pass an isolated
    /// hub to avoid cross-test interference.
    pub mesh_hub: Option<Arc<transport::sim::MeshHub>>,
    /// Enable the UDP LAN transport on this port; `None` disables it.
    pub udp_port: Option<u16>,
    pub tcp_listen: Option<SocketAddr>,
    /// Serial port path; only honoured when built with `--features real-serial`.
    pub serial_port: Option<String>,
    /// Directory containing the built web client (served at `/`).
    pub serve_web: Option<PathBuf>,
    /// Diagnostics mode: serve the API without running the protocol engine.
    pub api_only: bool,
    /// Start a backbone listener for this run (`openmesh-bridge backbone`).
    pub backbone_enabled: bool,
    /// CLI overrides applied on top of `<data-dir>/backbone.json`.
    pub backbone_overrides: ConfigOverrides,
    /// Outbound link to dial (`openmesh-bridge join <url>`).
    pub backbone_join: Option<JoinTarget>,
}

impl Default for BridgeOptions {
    fn default() -> Self {
        let data_dir = token::default_data_dir();
        Self {
            listen: SocketAddr::from(([127, 0, 0, 1], 8787)),
            token: None,
            token_file: token::default_token_path(),
            db_path: data_dir.join("openmesh.redb"),
            identity_seed_path: data_dir.join("identity.key"),
            data_dir,
            name: None,
            class: None,
            profile: None,
            region: None,
            sim: false,
            demo: false,
            sim_config: MeshConfig::default(),
            mesh_hub: None,
            udp_port: None,
            tcp_listen: None,
            serial_port: None,
            serve_web: None,
            api_only: false,
            backbone_enabled: false,
            backbone_overrides: ConfigOverrides::default(),
            backbone_join: None,
        }
    }
}

/// A running bridge daemon.
pub struct Bridge {
    pub addr: SocketAddr,
    pub token: String,
    pub node_id: String,
    pub snapshot: Arc<RwLock<Snapshot>>,
    pub events: broadcast::Sender<Arc<str>>,
    pub shared: Arc<Shared>,
    /// Backbone mode control surface.
    pub backbone: Arc<BackboneManager>,
    shutdown: watch::Sender<bool>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Bridge {
    /// Load persisted state, start transports, spawn the engine task and bind the API server.
    pub async fn start(options: BridgeOptions) -> Result<Self> {
        let started_at_ms = util::now_ms();

        // 1. Local state: token, identity seed, database, attachment store.
        std::fs::create_dir_all(&options.data_dir)
            .with_context(|| format!("creating data dir {}", options.data_dir.display()))?;
        if let Some(parent) = options.db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let token = match options.token.clone() {
            Some(token) => token,
            None => token::load_or_create(&options.token_file)?,
        };
        let store = Arc::new(Store::open(&options.db_path)?);
        let attachments = Arc::new(AttachmentStore::open(&options.data_dir, store.clone())?);
        let seed = token::load_or_create_seed(&options.identity_seed_path)?;
        let identity = Identity::from_seed(&seed);

        // 2. Runtime configuration: persisted, overridable from the command line.
        let mut config: RuntimeConfig = store.get_meta_json("config")?.unwrap_or_default();
        if let Some(name) = options.name.clone() {
            config.name = name;
        }
        if let Some(class) = options.class.clone() {
            config.class = class;
        }
        if let Some(profile) = options.profile.clone() {
            config.profile = profile;
        }
        if let Some(region) = options.region.clone() {
            config.region = region;
        }

        // 3. Protocol engine with persisted contacts and channels.
        let mut engine = Engine::new(identity, config.engine_config());
        runtime::restore_engine_state(&mut engine, &store)?;
        let node_id = engine.node_id();
        let bridge_ident = engine.public_identity();

        // 4. Channels + task plumbing.
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (commands_tx, commands_rx) = mpsc::channel::<EngineInput>(1024);
        let (inbound_tx, inbound_rx) = mpsc::channel::<transport::Inbound>(1024);
        let (events, _) = broadcast::channel::<Arc<str>>(1024);
        let snapshot = Arc::new(RwLock::new(Snapshot::default()));
        // Bind the local API early so the backbone setup payload reports the real port even when
        // `--listen 127.0.0.1:0` was requested.
        let api_listener = tokio::net::TcpListener::bind(options.listen)
            .await
            .with_context(|| format!("binding API listener on {}", options.listen))?;
        let addr = api_listener.local_addr()?;
        let backbone_envelopes = Arc::new(AtomicUsize::new(0));
        let backbone = BackboneManager::new(
            options.data_dir.clone(),
            node_id,
            addr,
            std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("openmesh-bridge")),
            options.serve_web.clone(),
            Some(options.token_file.clone()),
            events.clone(),
            commands_tx.clone(),
            shutdown_rx.clone(),
            backbone_envelopes.clone(),
        );

        // 5. Transports.
        let mut transports: Vec<TransportHandle> = Vec::new();
        let mut link_id: u16 = 1;
        let mut task_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();

        // Adapter: transport inbound frames -> engine task input.
        {
            let commands_tx = commands_tx.clone();
            task_handles.push(tokio::spawn(async move {
                let mut inbound_rx = inbound_rx;
                while let Some(inbound) = inbound_rx.recv().await {
                    if commands_tx
                        .send(EngineInput::Inbound(inbound))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }

        let mesh_hub = options
            .mesh_hub
            .clone()
            .unwrap_or_else(transport::sim::MeshHub::global);
        if options.sim || options.demo {
            let hub = mesh_hub.clone();
            hub.configure(options.sim_config);
            transports.push(transport::sim::spawn(
                hub.clone(),
                node_id,
                link_id,
                inbound_tx.clone(),
                shutdown_rx.clone(),
            ));
            link_id += 1;
        }
        if let Some(port) = options.udp_port {
            match transport::udp::spawn(
                transport::udp::UdpConfig { port },
                node_id,
                link_id,
                inbound_tx.clone(),
                shutdown_rx.clone(),
            ) {
                Ok(handle) => {
                    transports.push(handle);
                    link_id += 1;
                }
                Err(e) => tracing::warn!(error = %e, "UDP transport disabled"),
            }
        }
        if let Some(listen) = options.tcp_listen {
            match transport::tcp::spawn(listen, link_id, inbound_tx.clone(), shutdown_rx.clone()) {
                Ok(handle) => {
                    transports.push(handle);
                    link_id += 1;
                }
                Err(e) => tracing::warn!(error = %e, "TCP transport disabled"),
            }
        }
        if let Some(port) = options.serial_port.clone() {
            #[cfg(feature = "real-serial")]
            {
                match transport::serial::spawn(
                    transport::serial::SerialConfig {
                        port,
                        baud: 115_200,
                    },
                    link_id,
                    inbound_tx.clone(),
                    shutdown_rx.clone(),
                ) {
                    Ok(handle) => {
                        transports.push(handle);
                        link_id += 1;
                    }
                    Err(e) => tracing::warn!(error = %e, "serial transport disabled"),
                }
            }
            #[cfg(not(feature = "real-serial"))]
            {
                anyhow::bail!(
                    "--serial {port} requires building openmesh-bridge with --features real-serial"
                );
            }
        }
        transports.push(backbone.transport_handle(link_id));
        link_id = link_id.wrapping_add(1);
        let _ = link_id;
        if transports.is_empty() && !options.api_only {
            tracing::warn!("no transports configured: the bridge will only serve its local API");
        }
        if !transports.is_empty() {
            config.interface = transports
                .iter()
                .map(|t| t.kind)
                .collect::<Vec<_>>()
                .join("+");
        }
        if let Err(e) = store.set_meta_json("config", &config) {
            tracing::warn!(error = %e, "failed to persist runtime config");
        }

        // 6. Demo seeding (needs the engine and store before the engine task takes ownership).
        if options.demo {
            if let Err(e) = demo::seed_demo(&store, &mut engine, &bridge_ident, started_at_ms) {
                tracing::warn!(error = %e, "demo seeding failed");
            }
        }

        // 7. Backbone mode: load config, start listener/client before the engine task takes
        // ownership (the manager only queues engine inputs and broadcasts events).
        let startup = BackboneStartup {
            enabled: options.backbone_enabled,
            overrides: options.backbone_overrides.clone(),
            join: options.backbone_join.clone(),
            token_from_file: None,
            explicit: options.backbone_enabled || options.backbone_join.is_some(),
        };
        backbone
            .startup(&startup)
            .await
            .map_err(anyhow::Error::msg)?;

        // 8. API snapshot + engine task. Seed the snapshot synchronously so the first HTTP/WS
        // request already sees persisted state instead of racing the engine task.
        {
            let initial = runtime::build_snapshot(runtime::SnapshotInput {
                engine: &engine,
                store: &store,
                attachments: &attachments,
                transports: &transports,
                config: &config,
                started_at_ms,
                last_rssi: None,
                last_snr: None,
                backbone_configured: backbone.is_configured(),
                backbone_running: backbone.is_running(),
            });
            *snapshot.write().await = initial;
        }

        let shared = Arc::new(Shared {
            snapshot: snapshot.clone(),
            store: store.clone(),
            attachments: attachments.clone(),
            events: events.clone(),
            commands: commands_tx.clone(),
            started_at_ms,
            backbone: backbone.clone(),
            backbone_envelopes: backbone_envelopes.clone(),
        });

        if options.api_only {
            drop(engine);
            tracing::warn!("api-only mode: protocol engine is not running");
        } else {
            let engine_task = core_task(
                engine,
                store.clone(),
                attachments.clone(),
                snapshot.clone(),
                events.clone(),
                config.clone(),
                transports.clone(),
                started_at_ms,
                commands_rx,
                shutdown_rx.clone(),
                backbone.clone(),
                backbone_envelopes.clone(),
            );
            task_handles.push(tokio::spawn(engine_task));

            // 1 Hz tick drives announces, retries, status events and snapshot refresh.
            let commands_tx = commands_tx.clone();
            let mut tick_shutdown = shutdown_rx.clone();
            task_handles.push(tokio::spawn(async move {
                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(1));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    tokio::select! {
                        _ = tick_shutdown.changed() => break,
                        _ = ticker.tick() => {
                            if commands_tx.send(EngineInput::Tick).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }));
        }

        // 9. HTTP/WS API server (listener bound early above).
        let web_root = http::resolve_web_root(options.serve_web.clone());
        if let Some(root) = &web_root {
            tracing::info!(path = %root.display(), "serving web client");
        }
        let http_state = Arc::new(HttpState {
            token: Arc::new(token.clone()),
            shared: shared.clone(),
            backbone: backbone.clone(),
            web_root,
            shutdown: shutdown_rx.clone(),
        });
        task_handles.push(tokio::spawn(async move {
            if let Err(e) = http::serve(api_listener, http_state).await {
                tracing::error!(error = %e, "HTTP server stopped unexpectedly");
            }
        }));

        // 10. Demo virtual peers join the sim mesh after the bridge transport exists.
        if options.demo {
            let peers = demo::spawn_peers(&mesh_hub, &bridge_ident, shutdown_rx.clone());
            tracing::info!(peers = peers.len(), "demo virtual peers running");
            task_handles.extend(peers);
        }

        Ok(Bridge {
            addr,
            token,
            node_id: node_id.to_hex(),
            snapshot,
            events,
            shared,
            backbone,
            shutdown: shutdown_tx,
            tasks: task_handles,
        })
    }

    /// The bridge version reported by `/api/health` and `hello`.
    pub fn version(&self) -> &'static str {
        openmesh_core::CRATE_VERSION
    }

    /// A summary of the node identity advertised in `hello`.
    pub fn node(&self) -> NodeSelfJson {
        NodeSelfJson {
            node_id: self.node_id.clone(),
            name: String::new(),
            class: "client".to_string(),
            caps: 0,
        }
    }

    /// Store counts as shown in `hello.store`.
    pub async fn store_counts(&self) -> StoreCountsJson {
        let snapshot = self.snapshot.read().await;
        snapshot.store.clone()
    }

    /// Radios as shown in `hello.radios`.
    pub async fn radios(&self) -> Vec<RadioJson> {
        self.snapshot.read().await.radios.clone()
    }

    /// Stop all bridge tasks and release the listeners.
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        self.backbone.shutdown().await;
        let tasks = self.tasks;
        let join_all = futures_util::future::join_all(tasks);
        if tokio::time::timeout(std::time::Duration::from_secs(3), join_all)
            .await
            .is_err()
        {
            tracing::warn!("some bridge tasks did not stop in time");
        }
    }
}

/// The engine task: owns [`Engine`] and serializes all protocol state access.
#[allow(clippy::too_many_arguments)]
async fn core_task(
    engine: Engine,
    store: Arc<Store>,
    attachments: Arc<AttachmentStore>,
    snapshot: Arc<RwLock<Snapshot>>,
    events: broadcast::Sender<Arc<str>>,
    config: RuntimeConfig,
    transports: Vec<TransportHandle>,
    started_at_ms: u64,
    mut commands: mpsc::Receiver<EngineInput>,
    mut shutdown: watch::Receiver<bool>,
    backbone: Arc<BackboneManager>,
    backbone_envelopes: Arc<AtomicUsize>,
) {
    let mut runtime = CoreRuntime {
        engine,
        store,
        attachments,
        snapshot,
        events,
        config,
        transports,
        started_at_ms,
        last_announce_ms: 0,
        last_rssi: None,
        last_snr: None,
        outgoing_resources: std::collections::HashMap::new(),
        incoming_resources: std::collections::HashMap::new(),
        pending_resources: std::collections::HashMap::new(),
        backbone,
        backbone_envelopes,
        shutdown: shutdown.clone(),
    };
    // Introduce ourselves immediately so nearby nodes learn the identity.
    let now = util::now_ms();
    if let Ok(processed) = runtime.engine.announce(now) {
        runtime.process(processed, now).await;
        runtime.last_announce_ms = now;
    }
    runtime.refresh_snapshot().await;
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            input = commands.recv() => {
                let Some(input) = input else { break };
                let now = util::now_ms();
                match input {
                    EngineInput::Inbound(inbound) => {
                        let exclude = inbound.link.as_ref().and_then(|link| link.peer);
                        if let Some(link) = &inbound.link {
                            if link.rssi.is_some() {
                                runtime.last_rssi = link.rssi;
                            }
                            if link.snr.is_some() {
                                runtime.last_snr = link.snr;
                            }
                        }
                        let processed = runtime.engine.ingest(&inbound.bytes, inbound.link, now);
                        runtime.process_from(processed, now, exclude).await;
                        runtime.refresh_snapshot().await;
                    }
                    EngineInput::SetStoreForward(enabled) => {
                        runtime.set_store_forward(enabled).await;
                    }
                    EngineInput::Command { cmd, reply } => {
                        runtime.handle_command(cmd, reply, now).await;
                        runtime.refresh_snapshot().await;
                    }
                    EngineInput::Tick => runtime.tick(now).await,
                }
            }
        }
    }
    tracing::debug!("engine task stopped");
}
