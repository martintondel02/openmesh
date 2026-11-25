//! `openmesh-cli` — terminal client and diagnostics for the OpenMesh bridge.
//!
//! Two modes:
//! * connected to a running bridge over its documented WebSocket API (`--ws ws://...`), or
//! * standalone (`--sim`), running the protocol in-process through the mesh simulator so the
//!   commands work with no daemon at all.
//!
//! ```text
//! openmesh-cli send --to aabbccddeeff0011 --text "hello"
//! openmesh-cli listen
//! openmesh-cli announce
//! openmesh-cli nodes
//! openmesh-cli airtime --profile MEDIUM_FAST --bytes 5120
//! openmesh-cli --sim send --to local --text "hello"       # no bridge needed
//!
//! Backbone diagnostics (token from ~/.config/openmesh/bridge-token, --token or $OPENMESH_TOKEN):
//!
//! ```text
//! openmesh-cli backbone info
//! openmesh-cli backbone peers
//! openmesh-cli backbone kick <node_id>
//! openmesh-cli backbone rotate-token
//! openmesh-cli backbone connect 'openmesh://backbone?host=...&port=9443&token=...'
//! openmesh-cli backbone disconnect <node_id>
//! ```

use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use openmesh_core::util;
use openmesh_sim::{MeshSim, SimConfig};

#[derive(Debug, Parser)]
#[command(
    name = "openmesh-cli",
    version,
    about = "OpenMesh terminal client, diagnostics and airtime calculator",
    // `help [topic]` is an explicit subcommand reading the help center from the bridge, so
    // clap's generated help subcommand must be disabled to avoid the duplicate-name panic.
    disable_help_subcommand = true
)]
struct Cli {
    /// Bridge WebSocket URL (default ws://127.0.0.1:8787/api/ws).
    #[arg(long, global = true, value_name = "URL")]
    ws: Option<String>,

    /// Bridge REST API base URL (default http://127.0.0.1:8787, or derived from --ws).
    #[arg(long, global = true, value_name = "URL")]
    api: Option<String>,

    /// API bearer token (overrides the token file).
    #[arg(long, global = true, value_name = "TOKEN")]
    token: Option<String>,

    /// File holding the API bearer token.
    #[arg(long, global = true, value_name = "PATH")]
    token_file: Option<std::path::PathBuf>,

    /// Standalone mode: run the protocol in-process, no bridge daemon needed.
    #[arg(long, global = true)]
    sim: bool,

    /// Print raw JSON responses/events instead of human output.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Subcommand)]
enum BackboneCommand {
    /// Show backbone configuration, mode and live totals.
    Info,
    /// List connected backbone peers.
    Peers,
    /// Disconnect one peer by node id.
    Kick {
        /// Peer node id (hex, 8 bytes).
        node_id: String,
    },
    /// Rotate the backbone token (returned once; old token invalid for new handshakes).
    RotateToken,
    /// Add an outbound link to a backbone connection string.
    Connect {
        /// `openmesh://backbone?host=...&port=...&token=...` URL.
        url: String,
    },
    /// Disconnect an outbound backbone link by node id.
    Disconnect {
        /// Backbone server node id (hex).
        node_id: String,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum AgentCommand {
    /// Print a ready-to-paste system prompt for an AI agent (beginner or advanced).
    Prompt {
        /// Audience tier: beginner explains jargon, advanced includes exact numbers.
        #[arg(long, default_value = "beginner", value_name = "TIER")]
        tier: String,
    },
    /// Print the MCP client config JSON for this bridge.
    Config {
        /// MCP client: claude, cursor, dsh or generic.
        #[arg(long, default_value = "generic", value_name = "CLIENT")]
        client: String,
    },
    /// Show recent mutating actions (source, action, result).
    Activity {
        /// How many entries to show (max 256).
        #[arg(long, default_value_t = 20, value_name = "N")]
        limit: usize,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum ConfigCommand {
    /// Show the current config, legal maxima for the region and override state.
    Show,
    /// Change the regulatory override (emergency use; requires --i-understand-risk when it
    /// exceeds the legal maxima, otherwise the command exits 2 without changing anything).
    Override {
        /// Override duty cycle in percent (0 = no transmissions, legal way to go silent).
        #[arg(long, value_name = "PCT")]
        duty_pct: Option<u8>,
        /// Override TX power ceiling in dBm EIRP.
        #[arg(long, value_name = "DBM")]
        power_dbm: Option<i8>,
        /// Override centre frequency in kHz.
        #[arg(long, value_name = "KHZ")]
        frequency_khz: Option<u32>,
        /// Short reason stored in the activity log (max 120 characters).
        #[arg(long, value_name = "TEXT")]
        reason: Option<String>,
        /// Required acknowledgement when the override exceeds the legal maxima.
        #[arg(long = "i-understand-risk")]
        i_understand_risk: bool,
        /// Disable the override (always allowed, no acknowledgement needed).
        #[arg(long)]
        disable: bool,
    },
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Send a direct message (and wait for the delivery ack).
    Send {
        /// Recipient node id (hex, 8 bytes).
        #[arg(long, value_name = "ID")]
        to: String,
        /// Message text.
        #[arg(long, value_name = "TEXT", conflicts_with = "attachment")]
        text: Option<String>,
        /// Attach a file (uploaded via POST /api/attachments, then sent as an attachment message).
        #[arg(long, value_name = "PATH")]
        attachment: Option<std::path::PathBuf>,
        /// Only wait for `send_result`, not for the delivery ack.
        #[arg(long)]
        no_wait: bool,
        /// Ack wait timeout in seconds.
        #[arg(long, default_value_t = 30, value_name = "SECS")]
        timeout: u64,
    },
    /// Stream bridge events (messages, acks, nodes, ...) to the terminal.
    Listen {
        /// Only print these event types (comma separated), e.g. message,group_message,ack.
        #[arg(long, value_name = "TYPES")]
        types: Option<String>,
        /// In --sim mode, number of simulated messages before exiting.
        #[arg(long, default_value_t = 5, value_name = "N")]
        count: u32,
    },
    /// Force a mesh announce now.
    Announce,
    /// List nodes known to the bridge (or to the local simulator).
    Nodes,
    /// Show bridge/driver statistics.
    Stats,
    /// List joined channels.
    Channels,
    /// Inspect and control backbone mode through the bridge REST API.
    Backbone {
        #[command(subcommand)]
        command: Option<BackboneCommand>,
    },
    /// Agent/AI-native helpers: system prompt, MCP config, activity log.
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// Inspect and change the bridge config (including the regulatory override).
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Read the help center topics (same content as the web Help tab and MCP tools).
    Help {
        /// Topic slug, e.g. getting-started; omit to list all topics.
        topic: Option<String>,
    },
    /// Estimate LoRa airtime for a payload.
    Airtime {
        /// Radio profile: LONG_SLOW, LONG_FAST, MEDIUM_FAST, SHORT_FAST, SHORT_TURBO.
        #[arg(long, default_value = "MEDIUM_FAST", value_name = "PROFILE")]
        profile: String,
        /// Payload size in bytes.
        #[arg(long, default_value_t = 5120, value_name = "BYTES")]
        bytes: usize,
        /// Regulatory region for duty-cycle budgeting.
        #[arg(long, default_value = "EU868", value_name = "REGION")]
        region: String,
        /// Override duty cycle in percent (shows legal maxima + effective duty).
        #[arg(long, value_name = "PCT")]
        duty_override: Option<u8>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("openmesh-cli: failed to start tokio runtime: {e}");
            return ExitCode::from(1);
        }
    };
    match runtime.block_on(run(cli)) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("openmesh-cli: {e:#}");
            ExitCode::from(1)
        }
    }
}

async fn run(cli: Cli) -> Result<u8> {
    match &cli.command {
        Command::Backbone { command } => backbone_command(&cli, command.as_ref()).await,
        Command::Agent { command } => agent_command(&cli, command).await,
        Command::Config { command } => config_command(&cli, command).await,
        Command::Help { topic } => help_command(&cli, topic.as_deref()).await,
        Command::Airtime {
            profile,
            bytes,
            region,
            duty_override,
        } => airtime(profile, *bytes, region, *duty_override),
        Command::Send {
            to,
            text,
            attachment,
            no_wait,
            timeout,
        } => {
            if text.is_none() && attachment.is_none() {
                bail!("send: provide --text or --attachment");
            }
            if let Some(path) = attachment {
                let data = tokio::fs::read(path)
                    .await
                    .with_context(|| format!("reading attachment {}", path.display()))?;
                let mime = mime_for_path(path);
                if cli.sim {
                    sim_send_attachment(&cli, text.as_deref(), &mime, &data)
                } else {
                    ws_send(
                        &cli,
                        to,
                        text.as_deref(),
                        Some((mime, data)),
                        *no_wait,
                        *timeout,
                    )
                    .await
                }
            } else if cli.sim {
                sim_send_text(&cli, text.as_deref().unwrap_or(""))
            } else {
                ws_send(&cli, to, text.as_deref(), None, *no_wait, *timeout).await
            }
        }
        Command::Listen { types, count } => {
            if cli.sim {
                sim_listen(&cli, *count).await
            } else {
                ws_listen(&cli, types.as_deref()).await
            }
        }
        Command::Announce => {
            if cli.sim {
                let mut sim = two_node_sim(1);
                sim.announce_all();
                sim.run_until(500);
                println!("announced {} nodes on the simulated mesh", sim.nodes.len());
                Ok(0)
            } else {
                ws_command(&cli, "announce", json!({}), |typ, _data| typ == "sync_done").await
            }
        }
        Command::Nodes => {
            if cli.sim {
                let mut sim = two_node_sim(17);
                sim.announce_all();
                sim.run_until(5_000);
                print_node_table(&sim, 0);
                Ok(0)
            } else {
                let mut ws = WsClient::connect(&cli).await?;
                ws.send_command("get_nodes", json!({}), "n1").await?;
                let (typ, mut data) = ws.wait_for(&["nodes", "error"], "n1", 5_000).await?;
                if typ == "error" {
                    bail!("bridge error: {data}");
                }
                print_nodes(&mut data);
                Ok(0)
            }
        }
        Command::Stats => match ws_stats(&cli).await {
            Ok(()) => Ok(0),
            Err(e) => {
                if cli.sim {
                    bail!("stats: {e:#} (stats needs a running bridge; try --ws)");
                }
                Err(e)
            }
        },
        Command::Channels => {
            if cli.sim {
                let mut sim = two_node_sim(1);
                let channel_id = sim.add_channel_all("demo-channel", &[7u8; 32]);
                sim.announce_all();
                sim.run_until(200);
                println!(
                    "{}  {:<16} psk_set=true",
                    channel_id.to_hex(),
                    "demo-channel"
                );
                return Ok(0);
            }
            let mut ws = WsClient::connect(&cli).await?;
            ws.send_command("get_channels", json!({}), "c1").await?;
            let (typ, data) = ws.wait_for(&["channels", "error"], "c1", 5_000).await?;
            if typ == "error" {
                bail!("bridge error: {data}");
            }
            let channels = data
                .get("channels")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if channels.is_empty() {
                println!("no channels");
            } else {
                for channel in channels {
                    println!(
                        "{}  {:<16} psk_set={}",
                        channel
                            .get("channel_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("?"),
                        channel.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        channel
                            .get("psk_set")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                    );
                }
            }
            Ok(0)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Airtime calculator (works offline)
// ---------------------------------------------------------------------------------------------

fn airtime(
    profile_name: &str,
    bytes: usize,
    region_name: &str,
    duty_override: Option<u8>,
) -> Result<u8> {
    let Some(profile) = util::PROFILES
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(profile_name))
    else {
        bail!(
            "unknown profile '{profile_name}' (choose from {})",
            util::PROFILES
                .iter()
                .map(|p| p.name)
                .collect::<Vec<_>>()
                .join(", ")
        );
    };
    let Some(region) = util::REGIONS
        .iter()
        .find(|r| r.name.eq_ignore_ascii_case(region_name))
    else {
        bail!(
            "unknown region '{region_name}' (choose from {})",
            util::REGIONS
                .iter()
                .map(|r| r.name)
                .collect::<Vec<_>>()
                .join(", ")
        );
    };
    if profile.bw_khz > region.bw_khz {
        let allowed: Vec<&str> = util::PROFILES
            .iter()
            .filter(|candidate| candidate.bw_khz <= region.bw_khz)
            .map(|candidate| candidate.name)
            .collect();
        bail!(
            "profile {} needs {} kHz but {} allows at most {} kHz; allowed profiles: {}",
            profile.name,
            profile.bw_khz,
            region.name,
            region.bw_khz,
            allowed.join(", ")
        );
    }
    if let Some(duty) = duty_override {
        if duty > 100 {
            bail!("--duty-override must be 0..=100");
        }
        if duty > region.duty_cycle_pct.unwrap_or(100) {
            println!("warning       {OVERRIDE_WARNING}");
        }
    }
    let chunk_bytes = 180usize;
    let chunks = bytes.div_ceil(chunk_bytes).max(1);
    let frames = chunks + 4; // manifest + ~3 ACKs for a direct attachment
    let per_frame_ms = util::lora_airtime_ms(profile, 240);
    let total_ms = frames as u64 * per_frame_ms as u64;
    let legal_duty = region.duty_cycle_pct;
    let duty_pct = duty_override.or(legal_duty).unwrap_or(100);
    let legal_text = match legal_duty {
        Some(duty) => format!("{duty}%"),
        None => "fair use (no numeric duty cycle)".to_string(),
    };
    // Wall-clock time needed to stay inside the effective duty cycle.
    let legal_wait_s = if duty_pct == 0 {
        0.0
    } else {
        total_ms as f64 / 1000.0 * (100.0 / duty_pct as f64)
    };
    println!(
        "profile       {} (SF{} / {} kHz / CR4/{})",
        profile.name, profile.sf, profile.bw_khz, profile.coding_rate
    );
    println!(
        "region        {} ({} MHz, max {} dBm EIRP, legal duty {})",
        region.name,
        region.freq_khz as f64 / 1000.0,
        region.max_power_dbm,
        legal_text
    );
    println!(
        "max bw        {} kHz (profile uses {} kHz)",
        region.bw_khz, profile.bw_khz
    );
    if let Some(override_pct) = duty_override {
        println!(
            "effective     duty override {}% (legal {}) — REGULATORY OVERRIDE, verify local law",
            override_pct, legal_text
        );
        let _ = legal_duty;
    } else {
        println!("effective     legal duty {legal_text}");
    }
    println!(
        "payload       {} ({} chunk(s) of {} B)",
        util::format_bytes(bytes),
        chunks,
        chunk_bytes
    );
    println!(
        "frames        {} ({} chunks + manifest + ACKs)",
        frames, chunks
    );
    println!("per frame     {} ms at full 240-byte frame", per_frame_ms);
    println!(
        "total airtime {} ({} ms)",
        util::format_duration_ms(total_ms),
        total_ms
    );
    if legal_duty.is_some() || duty_override.is_some() {
        println!(
            "wall clock    ~{} at the effective {}% duty cycle",
            util::format_duration_ms((legal_wait_s * 1000.0) as u64),
            duty_pct
        );
    }
    Ok(0)
}

// ---------------------------------------------------------------------------------------------
// Standalone simulator mode
// ---------------------------------------------------------------------------------------------

fn two_node_sim(seed: u64) -> MeshSim {
    let mut sim = MeshSim::new(
        2,
        SimConfig {
            seed,
            latency_ms: 20,
            loss_pct: 0,
            ..Default::default()
        },
    );
    sim.set_full_topology();
    sim
}

fn sim_send_text(cli: &Cli, text: &str) -> Result<u8> {
    let mut sim = two_node_sim(1);
    sim.announce_all();
    sim.run_until(200);
    let msg_id = sim.send_text(0, 1, text).context("simulated send")?;
    let delivered = sim.run_until_cond(10_000, |sim| {
        sim.deliveries().iter().any(|d| d.msg_id == msg_id)
    });
    if delivered {
        sim.run_until_cond(sim.now_ms + 5_000, |sim| {
            sim.acks()
                .iter()
                .any(|a| a.msg_id == msg_id && a.kind == "delivered")
        });
    }
    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "msg_id": msg_id,
                "delivered": delivered,
                "delivery": sim.delivered_text(msg_id),
                "acks": sim.acks(),
                "airtime_total_ms": sim.total_airtime_ms(),
            }))?
        );
    } else {
        println!("sent msg {msg_id} to node-1 over the simulated mesh");
        match sim.delivered_text(msg_id) {
            Some(delivery) => println!(
                "delivered in {}ms ({} hops, {} frame airtime)",
                delivery.delivered_at_ms - delivery.sent_at_ms,
                delivery
                    .hops
                    .map(|h| h.to_string())
                    .unwrap_or_else(|| "?".into()),
                util::format_duration_ms(sim.total_airtime_ms())
            ),
            None => println!("not delivered"),
        }
        let acked = sim
            .acks()
            .iter()
            .any(|a| a.msg_id == msg_id && a.kind == "delivered");
        println!("delivery ack: {}", if acked { "yes" } else { "no" });
    }
    Ok(if delivered { 0 } else { 1 })
}

fn sim_send_attachment(cli: &Cli, text: Option<&str>, mime: &str, data: &[u8]) -> Result<u8> {
    let mut sim = two_node_sim(2);
    sim.announce_all();
    sim.run_until(200);
    if let Some(text) = text {
        sim.send_text(0, 1, text)?;
    }
    let (_msg_id, res_id) = sim
        .send_resource(0, 1, mime, data)
        .context("simulated resource send")?;
    let delivered = sim.run_until_cond(120_000, |sim| {
        sim.resource_deliveries().iter().any(|r| r.res_id == res_id)
    });
    let delivery = sim
        .resource_deliveries()
        .iter()
        .find(|r| r.res_id == res_id);
    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "res_id": res_id,
                "delivered": delivered,
                "bytes": delivery.map(|d| d.bytes),
                "sha_ok": delivery.map(|d| d.sha_ok),
                "airtime_total_ms": sim.total_airtime_ms(),
            }))?
        );
    } else {
        println!(
            "sent {} attachment over the simulated mesh",
            util::format_bytes(data.len())
        );
        if let Some(delivery) = delivery {
            println!(
                "delivered {} bytes in {}ms (sha ok: {}), {} airtime",
                delivery.bytes,
                delivery.at_ms,
                delivery.sha_ok,
                util::format_duration_ms(sim.total_airtime_ms())
            );
        } else {
            println!("transfer did not complete");
        }
    }
    Ok(if delivered { 0 } else { 1 })
}

async fn sim_listen(cli: &Cli, count: u32) -> Result<u8> {
    let mut sim = two_node_sim(3);
    sim.announce_all();
    sim.run_until(500);
    println!(
        "listening on the simulated mesh ({} messages, Ctrl-C to stop)",
        count
    );
    for i in 0..count {
        let msg_id = sim
            .send_text(1, 0, &format!("simulated peer message #{i}"))
            .context("send")?;
        sim.run_until_cond(10_000, |sim| {
            sim.deliveries().iter().any(|d| d.msg_id == msg_id)
        });
        if let Some(delivery) = sim.delivered_text(msg_id) {
            let stamp = clock(delivery.sent_at_ms);
            println!(
                "{stamp} node-1 -> you   {:?}   (msg {msg_id})",
                delivery.text.as_deref().unwrap_or("")
            );
        }
        if cli.json {
            println!("{}", serde_json::to_string(&json!({"msg_id": msg_id}))?);
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    let acked = sim.acks().len();
    println!(
        "done: {} message(s), {} ack(s), {} airtime",
        count,
        acked,
        util::format_duration_ms(sim.total_airtime_ms())
    );
    Ok(0)
}

fn print_node_table(sim: &MeshSim, index: usize) {
    let mut nodes = sim.known_nodes(index);
    nodes.sort_by(|a, b| a.name.cmp(&b.name));
    println!(
        "{:<18} {:<14} {:<10} {:>4}  last seen",
        "node id", "name", "class", "hops"
    );
    for node in &nodes {
        println!(
            "{:<18} {:<14} {:<10} {:>4}  {}ms ago",
            node.node_id.to_hex(),
            node.name,
            node.class.as_str(),
            node.hops,
            sim.now_ms.saturating_sub(node.last_seen_ms)
        );
    }
    if nodes.is_empty() {
        println!("(no nodes yet)");
    }
}

// ---------------------------------------------------------------------------------------------
// WebSocket client
// ---------------------------------------------------------------------------------------------

struct WsClient {
    stream: tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl WsClient {
    async fn connect(cli: &Cli) -> Result<Self> {
        let base = cli
            .ws
            .clone()
            .unwrap_or_else(|| "ws://127.0.0.1:8787/api/ws".to_string());
        let token = resolve_token(cli)?;
        let url = if base.contains("token=") {
            base
        } else {
            format!(
                "{base}{}token={token}",
                if base.contains('?') { '&' } else { '?' }
            )
        };
        let (stream, _response) = connect_async(&url).await.with_context(|| {
            format!("connecting to bridge at {url} (is openmesh-bridge running?)")
        })?;
        Ok(Self { stream, next_id: 1 })
    }

    async fn send_command(&mut self, typ: &str, data: Value, id: &str) -> Result<()> {
        let envelope = json!({ "v": 1, "type": typ, "id": id, "data": data }).to_string();
        self.stream
            .send(Message::Text(envelope))
            .await
            .context("sending command to bridge")
    }

    /// Read the next JSON frame, or `None` on timeout/close/error.
    async fn next_value(&mut self, timeout: Duration) -> Option<Value> {
        let frame = tokio::time::timeout(timeout, self.stream.next())
            .await
            .ok()??
            .ok()?;
        let text = frame.to_text().ok()?;
        serde_json::from_str(text).ok()
    }

    /// Read frames until a matching `type` arrives (or the timeout expires).
    async fn wait_for(
        &mut self,
        types: &[&str],
        id: &str,
        timeout_ms: u64,
    ) -> Result<(String, Value)> {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                bail!("timed out waiting for {}", types.join("/"));
            }
            let frame = tokio::time::timeout(remaining, self.stream.next())
                .await
                .map_err(|_| anyhow::anyhow!("timed out waiting for {}", types.join("/")))?;
            let Some(Ok(message)) = frame else {
                bail!("bridge closed the connection");
            };
            let Ok(text) = message.to_text() else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(text) else {
                continue;
            };
            let typ = value
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let matches_type = types.contains(&typ.as_str());
            let matches_id = value.get("id").and_then(|v| v.as_str()) == Some(id);
            if matches_type && (matches_id || types.contains(&"hello") || value.get("id").is_none())
            {
                if typ == "error" {
                    let code = value
                        .pointer("/data/code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("error");
                    let message = value
                        .pointer("/data/message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    bail!("bridge error {code}: {message}");
                }
                return Ok((typ, value.get("data").cloned().unwrap_or(Value::Null)));
            }
        }
    }
}

fn resolve_token(cli: &Cli) -> Result<String> {
    if let Some(token) = &cli.token {
        return Ok(token.clone());
    }
    if let Ok(token) = std::env::var("OPENMESH_TOKEN") {
        if !token.trim().is_empty() {
            return Ok(token.trim().to_string());
        }
    }
    let path = cli.token_file.clone().unwrap_or_else(default_token_path);
    let token = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "reading bridge token {} (use --token or --token-file)",
            path.display()
        )
    })?;
    Ok(token.trim().to_string())
}

fn default_token_path() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        std::path::PathBuf::from(dir)
            .join("openmesh")
            .join("bridge-token")
    } else if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        std::path::PathBuf::from(home)
            .join(".config")
            .join("openmesh")
            .join("bridge-token")
    } else {
        std::path::PathBuf::from(".config")
            .join("openmesh")
            .join("bridge-token")
    }
}

/// Parse `ws://host:port/api/ws` into `host:port` for the REST helper.
fn ws_authority(ws: &str) -> Result<String> {
    let rest = ws
        .strip_prefix("ws://")
        .or_else(|| ws.strip_prefix("http://"))
        .unwrap_or(ws);
    let authority = rest
        .split('/')
        .next()
        .unwrap_or(rest)
        .split('?')
        .next()
        .unwrap_or(rest);
    if authority.is_empty() {
        bail!("invalid bridge URL '{ws}'");
    }
    Ok(if authority.contains(':') {
        authority.to_string()
    } else {
        format!("{authority}:80")
    })
}

fn ws_base(cli: &Cli) -> Result<String> {
    let ws = cli
        .ws
        .clone()
        .unwrap_or_else(|| "ws://127.0.0.1:8787/api/ws".to_string());
    Ok(ws)
}

async fn http_post_attachment(cli: &Cli, mime: &str, filename: &str, data: &[u8]) -> Result<Value> {
    let authority = ws_authority(&ws_base(cli)?)?;
    let token = resolve_token(cli)?;
    let mut stream = TcpStream::connect(&authority)
        .await
        .with_context(|| format!("connecting to {authority}"))?;
    let mut request = format!(
        "POST /api/attachments HTTP/1.1\r\nHost: {authority}\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: {mime}\r\nX-OpenMesh-Filename: {filename}\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n",
        data.len()
    )
    .into_bytes();
    request.extend_from_slice(data);
    stream.write_all(&request).await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let text = String::from_utf8_lossy(&response);
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or("");
    let value: Value =
        serde_json::from_str(body).with_context(|| format!("bad attachment response: {body}"))?;
    if let Some(error) = value.get("error") {
        bail!("attachment upload failed: {error}");
    }
    Ok(value)
}

async fn ws_send(
    cli: &Cli,
    to: &str,
    text: Option<&str>,
    attachment: Option<(String, Vec<u8>)>,
    no_wait: bool,
    timeout_s: u64,
) -> Result<u8> {
    let mut ws = WsClient::connect(cli).await?;
    let mut attachment_id = None;
    if let Some((mime, data)) = attachment {
        let filename = "attachment.bin";
        let meta = http_post_attachment(cli, &mime, filename, &data).await?;
        let id = meta
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("attachment upload returned no id"))?
            .to_string();
        if !cli.json {
            println!(
                "uploaded {} attachment {} ({})",
                util::format_bytes(data.len()),
                id,
                meta.get("sha256").and_then(|v| v.as_str()).unwrap_or("")
            );
        }
        attachment_id = Some(id);
    }
    let id = format!("cli-{}", ws.next_id);
    ws.next_id += 1;
    ws.send_command(
        "send_message",
        json!({ "to": to, "text": text, "attachment_id": attachment_id }),
        &id,
    )
    .await?;
    let (_typ, result) = ws.wait_for(&["send_result"], &id, 10_000).await?;
    let msg_id = result.get("msg_id").and_then(|v| v.as_u64()).unwrap_or(0);
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "queued msg {msg_id} to {to} (est. airtime {}ms)",
            result
                .get("est_airtime_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
        );
    }
    if no_wait {
        return Ok(0);
    }
    if !cli.json {
        println!("waiting for delivery confirmation (timeout {timeout_s}s)...");
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_s);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            if !cli.json {
                println!("no delivery confirmation yet (message is queued/retrying)");
            }
            return Ok(0);
        }
        let Some(value) = ws.next_value(remaining).await else {
            if !cli.json {
                println!("bridge connection closed while waiting for delivery confirmation");
            }
            return Ok(0);
        };
        let typ = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let data = value.get("data").cloned().unwrap_or(Value::Null);
        let same_msg = data.get("msg_id").and_then(|v| v.as_u64()) == Some(msg_id);
        match typ {
            "ack" if same_msg => {
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&data)?);
                } else {
                    println!(
                        "ack: {} for msg {msg_id}",
                        data.get("kind").and_then(|v| v.as_str()).unwrap_or("")
                    );
                }
                return Ok(0);
            }
            "message_update"
                if same_msg && data.get("delivered").and_then(|v| v.as_bool()) == Some(true) =>
            {
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&data)?);
                } else {
                    println!("delivered msg {msg_id} (resource/resource-ack confirmation)");
                }
                return Ok(0);
            }
            _ => {}
        }
    }
}

async fn ws_stats(cli: &Cli) -> Result<()> {
    let mut ws = WsClient::connect(cli).await?;
    ws.send_command("get_stats", json!({}), "s1").await?;
    let (_typ, data) = ws.wait_for(&["stats"], "s1", 5_000).await?;
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&data)?);
    } else {
        println!(
            "frames tx {:<6} rx {:<6} forwarded {:<6} dropped {:<4} known nodes {}",
            data.get("tx_frames").and_then(|v| v.as_u64()).unwrap_or(0),
            data.get("rx_frames").and_then(|v| v.as_u64()).unwrap_or(0),
            data.get("forwarded").and_then(|v| v.as_u64()).unwrap_or(0),
            data.get("dropped").and_then(|v| v.as_u64()).unwrap_or(0),
            data.get("known_nodes")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        );
        println!(
            "duty cycle {:.2}%   stored envelopes {}   channels {}",
            data.get("duty_cycle_pct")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
            data.get("stored_envelopes")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            data.get("channels").and_then(|v| v.as_u64()).unwrap_or(0),
        );
        if let Some(links) = data.get("links").and_then(|v| v.as_array()) {
            for link in links {
                println!(
                    "  link {} {}: tx {} rx {} rssi {:?} snr {:?}",
                    link.get("link").and_then(|v| v.as_u64()).unwrap_or(0),
                    link.get("kind").and_then(|v| v.as_str()).unwrap_or(""),
                    link.get("tx_frames").and_then(|v| v.as_u64()).unwrap_or(0),
                    link.get("rx_frames").and_then(|v| v.as_u64()).unwrap_or(0),
                    link.get("rssi"),
                    link.get("snr"),
                );
            }
        }
    }
    Ok(())
}

async fn ws_command(
    cli: &Cli,
    typ: &str,
    data: Value,
    done: impl Fn(&str, &Value) -> bool,
) -> Result<u8> {
    let mut ws = WsClient::connect(cli).await?;
    let id = "cli-announce";
    ws.send_command(typ, data, id).await?;
    println!("sent {typ} to the bridge");
    // Wait for any confirmation-ish frame or a short idle period; announces have no response.
    let deadline = tokio::time::Instant::now() + Duration::from_millis(500);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, ws.stream.next()).await {
            Ok(Some(Ok(message))) => {
                if let Ok(text) = message.to_text() {
                    if let Ok(value) = serde_json::from_str::<Value>(text) {
                        let event_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        if done(event_type, &value) {
                            break;
                        }
                    }
                }
            }
            _ => break,
        }
    }
    Ok(0)
}

async fn ws_listen(cli: &Cli, types: Option<&str>) -> Result<u8> {
    let filter: Vec<String> = match types {
        Some(types) => types
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect(),
        None => vec![
            "message".into(),
            "group_message".into(),
            "ack".into(),
            "node".into(),
            "position".into(),
            "telemetry".into(),
            "store_offer".into(),
            "resource_progress".into(),
        ],
    };
    let mut ws = WsClient::connect(cli).await?;
    println!("connected; streaming events (Ctrl-C to stop)");
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(3600), ws.stream.next()).await;
        let Ok(Some(Ok(message))) = frame else {
            break;
        };
        let Ok(text) = message.to_text() else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            continue;
        };
        let typ = value
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let data = value.get("data").cloned().unwrap_or(Value::Null);
        if cli.sim {
            let _ = &data;
        }
        if !filter.iter().any(|f| f == &typ) {
            continue;
        }
        if cli.json {
            println!("{}", serde_json::to_string(&value)?);
            continue;
        }
        match typ.as_str() {
            "message" => {
                let text = data.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let outgoing = data
                    .get("outgoing")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let peer = short(data.get("peer").and_then(|v| v.as_str()).unwrap_or(""));
                let stamp = clock(data.get("sent_at_ms").and_then(|v| v.as_u64()).unwrap_or(0));
                if outgoing {
                    println!("{stamp} you -> {peer}: {text}");
                } else {
                    println!("{stamp} {peer} -> you: {text}");
                }
            }
            "group_message" => {
                let text = data.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let channel = data
                    .get("channel_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("channel");
                let from = data
                    .get("from_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let stamp = clock(data.get("sent_at_ms").and_then(|v| v.as_u64()).unwrap_or(0));
                println!("{stamp} #{channel} {from}: {text}");
            }
            "ack" => println!(
                "ack {} for msg {} from {}",
                data.get("kind").and_then(|v| v.as_str()).unwrap_or(""),
                data.get("msg_id").and_then(|v| v.as_u64()).unwrap_or(0),
                short(data.get("peer").and_then(|v| v.as_str()).unwrap_or(""))
            ),
            "node" => println!(
                "+ node {} '{}' ({}) hops={}",
                short(data.get("node_id").and_then(|v| v.as_str()).unwrap_or("")),
                data.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                data.get("class").and_then(|v| v.as_str()).unwrap_or(""),
                data.get("hops").and_then(|v| v.as_u64()).unwrap_or(0)
            ),
            "position" => println!(
                "position from {}: lat {:.5} lon {:.5}",
                short(data.get("node_id").and_then(|v| v.as_str()).unwrap_or("")),
                data.pointer("/position/lat")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                data.pointer("/position/lon")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
            ),
            "store_offer" => println!(
                "store offer: relay {} holds {} envelope(s) for you",
                short(data.get("relay").and_then(|v| v.as_str()).unwrap_or("")),
                data.get("count").and_then(|v| v.as_u64()).unwrap_or(0)
            ),
            "resource_progress" => println!(
                "resource {} {}/{} chunks ({})",
                data.get("res_id").and_then(|v| v.as_u64()).unwrap_or(0),
                data.get("received").and_then(|v| v.as_u64()).unwrap_or(0),
                data.get("total").and_then(|v| v.as_u64()).unwrap_or(0),
                data.get("direction").and_then(|v| v.as_str()).unwrap_or("")
            ),
            other => println!("{other}: {data}"),
        }
    }
    Ok(0)
}

fn print_nodes(data: &mut Value) {
    let nodes = data
        .get_mut("nodes")
        .and_then(|v| v.as_array_mut())
        .cloned()
        .unwrap_or_default();
    if nodes.is_empty() {
        println!("(no nodes yet — wait for announces)");
        return;
    }
    println!(
        "{:<18} {:<16} {:<10} {:>4}  last seen",
        "node id", "name", "class", "hops"
    );
    for node in &nodes {
        println!(
            "{:<18} {:<16} {:<10} {:>4}  {}ms ago",
            node.get("node_id").and_then(|v| v.as_str()).unwrap_or(""),
            node.get("name").and_then(|v| v.as_str()).unwrap_or(""),
            node.get("class").and_then(|v| v.as_str()).unwrap_or(""),
            node.get("hops").and_then(|v| v.as_u64()).unwrap_or(0),
            now_ms().saturating_sub(
                node.get("last_seen_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
            )
        );
    }
}

fn now_ms() -> u64 {
    util::now_ms()
}

fn short(id: &str) -> String {
    if id.len() > 12 {
        format!("{}…", &id[..12])
    } else {
        id.to_string()
    }
}

/// Format a Unix-millisecond timestamp as `HH:MM:SS` in UTC without pulling in a date library.
fn clock(ms: u64) -> String {
    let secs = ms / 1000 % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

fn mime_for_path(path: &std::path::Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "opus" | "ogg" => "audio/ogg",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "txt" => "text/plain",
        "json" => "application/json",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
    .to_string()
}

// ---------------------------------------------------------------------------------------------
// Agent helpers + help center (AI-NATIVE.md §3.2, ux-help-system.md §5)
// ---------------------------------------------------------------------------------------------

/// Locate the `openmesh-bridge` binary for MCP client configs.
fn bridge_binary_path() -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            for name in ["openmesh-bridge", "openmesh-bridge.exe"] {
                let candidate = parent.join(name);
                if candidate.is_file() {
                    return candidate.display().to_string();
                }
            }
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in ["openmesh-bridge", "openmesh-bridge.exe"] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return candidate.display().to_string();
                }
            }
        }
    }
    "openmesh-bridge".to_string()
}

fn mcp_client_config(api: &str, token_file: &str) -> Value {
    json!({
        "mcpServers": {
            "openmesh": {
                "command": bridge_binary_path(),
                "args": ["mcp"],
                "env": {
                    "OPENMESH_API": api,
                    "OPENMESH_TOKEN_FILE": token_file,
                    "OPENMESH_MCP_READ_ONLY": "0",
                },
            }
        }
    })
}

fn tier_is_advanced(tier: &str) -> Result<bool> {
    match tier.trim().to_ascii_lowercase().as_str() {
        "beginner" => Ok(false),
        "advanced" => Ok(true),
        other => bail!("unknown tier '{other}': use beginner or advanced"),
    }
}

fn help_topic_list(help: &Value) -> Vec<String> {
    help.get("topics")
        .and_then(|v| v.as_array())
        .map(|topics| {
            topics
                .iter()
                .filter_map(|topic| {
                    topic
                        .get("slug")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn build_agent_prompt(
    tier: &str,
    api: &str,
    config: &Value,
    status: &Value,
    setup: &Value,
    help: &Value,
    mcp_config: &Value,
) -> Value {
    let advanced = tier_is_advanced(tier).unwrap_or(false);
    let node_name = setup
        .get("node_name")
        .and_then(|v| v.as_str())
        .filter(|name| !name.is_empty())
        .or_else(|| config.pointer("/node/name").and_then(|v| v.as_str()))
        .unwrap_or("openmesh");
    let node_id = setup
        .get("node_id")
        .and_then(|v| v.as_str())
        .or_else(|| status.pointer("/node_id").and_then(|v| v.as_str()))
        .unwrap_or("unknown");
    let region = config
        .pointer("/radio/region")
        .and_then(|v| v.as_str())
        .unwrap_or("EU868");
    let profile = config
        .pointer("/radio/profile")
        .and_then(|v| v.as_str())
        .unwrap_or("MEDIUM_FAST");
    let help_topics = help_topic_list(help);
    let safety = vec![
        "Always ask the human before destructive changes: rotating the backbone token, kicking peers, changing radio region/profile, disabling auth.".to_string(),
        "Never paste API tokens, backbone tokens, PSKs or identity seeds into chat logs. Rotate outputs are shown once; write them to a file and tell the human where.".to_string(),
        "Prefer read-only tools for diagnosis; write tools are explicit and every mutation is attributed in the activity log (GET /api/activity).".to_string(),
        "If you need a capability that is not in the tool list, tell the human instead of guessing.".to_string(),
    ];
    let mut text = String::new();
    text.push_str(&format!(
        "You are an OpenMesh operations assistant connected to the bridge at {api} (node \"{node_name}\", id {node_id}).\n"
    ));
    if advanced {
        text.push_str(&format!(
            "This is the ADVANCED tier: answer with exact values and protocol terms. Radio: region {region}, profile {profile}. \n"
        ));
    } else {
        text.push_str(
            "This is the BEGINNER tier: explain any jargon in plain language, keep answers short, and never blame the user. Offer a next step and a help topic for every question.\n",
        );
    }
    text.push_str("\nLive state (from the bridge API):\n");
    text.push_str(&format!(
        "- Uptime: {}s; radio connected: {}; airtime: {:.1}%.\n",
        status.get("uptime_s").and_then(|v| v.as_u64()).unwrap_or(0),
        status
            .pointer("/radio/connected")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        status
            .pointer("/radio/airtime_used_pct")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
    ));
    text.push_str(&format!(
        "- Setup complete: {}; channels: {}; known nodes: {}.\n",
        setup
            .get("setup_completed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        setup
            .pointer("/counts/channels")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        setup
            .pointer("/counts/nodes")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    ));
    if setup.get("first_run").and_then(|v| v.as_bool()) == Some(true) {
        text.push_str("- The node is not set up yet: ask for role/country/name, then call `openmesh_setup_quick`.\n");
    }
    text.push_str("\nMCP client config (paste into your MCP client):\n");
    text.push_str(&serde_json::to_string_pretty(mcp_config).unwrap_or_default());
    text.push_str("\n\nKey tools:\n");
    text.push_str("- Read: openmesh_health, openmesh_status, openmesh_nodes, openmesh_messages, openmesh_channels, openmesh_contacts, openmesh_backbone_status, openmesh_setup_state, openmesh_activity, openmesh_help.\n");
    text.push_str("- Write: openmesh_send_message, openmesh_send_channel_message, openmesh_announce, openmesh_set_config, openmesh_setup_quick, openmesh_create_channel, openmesh_join_channel, openmesh_add_contact, openmesh_backbone_config.\n");
    text.push_str("- Destructive (require confirm:true): openmesh_rotate_backbone_token, openmesh_kick_backbone_peer.\n");
    text.push_str("\nSafety rules:\n");
    for (index, rule) in safety.iter().enumerate() {
        text.push_str(&format!("{}. {rule}\n", index + 1));
    }
    text.push_str(&format!(
        "\nHelp topics (call openmesh_help with one slug): {}.\n",
        help_topics.join(", ")
    ));
    if advanced {
        text.push_str("\nAdvanced notes: OBB/1 control channel for backbone, HMAC-SHA256 token auth, per-peer frame/byte counters, 30 mutating MCP calls/minute, 10s API timeout. Read docs/architecture/AI-NATIVE.md for the full tool contract.\n");
    } else {
        text.push_str("\nIf something looks wrong, say what happened and what to try next, then offer the `troubleshooting` help topic.\n");
    }
    json!({
        "tier": if advanced { "advanced" } else { "beginner" },
        "api": api,
        "node": { "name": node_name, "node_id": node_id, "region": region, "profile": profile },
        "prompt": text,
        "mcp_config": mcp_config,
        "tools": {
            "read": ["openmesh_health","openmesh_status","openmesh_nodes","openmesh_node","openmesh_messages","openmesh_channels","openmesh_contacts","openmesh_backbone_status","openmesh_backbone_peers","openmesh_backbone_setup","openmesh_setup_state","openmesh_activity","openmesh_help"],
            "write": ["openmesh_send_message","openmesh_send_channel_message","openmesh_share_position","openmesh_announce","openmesh_mark_read","openmesh_retry_message","openmesh_set_config","openmesh_setup_quick","openmesh_create_channel","openmesh_join_channel","openmesh_add_contact","openmesh_backbone_config"],
            "destructive": ["openmesh_rotate_backbone_token","openmesh_kick_backbone_peer"],
        },
        "safety_rules": safety,
        "help_topics": help_topics,
    })
}

async fn agent_command(cli: &Cli, command: &AgentCommand) -> Result<u8> {
    let base = api_base(cli);
    let token = resolve_token(cli)?;
    match command {
        AgentCommand::Prompt { tier } => {
            tier_is_advanced(tier)?;
            let config = http_json("GET", &base, "/api/config", &token, None).await?;
            let status = http_json("GET", &base, "/api/status", &token, None).await?;
            let setup = http_json("GET", &base, "/api/setup/state", &token, None).await?;
            let help = http_json("GET", &base, "/api/help", &token, None).await?;
            let token_file = cli
                .token_file
                .clone()
                .unwrap_or_else(default_token_path)
                .display()
                .to_string();
            let mcp_config = mcp_client_config(&base, &token_file);
            let value =
                build_agent_prompt(tier, &base, &config, &status, &setup, &help, &mcp_config);
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                println!("{}", value["prompt"].as_str().unwrap_or(""));
            }
        }
        AgentCommand::Config { client } => {
            let client = client.trim().to_ascii_lowercase();
            if !matches!(client.as_str(), "claude" | "cursor" | "dsh" | "generic") {
                bail!("unknown client '{client}': use claude, cursor, dsh or generic");
            }
            let token_file = cli
                .token_file
                .clone()
                .unwrap_or_else(default_token_path)
                .display()
                .to_string();
            let config = mcp_client_config(&base, &token_file);
            // stdout stays valid JSON so it can be redirected straight into a client config.
            println!("{}", serde_json::to_string_pretty(&config)?);
            eprintln!(
                "openmesh-cli: paste this into the {client} MCP configuration (mcpServers). \
                 It runs `{} mcp` with OPENMESH_API={base} and reads the token from {token_file}.",
                bridge_binary_path()
            );
        }
        AgentCommand::Activity { limit } => {
            let limit = (*limit).clamp(1, 256);
            let value = http_json(
                "GET",
                &base,
                &format!("/api/activity?limit={limit}"),
                &token,
                None,
            )
            .await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                let entries = value
                    .get("entries")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                if entries.is_empty() {
                    println!("(no recorded actions yet)");
                } else {
                    for entry in entries {
                        println!(
                            "[{}] {:<24} {:<7} {:<28} {}",
                            entry.get("at_ms").and_then(|v| v.as_u64()).unwrap_or(0),
                            entry.get("source").and_then(|v| v.as_str()).unwrap_or("?"),
                            entry.get("method").and_then(|v| v.as_str()).unwrap_or(""),
                            entry.get("path").and_then(|v| v.as_str()).unwrap_or(""),
                            if entry.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                                entry
                                    .get("summary")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string()
                            } else {
                                format!(
                                    "FAILED: {}",
                                    entry
                                        .get("error")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("error")
                                )
                            },
                        );
                    }
                }
            }
        }
    }
    Ok(0)
}

async fn help_command(cli: &Cli, topic: Option<&str>) -> Result<u8> {
    let base = api_base(cli);
    let token = resolve_token(cli)?;
    match topic {
        Some(topic) => {
            let value = http_json("GET", &base, &format!("/api/help/{topic}"), &token, None)
                .await
                .with_context(|| format!("unknown or unavailable help topic '{topic}'"))?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                let title = value.get("title").and_then(|v| v.as_str()).unwrap_or(topic);
                let markdown = value.get("markdown").and_then(|v| v.as_str()).unwrap_or("");
                if !markdown.trim_start().starts_with("# ") {
                    println!("# {title}\n");
                }
                println!("{markdown}");
            }
        }
        None => {
            let value = http_json("GET", &base, "/api/help", &token, None).await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                if let Some(topics) = value.get("topics").and_then(|v| v.as_array()) {
                    println!("OpenMesh help topics (openmesh-cli help <slug>):");
                    for topic in topics {
                        println!(
                            "  {:<24} {:<10} {}",
                            topic.get("slug").and_then(|v| v.as_str()).unwrap_or("?"),
                            topic
                                .get("audience")
                                .and_then(|v| v.as_str())
                                .unwrap_or("both"),
                            topic.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                        );
                    }
                }
            }
        }
    }
    Ok(0)
}

// ---------------------------------------------------------------------------------------------
// Config + regulatory override (regulatory-and-regions.md §2.4)
// ---------------------------------------------------------------------------------------------

/// Verbatim warning shown whenever an override may exceed the legal maxima.
const OVERRIDE_WARNING: &str = "Regulatory override active — transmissions may violate local radio regulations and void your radio's certification. Use only in a life-threatening emergency where local law permits. You are responsible for compliance and for any interference or damage.";

fn region_legal(region_name: &str) -> Option<&'static util::Region> {
    util::REGIONS
        .iter()
        .find(|region| region.name.eq_ignore_ascii_case(region_name))
}

/// Whether the requested values exceed the legal maxima for the region.
///
/// A frequency different from the region centre is treated as exceeding; reducing power or duty
/// is not. Unknown regions are treated conservatively (exceeds) so the CLI never silently bypasses
/// the acknowledgement.
fn override_values_exceed_legal(
    region_name: &str,
    duty_pct: Option<u8>,
    power_dbm: Option<i8>,
    frequency_khz: Option<u32>,
) -> bool {
    let Some(region) = region_legal(region_name) else {
        return true;
    };
    if let Some(duty) = duty_pct {
        if region.duty_cycle_pct.is_some_and(|legal| duty > legal) {
            return true;
        }
    }
    if let Some(power) = power_dbm {
        if power > region.max_power_dbm {
            return true;
        }
    }
    if let Some(frequency) = frequency_khz {
        if frequency != region.freq_khz {
            return true;
        }
    }
    false
}

/// The `--i-understand-risk` flag is mandatory when the requested values exceed the legal maxima
/// (regulatory-and-regions.md §2.4). Safe reductions/zero-TX stay usable without it.
fn override_requires_risk(desired_enabled: bool, exceeds: bool) -> bool {
    desired_enabled && exceeds
}

fn print_override_summary(config: &Value) {
    let current = &config["regulatory_override"];
    let active = config["override_active"].as_bool().unwrap_or(false);
    println!(
        "override      enabled={} acknowledged={} duty={} power={} frequency={}{}{}",
        current["enabled"].as_bool().unwrap_or(false),
        current["acknowledged"].as_bool().unwrap_or(false),
        current["duty_cycle_pct"]
            .as_u64()
            .map(|value| format!("{value}%"))
            .unwrap_or_else(|| "legal".to_string()),
        current["max_power_dbm"]
            .as_i64()
            .map(|value| format!("{value} dBm"))
            .unwrap_or_else(|| "legal".to_string()),
        current["frequency_khz"]
            .as_u64()
            .map(|value| format!("{value}kHz"))
            .unwrap_or_else(|| "legal".to_string()),
        current["reason"]
            .as_str()
            .map(|reason| format!(" reason={reason:?}"))
            .unwrap_or_default(),
        if active { " ACTIVE" } else { "" },
    );
    if active {
        println!("WARNING       {OVERRIDE_WARNING}");
    }
    if let Some(warning) = config["override_warning"]
        .as_str()
        .filter(|warning| *warning != OVERRIDE_WARNING)
    {
        println!("warning       {warning}");
    }
}

fn format_config_show(config: &Value) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "node          {} ({})  announce {}s\n",
        config
            .pointer("/node/name")
            .and_then(|v| v.as_str())
            .unwrap_or("?"),
        config
            .pointer("/node/class")
            .and_then(|v| v.as_str())
            .unwrap_or("?"),
        config
            .pointer("/node/announce_interval_s")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    ));
    out.push_str(&format!(
        "radio         profile={} region={} tx_power={} dBm frequency={} MHz interface={}\n",
        config
            .pointer("/radio/profile")
            .and_then(|v| v.as_str())
            .unwrap_or("?"),
        config
            .pointer("/radio/region")
            .and_then(|v| v.as_str())
            .unwrap_or("?"),
        config
            .pointer("/radio/tx_power_dbm")
            .and_then(|v| v.as_i64())
            .map(|value| value.to_string())
            .unwrap_or_else(|| "?".to_string()),
        config
            .pointer("/radio/frequency_mhz")
            .and_then(|v| v.as_f64())
            .map(|value| format!("{value:.3}"))
            .unwrap_or_else(|| "?".to_string()),
        config
            .pointer("/radio/interface")
            .and_then(|v| v.as_str())
            .unwrap_or("?"),
    ));
    let legal = &config["legal"];
    out.push_str(&format!(
        "legal         region={} max_power={} dBm duty={} max_bw={} kHz  ({})\n",
        legal.get("region").and_then(|v| v.as_str()).unwrap_or("?"),
        legal
            .get("max_power_dbm")
            .and_then(|v| v.as_i64())
            .map(|value| value.to_string())
            .unwrap_or_else(|| "?".to_string()),
        legal
            .get("duty_cycle_pct")
            .and_then(|v| v.as_u64())
            .map(|value| format!("{value}%"))
            .unwrap_or_else(|| "fair use".to_string()),
        legal
            .get("max_bw_khz")
            .and_then(|v| v.as_u64())
            .map(|value| value.to_string())
            .unwrap_or_else(|| "?".to_string()),
        legal
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("verify local law"),
    ));
    out.push_str(&format!(
        "features      auto_announce={} store_forward={} position_sharing={}\n",
        config
            .pointer("/features/auto_announce")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        config
            .pointer("/features/store_forward")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        config
            .pointer("/features/position_sharing")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    ));
    out.push_str(&format!(
        "channels      {}\n",
        config
            .get("channels")
            .and_then(|v| v.as_array())
            .map(Vec::len)
            .unwrap_or(0)
    ));
    let current = &config["regulatory_override"];
    out.push_str(&format!(
        "override      enabled={} acknowledged={} duty={} power={} frequency={} reason={}\n",
        current
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        current
            .get("acknowledged")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        current
            .get("duty_cycle_pct")
            .and_then(|v| v.as_u64())
            .map(|value| format!("{value}%"))
            .unwrap_or_else(|| "legal".to_string()),
        current
            .get("max_power_dbm")
            .and_then(|v| v.as_i64())
            .map(|value| format!("{value} dBm"))
            .unwrap_or_else(|| "legal".to_string()),
        current
            .get("frequency_khz")
            .and_then(|v| v.as_u64())
            .map(|value| format!("{value} kHz"))
            .unwrap_or_else(|| "legal".to_string()),
        current
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("-"),
    ));
    if config
        .get("override_active")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        out.push_str(&format!("WARNING       {OVERRIDE_WARNING}\n"));
    }
    if let Some(warning) = config
        .get("override_warning")
        .and_then(|v| v.as_str())
        .filter(|warning| *warning != OVERRIDE_WARNING)
    {
        out.push_str(&format!("warning       {warning}\n"));
    }
    out
}

async fn config_command(cli: &Cli, command: &ConfigCommand) -> Result<u8> {
    let base = api_base(cli);
    let token = resolve_token(cli)?;
    match command {
        ConfigCommand::Show => {
            let config = http_json("GET", &base, "/api/config", &token, None).await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&config)?);
            } else {
                print!("{}", format_config_show(&config));
            }
        }
        ConfigCommand::Override {
            duty_pct,
            power_dbm,
            frequency_khz,
            reason,
            i_understand_risk,
            disable,
        } => {
            if *disable {
                let body = json!({ "regulatory_override": { "enabled": false } });
                let updated = http_json("PATCH", &base, "/api/config", &token, Some(body)).await?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&updated)?);
                } else {
                    println!("Regulatory override disabled.");
                    print_override_summary(&updated);
                }
                return Ok(0);
            }
            if let Some(reason) = reason {
                if reason.chars().count() > 120 {
                    bail!("reason must be at most 120 characters");
                }
            }
            if duty_pct.is_none()
                && power_dbm.is_none()
                && frequency_khz.is_none()
                && reason.is_none()
            {
                bail!(
                    "nothing to change: pass --duty-pct/--power-dbm/--frequency-khz/--reason, or --disable"
                );
            }
            let config = http_json("GET", &base, "/api/config", &token, None).await?;
            if let Some(duty) = duty_pct {
                if *duty > 100 {
                    bail!("--duty-pct must be 0..=100");
                }
            }
            let region_name = config
                .pointer("/legal/region")
                .and_then(|v| v.as_str())
                .or_else(|| config.pointer("/radio/region").and_then(|v| v.as_str()))
                .unwrap_or("EU868")
                .to_string();
            let current_enabled = config
                .pointer("/regulatory_override/enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let exceeds =
                override_values_exceed_legal(&region_name, *duty_pct, *power_dbm, *frequency_khz);
            let _ = current_enabled;
            let requires_risk = override_requires_risk(true, exceeds);
            if requires_risk && !i_understand_risk {
                println!(
                    "Regulatory override NOT applied — this needs an explicit acknowledgement."
                );
                println!(
                    "Requested values exceed the legal maxima for {region_name} (or enable the override)."
                );
                println!("warning: {OVERRIDE_WARNING}");
                println!(
                    "Re-run with --i-understand-risk to apply it anyway; nothing was changed."
                );
                return Ok(2);
            }
            let mut override_patch = serde_json::Map::new();
            override_patch.insert("enabled".into(), json!(true));
            if *i_understand_risk {
                override_patch.insert("acknowledged".into(), json!(true));
            }
            if let Some(duty) = duty_pct {
                override_patch.insert("duty_cycle_pct".into(), json!(duty));
            }
            if let Some(power) = power_dbm {
                override_patch.insert("max_power_dbm".into(), json!(power));
            }
            if let Some(frequency) = frequency_khz {
                override_patch.insert("frequency_khz".into(), json!(frequency));
            }
            if let Some(reason) = reason {
                override_patch.insert("reason".into(), json!(reason));
            }
            let body = json!({ "regulatory_override": Value::Object(override_patch) });
            let updated = http_json("PATCH", &base, "/api/config", &token, Some(body)).await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&updated)?);
            } else {
                println!("Regulatory override updated.");
                print_override_summary(&updated);
            }
        }
    }
    Ok(0)
}

// ---------------------------------------------------------------------------------------------
// Backbone diagnostics (REST)
// ---------------------------------------------------------------------------------------------

/// Resolve the REST API base URL, deriving it from `--ws` when `--api` is not given.
fn api_base(cli: &Cli) -> String {
    if let Some(api) = &cli.api {
        return api.trim_end_matches('/').to_string();
    }
    if let Some(ws) = &cli.ws {
        let rest = ws
            .strip_prefix("ws://")
            .or_else(|| ws.strip_prefix("wss://"))
            .unwrap_or(ws);
        if let Some((authority, _)) = rest.split_once('/') {
            return format!("http://{authority}");
        }
        return format!("http://{rest}");
    }
    "http://127.0.0.1:8787".to_string()
}

async fn http_json(
    method: &str,
    base: &str,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> Result<Value> {
    let rest = base
        .strip_prefix("http://")
        .or_else(|| base.strip_prefix("https://"))
        .unwrap_or(base);
    let (authority, prefix) = rest.split_once('/').unwrap_or((rest, ""));
    let mut stream = TcpStream::connect(authority)
        .await
        .with_context(|| format!("connecting to bridge API at {base}"))?;
    let payload = body.map(|value| value.to_string()).unwrap_or_default();
    let mut request = format!(
        "{method} {prefix}{path} HTTP/1.1\r\nHost: {authority}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n"
    );
    if !payload.is_empty() {
        request.push_str("Content-Type: application/json\r\n");
        request.push_str(&format!("Content-Length: {}\r\n", payload.len()));
    }
    request.push_str("\r\n");
    request.push_str(&payload);
    stream.write_all(request.as_bytes()).await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let text = String::from_utf8_lossy(&response);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_ref(), ""));
    let status: u16 = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let value: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        let message = value
            .get("message")
            .and_then(|v| v.as_str())
            .or_else(|| value.get("error").and_then(|v| v.as_str()))
            .unwrap_or(body.trim());
        bail!("bridge API {method} {path} failed with HTTP {status}: {message}");
    }
    Ok(value)
}

fn print_backbone_info(info: &Value, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(info)?);
        return Ok(());
    }
    let mode = info
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("disabled");
    if info.get("enabled").and_then(|v| v.as_bool()) == Some(false) && mode == "disabled" {
        println!("Backbone mode is disabled on this bridge.");
        println!(
            "Enable it with: openmesh-cli backbone connect '<openmesh://backbone?...>' or from the web panel."
        );
        return Ok(());
    }
    println!(
        "backbone     mode={} enabled={} running={} bind={}:{} auth={} token_set={}",
        mode,
        info.get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        info.get("running")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        info.get("bind").and_then(|v| v.as_str()).unwrap_or(""),
        info.get("port").and_then(|v| v.as_u64()).unwrap_or(0),
        info.get("auth").and_then(|v| v.as_str()).unwrap_or(""),
        info.get("token_set")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    );
    println!(
        "name         {}   public_host={}   max_peers={}   store_forward={}",
        info.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        info.get("public_host")
            .and_then(|v| v.as_str())
            .unwrap_or("(auto)"),
        info.get("max_peers").and_then(|v| v.as_u64()).unwrap_or(0),
        info.get("store_forward")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    );
    if let Some(allowlist) = info.get("allowlist").and_then(|v| v.as_array()) {
        println!(
            "allowlist    {}",
            if allowlist.is_empty() {
                "(empty: token only)".to_string()
            } else {
                allowlist
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        );
    }
    if let Some(totals) = info.get("totals") {
        println!(
            "peers        {}   frames in/out {}/{}   bytes in/out {}/{}   auth_failures={}",
            info.get("peers").and_then(|v| v.as_u64()).unwrap_or(0),
            totals
                .get("frames_in")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            totals
                .get("frames_out")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            totals.get("bytes_in").and_then(|v| v.as_u64()).unwrap_or(0),
            totals
                .get("bytes_out")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            totals
                .get("auth_failures")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        );
    }
    Ok(())
}

async fn backbone_command(cli: &Cli, command: Option<&BackboneCommand>) -> Result<u8> {
    let base = api_base(cli);
    let token = resolve_token(cli)?;
    match command {
        None | Some(BackboneCommand::Info) => {
            let info = http_json("GET", &base, "/api/backbone", &token, None).await?;
            print_backbone_info(&info, cli.json)?;
        }
        Some(BackboneCommand::Peers) => {
            let value = http_json("GET", &base, "/api/backbone/peers", &token, None).await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else if value.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
                println!("Backbone mode is disabled on this bridge.");
            } else {
                let peers = value.as_array().cloned().unwrap_or_default();
                if peers.is_empty() {
                    println!("(no backbone peers)");
                } else {
                    println!(
                        "{:<18} {:<14} {:<9} {:<21} {:>7} {:>7} {:>6} auth",
                        "node id", "name", "direction", "remote", "frames", "bytes", "rtt"
                    );
                    for peer in peers {
                        println!(
                            "{:<18} {:<14} {:<9} {:<21} {:>7} {:>7} {:>6} {}",
                            peer.get("node_id").and_then(|v| v.as_str()).unwrap_or(""),
                            peer.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                            peer.get("direction").and_then(|v| v.as_str()).unwrap_or(""),
                            peer.get("remote_addr")
                                .and_then(|v| v.as_str())
                                .unwrap_or(""),
                            peer.get("frames_in").and_then(|v| v.as_u64()).unwrap_or(0)
                                + peer.get("frames_out").and_then(|v| v.as_u64()).unwrap_or(0),
                            peer.get("bytes_in").and_then(|v| v.as_u64()).unwrap_or(0)
                                + peer.get("bytes_out").and_then(|v| v.as_u64()).unwrap_or(0),
                            peer.get("rtt_ms")
                                .and_then(|v| v.as_u64())
                                .map(|rtt| format!("{rtt}ms"))
                                .unwrap_or_else(|| "-".to_string()),
                            peer.get("auth").and_then(|v| v.as_str()).unwrap_or(""),
                        );
                    }
                }
            }
        }
        Some(BackboneCommand::Kick { node_id }) => {
            let value = http_json(
                "DELETE",
                &base,
                &format!("/api/backbone/peers/{node_id}"),
                &token,
                None,
            )
            .await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                println!(
                    "peer {node_id}: disconnected={}",
                    value
                        .get("disconnected")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                );
            }
        }
        Some(BackboneCommand::RotateToken) => {
            let value = http_json(
                "POST",
                &base,
                "/api/backbone/token/rotate",
                &token,
                Some(json!({})),
            )
            .await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else if let Some(new_token) = value.get("token").and_then(|v| v.as_str()) {
                println!(
                    "New backbone token (shown once, old token is now invalid for new handshakes):"
                );
                println!("{new_token}");
                println!(
                    "Update any `openmesh-bridge join` gateways; connected peers stay up until they reconnect or are kicked."
                );
            } else {
                println!("Backbone mode is disabled on this bridge.");
            }
        }
        Some(BackboneCommand::Connect { url }) => {
            let value = http_json(
                "POST",
                &base,
                "/api/backbone/config",
                &token,
                Some(json!({ "join_url": url })),
            )
            .await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                println!("Outbound backbone link requested; it reconnects with backoff.");
                if let Some(mode) = value.get("mode").and_then(|v| v.as_str()) {
                    println!("  mode        {mode}");
                }
                println!("  inspect     openmesh-cli backbone peers");
            }
        }
        Some(BackboneCommand::Disconnect { node_id }) => {
            let value = http_json(
                "DELETE",
                &base,
                &format!("/api/backbone/peers/{node_id}"),
                &token,
                None,
            )
            .await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                println!(
                    "backbone link {node_id}: disconnected={}",
                    value
                        .get("disconnected")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                );
            }
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_parsing() {
        assert_eq!(
            ws_authority("ws://127.0.0.1:8787/api/ws").unwrap(),
            "127.0.0.1:8787"
        );
        assert_eq!(
            ws_authority("ws://bridge.local/api/ws").unwrap(),
            "bridge.local:80"
        );
        assert!(ws_authority("garbage").is_ok());
    }

    #[test]
    fn clock_formatting() {
        assert_eq!(clock(0), "00:00:00");
        assert_eq!(clock(3_661_000), "01:01:01");
    }

    #[test]
    fn mime_guessing() {
        assert_eq!(mime_for_path(std::path::Path::new("a.jpg")), "image/jpeg");
        assert_eq!(
            mime_for_path(std::path::Path::new("a.bin")),
            "application/octet-stream"
        );
    }

    #[test]
    fn airtime_runs() {
        assert_eq!(airtime("MEDIUM_FAST", 5120, "EU868", None).unwrap(), 0);
        assert_eq!(airtime("MEDIUM_FAST", 5120, "EU868", Some(50)).unwrap(), 0);
        assert!(airtime("NOPE", 1, "EU868", None).is_err());
        assert!(airtime("MEDIUM_FAST", 1, "NOPE", None).is_err());
        // SHORT_TURBO (500 kHz) is not legal in EU868 (250 kHz) but is in US915 (500 kHz).
        assert!(airtime("SHORT_TURBO", 5120, "EU868", None).is_err());
        assert_eq!(airtime("SHORT_TURBO", 5120, "US915", None).unwrap(), 0);
    }

    #[test]
    fn regulatory_gating_rules() {
        let region = region_legal("EU868").expect("EU868");
        // Exceeding values need the acknowledgement flag.
        assert!(override_requires_risk(true, true));
        // Safe reductions do not.
        assert!(!override_requires_risk(true, false));
        // Disabling needs nothing.
        assert!(!override_requires_risk(false, true));
        let legal_duty = region.duty_cycle_pct.unwrap();
        assert!(override_values_exceed_legal(
            "EU868",
            Some(legal_duty + 1),
            None,
            None
        ));
        assert!(!override_values_exceed_legal(
            "EU868",
            Some(legal_duty),
            None,
            None
        ));
        assert!(override_values_exceed_legal(
            "EU868",
            None,
            Some(region.max_power_dbm + 1),
            None
        ));
        assert!(!override_values_exceed_legal(
            "EU868",
            None,
            Some(region.max_power_dbm),
            None
        ));
        assert!(override_values_exceed_legal(
            "EU868",
            None,
            None,
            Some(region.freq_khz + 1)
        ));
        assert!(!override_values_exceed_legal(
            "EU868",
            None,
            None,
            Some(region.freq_khz)
        ));
        // Unknown regions are conservative.
        assert!(override_values_exceed_legal("MARS", Some(1), None, None));
    }

    #[test]
    fn config_show_renders_legal_and_override() {
        let config = serde_json::json!({
            "node": {"name": "test", "class": "client", "announce_interval_s": 1800},
            "radio": {"profile": "MEDIUM_FAST", "region": "EU868", "tx_power_dbm": 27,
                       "frequency_mhz": 869.525, "interface": "udp"},
            "legal": {"region": "EU868", "max_power_dbm": 27, "duty_cycle_pct": 10,
                       "max_bw_khz": 250, "source": "CEPT"},
            "features": {"auto_announce": true, "store_forward": false, "position_sharing": false},
            "channels": [{"channel_id": "11223344aabbccdd", "name": "family", "psk_set": true}],
            "regulatory_override": {"enabled": true, "acknowledged": true, "duty_cycle_pct": 50,
                                     "max_power_dbm": null, "frequency_khz": null, "reason": "test"},
            "override_active": true,
            "override_warning": "Regulatory override active"
        });
        let rendered = format_config_show(&config);
        assert!(rendered.contains("legal         region=EU868"));
        assert!(rendered.contains("max_power=27 dBm"));
        assert!(rendered.contains("duty=10%"));
        assert!(rendered.contains("WARNING"));
        assert!(rendered.contains("Regulatory override active"));
    }
}
