//! Minimal WebSocket client demonstrating the documented bridge round-trip.
//!
//! Starts with `hello`, performs the initial `sync`, sends a direct message to the node given
//! with `--to` (or the first demo contact), then prints the `send_result`, message echo and
//! delivery confirmation frames as they arrive.
//!
//! ```text
//! cargo run -p openmesh-bridge --example ws_roundtrip -- \
//!     --url ws://127.0.0.1:8787/api/ws --token-file ~/.config/openmesh/bridge-token \
//!     --text "hello from the ws example"
//! ```

use std::time::Duration;

use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Parser)]
struct Args {
    /// Bridge WebSocket URL.
    #[arg(long, default_value = "ws://127.0.0.1:8787/api/ws")]
    url: String,
    /// File containing the bridge API token.
    #[arg(long)]
    token_file: Option<std::path::PathBuf>,
    /// Explicit token (takes precedence over the token file).
    #[arg(long)]
    token: Option<String>,
    /// Recipient node id (hex). Defaults to the first seeded demo contact.
    #[arg(long)]
    to: Option<String>,
    /// Message text.
    #[arg(long, default_value = "hello from the ws_roundtrip example")]
    text: String,
    /// How long to wait for the delivery confirmation.
    #[arg(long, default_value_t = 30)]
    timeout: u64,
}

fn token(args: &Args) -> String {
    if let Some(token) = &args.token {
        return token.clone();
    }
    let path = args.token_file.clone().unwrap_or_else(|| {
        let home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        home.join(".config").join("openmesh").join("bridge-token")
    });
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading bridge token {}: {e}", path.display()))
        .trim()
        .to_string()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let token = token(&args);
    let separator = if args.url.contains('?') { '&' } else { '?' };
    let url = format!("{}{separator}token={token}", args.url);
    println!("$ connecting to {}", args.url);
    let (mut ws, _response) = connect_async(&url).await?;

    // 3.1 hello
    let hello = next(&mut ws, Duration::from_secs(5)).await?;
    println!(
        "<- hello: node={} radios={} store={}",
        hello["data"]["node"]["node_id"],
        hello["data"]["radios"]
            .as_array()
            .map(|r| r.len())
            .unwrap_or(0),
        hello["data"]["store"]
    );

    // 4.1 initial sync
    ws.send(Message::Text(
        json!({"v":1,"type":"sync","id":"sync-1","data":{"since_ms":0}}).to_string(),
    ))
    .await?;
    println!("-> sync {{\"since_ms\":0}}");
    let mut nodes: Vec<Value> = Vec::new();
    loop {
        let frame = next(&mut ws, Duration::from_secs(5)).await?;
        match frame["type"].as_str().unwrap_or("") {
            "node" => nodes.push(frame["data"].clone()),
            "message" | "group_message" => {}
            "config" => println!(
                "<- config: node.name={} radio.profile={}",
                frame["data"]["node"]["name"], frame["data"]["radio"]["profile"]
            ),
            "sync_done" => {
                println!(
                    "<- sync_done: {} messages, {} nodes",
                    frame["data"]["messages"], frame["data"]["nodes"]
                );
                break;
            }
            other => println!("<- {other}"),
        }
    }

    // Choose a demo contact when no recipient was supplied.
    let to = match args.to.clone() {
        Some(to) => to,
        None => {
            let contact = nodes
                .iter()
                .find(|n| {
                    n["is_contact"].as_bool() == Some(true)
                        && n["name"] != Value::Null
                        && n["name"].as_str() != Some("")
                })
                .cloned();
            match contact {
                Some(node) => {
                    println!(
                        "(using demo contact {} = {})",
                        node["name"], node["node_id"]
                    );
                    node["node_id"].as_str().unwrap_or_default().to_string()
                }
                None => anyhow::bail!("no --to given and no demo contacts in the node table"),
            }
        }
    };

    // 4.2 send_message
    ws.send(Message::Text(
        json!({"v":1,"type":"send_message","id":"m1","data":{"to":to,"text":args.text}})
            .to_string(),
    ))
    .await?;
    println!(
        "-> send_message {{\"to\":\"{to}\",\"text\":{:?}}}",
        args.text
    );

    let mut msg_id: Option<u64> = None;
    let mut saw_echo = false;
    let mut saw_delivery = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(args.timeout);
    while tokio::time::Instant::now() < deadline && !saw_delivery {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Ok(frame) = next(&mut ws, remaining).await else {
            break;
        };
        let typ = frame["type"].as_str().unwrap_or("");
        let data = &frame["data"];
        match typ {
            "send_result" if frame["id"] == "m1" => {
                msg_id = data["msg_id"].as_u64();
                println!(
                    "<- send_result: msg_id={} est_airtime_ms={} queued={}",
                    data["msg_id"], data["est_airtime_ms"], data["queued"]
                );
            }
            "message" if data["outgoing"] == true => {
                saw_echo = true;
                println!(
                    "<- message (echo): msg_id={} peer={} kind={} text={:?}",
                    data["msg_id"], data["peer"], data["kind"], data["text"]
                );
            }
            "message" if data["peer"] == to => {
                println!(
                    "<- message (incoming): msg_id={} from={} text={:?}",
                    data["msg_id"], data["from"], data["text"]
                );
            }
            "ack" if msg_id.is_none() || data["msg_id"].as_u64() == msg_id => {
                saw_delivery = true;
                println!(
                    "<- ack: msg_id={} peer={} kind={} at_ms={}",
                    data["msg_id"], data["peer"], data["kind"], data["at_ms"]
                );
            }
            "message_update" if data["delivered"] == true => {
                saw_delivery = true;
                println!(
                    "<- message_update: msg_id={} delivered=true",
                    data["msg_id"]
                );
            }
            "status" | "position" | "telemetry" => {}
            other => println!("<- {other}: {data}"),
        }
    }
    if !saw_echo {
        println!("(no echo observed)");
    }
    if !saw_delivery {
        println!("(no delivery confirmation within {}s)", args.timeout);
        std::process::exit(1);
    }
    println!("round-trip complete");
    Ok(())
}

async fn next(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    timeout: Duration,
) -> Result<Value, anyhow::Error> {
    loop {
        let frame = tokio::time::timeout(timeout, ws.next())
            .await
            .map_err(|_| anyhow::anyhow!("timed out waiting for a WebSocket frame"))?
            .ok_or_else(|| anyhow::anyhow!("bridge closed the connection"))??;
        if let Ok(text) = frame.to_text() {
            return Ok(serde_json::from_str(text)?);
        }
    }
}
