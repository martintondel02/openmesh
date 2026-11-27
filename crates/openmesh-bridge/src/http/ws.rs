//! WebSocket endpoint (`/api/ws`): the documented event/command protocol.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Role, WebSocketConfig};
use tokio_tungstenite::tungstenite::{Message, handshake::derive_accept_key};

use openmesh_core::NodeId;

use crate::activity;
use crate::http::{HttpRequest, HttpState, authorized};
use crate::json::*;
use crate::runtime::{self, ClientCommand, CommandKind, EngineInput, broadcast_event};

/// Complete the WebSocket handshake and run one client session.
pub async fn upgrade(
    mut stream: TcpStream,
    request: HttpRequest,
    state: Arc<HttpState>,
) -> std::io::Result<()> {
    let Some(key) = request.header("sec-websocket-key") else {
        return crate::http::write_response(
            &mut stream,
            crate::http::HttpResponse::json(
                400,
                serde_json::json!({ "error": "bad_request", "message": "missing Sec-WebSocket-Key" }),
            ),
        )
        .await;
    };
    let upgrade_header = request.header("upgrade").unwrap_or("").to_ascii_lowercase();
    if !upgrade_header.contains("websocket") {
        return crate::http::write_response(
            &mut stream,
            crate::http::HttpResponse::json(
                426,
                serde_json::json!({ "error": "upgrade_required", "message": "expected a WebSocket upgrade" }),
            ),
        )
        .await;
    }

    let accept = derive_accept_key(key.as_bytes());
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;

    let config = WebSocketConfig {
        max_message_size: Some(8 << 20),
        ..Default::default()
    };
    let ws = WebSocketStream::from_raw_socket(stream, Role::Server, Some(config)).await;

    let token_ok = authorized(&request, &state.token);
    let source = activity::source_from_headers(&request.headers);
    session(ws, state, token_ok, source).await
}

/// One WebSocket session. Sends `hello` immediately, then handles commands until close.
async fn session<S>(
    mut ws: WebSocketStream<S>,
    state: Arc<HttpState>,
    token_ok: bool,
    source: String,
) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if !token_ok {
        // Spec §1: WS auth failures use close code 4401.
        let _ = ws
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::Library(4401),
                reason: "unauthorized".into(),
            })))
            .await;
        let _ = ws.close(None).await;
        return Ok(());
    }

    let (reply_tx, mut reply_rx) = mpsc::unbounded_channel::<Arc<str>>();
    let mut events = state.shared.events.subscribe();
    let mut shutdown = state.shutdown.clone();

    {
        let snapshot = state.shared.snapshot.read().await;
        let hello = runtime::build_hello(&snapshot);
        let json = event_json("hello", Some(hello));
        if ws.send(Message::Text(json)).await.is_err() {
            return Ok(());
        }
    }

    let mut flush_tick = tokio::time::interval(std::time::Duration::from_secs(15));
    flush_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                let _ = ws.send(Message::Close(Some(CloseFrame { code: CloseCode::Away, reason: "bridge shutting down".into() }))).await;
                break;
            }
            _ = flush_tick.tick() => {
                if ws.flush().await.is_err() { break; }
            }
            reply = reply_rx.recv() => {
                let Some(json) = reply else { break };
                if ws.send(Message::Text(json.to_string())).await.is_err() { break; }
            }
            event = events.recv() => match event {
                Ok(json) => {
                    if ws.send(Message::Text(json.to_string())).await.is_err() { break; }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            },
            incoming = ws.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    let messages = dispatch(&text, &state, &reply_tx, &source).await;
                    for json in messages {
                        if ws.send(Message::Text(json)).await.is_err() { return Ok(()); }
                    }
                }
                Some(Ok(Message::Binary(_))) => {
                    // The WebSocket API is JSON only; ignore binary frames.
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {}
                Some(Err(_)) => break,
            }
        }
    }
    let _ = ws.close(None).await;
    Ok(())
}

/// Dispatch one client JSON frame. Read-only commands answer inline; mutating commands are
/// forwarded to the engine task, which answers through `reply_tx`.
async fn dispatch(
    text: &str,
    state: &Arc<HttpState>,
    reply_tx: &mpsc::UnboundedSender<Arc<str>>,
    source: &str,
) -> Vec<String> {
    let msg: ClientMsg = match serde_json::from_str(text) {
        Ok(msg) => msg,
        Err(_) => return vec![error_json(None, "bad_request", "invalid JSON envelope")],
    };
    let id = msg.id.as_deref();
    if let Some(summary) = ws_write_summary(&msg) {
        state
            .activity
            .record(source, "WS", "/api/ws", summary, true, None);
    }

    // Read-only commands are answered from the snapshot/store without touching the engine task.
    match msg.typ.as_str() {
        "sync" => return sync_sequence(&msg, state).await,
        "get_contacts" => {
            return match runtime::contacts_json(&state.shared.store) {
                Ok(contacts) => vec![response_json("contacts", id, Some(contacts))],
                Err(e) => vec![error_json(id, "internal", &e.to_string())],
            };
        }
        "get_channels" => {
            return match runtime::channels_json(&state.shared.store) {
                Ok(channels) => vec![response_json("channels", id, Some(channels))],
                Err(e) => vec![error_json(id, "internal", &e.to_string())],
            };
        }
        "get_config" => {
            let snapshot = state.shared.snapshot.read().await;
            return vec![response_json("config", id, Some(snapshot.config.clone()))];
        }
        "get_stats" => {
            let snapshot = state.shared.snapshot.read().await;
            return vec![response_json(
                "stats",
                id,
                Some(runtime::stats_json(&snapshot)),
            )];
        }
        "get_nodes" | "get_nodes_map" => {
            let snapshot = state.shared.snapshot.read().await;
            return vec![response_json(
                "nodes",
                id,
                Some(runtime::nodes_json(&snapshot)),
            )];
        }
        "get_messages" => {
            let data: GetMessagesData = match serde_json::from_value(msg.data.clone()) {
                Ok(data) => data,
                Err(e) => return vec![error_json(id, "bad_request", &e.to_string())],
            };
            let peer = data.peer.as_deref();
            let channel = data.channel_id.as_deref();
            if peer.is_none() && channel.is_none() {
                return vec![error_json(id, "bad_request", "peer or channel_id required")];
            }
            return match runtime::messages_payload(
                &state.shared.store,
                peer,
                channel,
                data.before_ms,
                data.limit,
            ) {
                Ok(payload) => vec![response_json("messages", id, Some(payload))],
                Err(e) => vec![error_json(id, "bad_request", &e.to_string())],
            };
        }
        "contact_qr" => {
            let data: NodeIdData = match serde_json::from_value(msg.data.clone()) {
                Ok(data) => data,
                Err(e) => return vec![error_json(id, "bad_request", &e.to_string())],
            };
            return match contact_qr_url(state, &data.node_id).await {
                Some(url) => vec![response_json("contact_url", id, Some(UrlJson { url }))],
                None => vec![error_json(id, "unknown_peer", "no identity for node")],
            };
        }
        "channel_invite" => {
            let data: ChannelIdData = match serde_json::from_value(msg.data.clone()) {
                Ok(data) => data,
                Err(e) => return vec![error_json(id, "bad_request", &e.to_string())],
            };
            let Some(channel) = parse_node_id(&data.channel_id) else {
                return vec![error_json(id, "bad_request", "invalid channel_id")];
            };
            return match state.shared.store.get_channel(channel) {
                Ok(Some(record)) => match record.psk_bytes() {
                    Some(psk) => vec![response_json(
                        "channel_url",
                        id,
                        Some(UrlJson {
                            url: runtime::channel_url(record.channel_id, &record.name, &psk),
                        }),
                    )],
                    None => vec![error_json(id, "internal", "channel key is corrupt")],
                },
                Ok(None) => vec![error_json(id, "not_found", "unknown channel")],
                Err(e) => vec![error_json(id, "internal", &e.to_string())],
            };
        }
        "delete_message" => {
            let data: MsgIdData = match serde_json::from_value(msg.data.clone()) {
                Ok(data) => data,
                Err(e) => return vec![error_json(id, "bad_request", &e.to_string())],
            };
            return match state.shared.store.delete_message(data.msg_id) {
                Ok(true) => {
                    broadcast_event(
                        &state.shared.events,
                        event_json(
                            "message_update",
                            Some(serde_json::json!({ "msg_id": data.msg_id, "deleted": true })),
                        ),
                    );
                    Vec::new()
                }
                Ok(false) => vec![error_json(id, "not_found", "unknown message id")],
                Err(e) => vec![error_json(id, "internal", &e.to_string())],
            };
        }
        "radio_scan" => {
            return vec![error_json(
                id,
                "radio_busy",
                "radio scan is not supported by this interface",
            )];
        }
        "send_message" => {
            return forward_command(
                state,
                reply_tx,
                id,
                parse_data::<SendMessageData>(&msg).map(CommandKind::SendMessage),
            )
            .await;
        }
        "send_group_message" => {
            return forward_command(
                state,
                reply_tx,
                id,
                parse_data::<SendGroupMessageData>(&msg).map(CommandKind::SendGroupMessage),
            )
            .await;
        }
        "send_position" => {
            return forward_command(
                state,
                reply_tx,
                id,
                parse_data::<SendPositionData>(&msg).map(CommandKind::SendPosition),
            )
            .await;
        }
        "mark_read" => {
            return forward_command(
                state,
                reply_tx,
                id,
                parse_data::<MsgIdData>(&msg).map(CommandKind::MarkRead),
            )
            .await;
        }
        "retry" => {
            return forward_command(
                state,
                reply_tx,
                id,
                parse_data::<MsgIdData>(&msg).map(CommandKind::Retry),
            )
            .await;
        }
        "add_contact" => {
            return forward_command(
                state,
                reply_tx,
                id,
                parse_data::<AddContactData>(&msg).map(CommandKind::AddContact),
            )
            .await;
        }
        "remove_contact" => {
            return forward_command(
                state,
                reply_tx,
                id,
                parse_data::<NodeIdData>(&msg).map(|d| CommandKind::RemoveContact(d.node_id)),
            )
            .await;
        }
        "create_channel" => {
            return forward_command(
                state,
                reply_tx,
                id,
                parse_data::<ChannelCreateData>(&msg).map(CommandKind::CreateChannel),
            )
            .await;
        }
        "join_channel" => {
            return forward_command(
                state,
                reply_tx,
                id,
                parse_data::<ChannelCreateData>(&msg).map(CommandKind::JoinChannel),
            )
            .await;
        }
        "leave_channel" => {
            return forward_command(
                state,
                reply_tx,
                id,
                parse_data::<ChannelIdData>(&msg).map(CommandKind::LeaveChannel),
            )
            .await;
        }
        "set_config" => {
            return forward_command(
                state,
                reply_tx,
                id,
                Ok(CommandKind::SetConfig(msg.data.clone())),
            )
            .await;
        }
        "announce" => {
            return forward_command(state, reply_tx, id, Ok(CommandKind::Announce)).await;
        }
        // 8. Backbone control surface (`BACKBONE.md` §7.2).
        "get_backbone" => {
            let mut data = state.shared.backbone.status_view().await;
            let config = state.shared.backbone.describe().await;
            if let Some(object) = data.as_object_mut() {
                object.insert("config".into(), config);
            }
            return vec![response_json("backbone_status", id, Some(data))];
        }
        "set_backbone_config" => {
            return match state.shared.backbone.update_config(&msg.data).await {
                Ok((config, restarted)) => {
                    let mut data = state.shared.backbone.status_view().await;
                    if let Some(object) = data.as_object_mut() {
                        object.insert("config".into(), config);
                        object.insert("restarted".into(), serde_json::json!(restarted));
                    }
                    vec![response_json("backbone_status", id, Some(data))]
                }
                Err(e) => vec![error_json(id, "bad_request", &e)],
            };
        }
        "backbone_kick" => {
            let data: NodeIdData = match serde_json::from_value(msg.data.clone()) {
                Ok(data) => data,
                Err(e) => return vec![error_json(id, "bad_request", &e.to_string())],
            };
            let Some(node) = parse_node_id(&data.node_id) else {
                return vec![error_json(id, "bad_request", "invalid node id")];
            };
            return match state.shared.backbone.kick(node).await {
                Some(true) => {
                    let mut status = state.shared.backbone.status_view().await;
                    if let Some(object) = status.as_object_mut() {
                        object.insert("disconnected".into(), serde_json::json!(true));
                    }
                    vec![response_json("backbone_status", id, Some(status))]
                }
                Some(false) => vec![error_json(id, "not_found", "peer is not connected")],
                None => vec![response_json(
                    "backbone_status",
                    id,
                    Some(serde_json::json!({ "enabled": false })),
                )],
            };
        }
        "backbone_rotate_token" => {
            return match state.shared.backbone.rotate_token().await {
                Ok(Some(token)) => {
                    vec![response_json(
                        "backbone_token",
                        id,
                        Some(serde_json::json!({ "token": token })),
                    )]
                }
                Ok(None) => vec![response_json(
                    "backbone_status",
                    id,
                    Some(serde_json::json!({ "enabled": false })),
                )],
                Err(e) => vec![error_json(id, "internal", &e)],
            };
        }
        _ => {}
    }

    // Unknown event/command types are ignored so newer clients can talk to older bridges (§7).
    tracing::debug!(typ = %msg.typ, "ignoring unknown client message type");
    Vec::new()
}

/// Summarise a mutating WebSocket command for the activity log (never message bodies or keys).
fn ws_write_summary(msg: &ClientMsg) -> Option<String> {
    let data = &msg.data;
    let text_len = data
        .get("text")
        .and_then(|v| v.as_str())
        .map(str::len)
        .unwrap_or(0);
    Some(match msg.typ.as_str() {
        "send_message" => format!("ws send_message ({text_len} bytes)"),
        "send_group_message" => format!("ws send_group_message ({text_len} bytes)"),
        "send_position" => "ws send_position".to_string(),
        "mark_read" => "ws mark_read".to_string(),
        "retry" => "ws retry".to_string(),
        "add_contact" => "ws add_contact".to_string(),
        "remove_contact" => "ws remove_contact".to_string(),
        "create_channel" => "ws create_channel".to_string(),
        "join_channel" => "ws join_channel".to_string(),
        "leave_channel" => "ws leave_channel".to_string(),
        "set_config" => "ws set_config".to_string(),
        "announce" => "ws announce".to_string(),
        "delete_message" => "ws delete_message".to_string(),
        "set_backbone_config" => "ws set_backbone_config".to_string(),
        "backbone_kick" => "ws backbone_kick".to_string(),
        "backbone_rotate_token" => "ws backbone_rotate_token".to_string(),
        _ => return None,
    })
}

fn parse_data<T: serde::de::DeserializeOwned>(msg: &ClientMsg) -> Result<T, String> {
    serde_json::from_value(msg.data.clone()).map_err(|e| e.to_string())
}

async fn forward_command(
    state: &Arc<HttpState>,
    reply_tx: &mpsc::UnboundedSender<Arc<str>>,
    id: Option<&str>,
    kind: Result<CommandKind, String>,
) -> Vec<String> {
    let kind = match kind {
        Ok(kind) => kind,
        Err(e) => return vec![error_json(id, "bad_request", &e)],
    };
    let cmd = ClientCommand {
        id: id.map(|s| s.to_string()),
        kind,
    };
    match state
        .shared
        .commands
        .send(EngineInput::Command {
            cmd,
            reply: reply_tx.clone(),
        })
        .await
    {
        Ok(()) => Vec::new(),
        Err(_) => vec![error_json(id, "internal", "bridge engine is not available")],
    }
}

/// §4.1: reply with node/message/group_message/config events then `sync_done`.
async fn sync_sequence(msg: &ClientMsg, state: &Arc<HttpState>) -> Vec<String> {
    let id = msg.id.as_deref();
    let data: SyncData =
        serde_json::from_value(msg.data.clone()).unwrap_or(SyncData { since_ms: 0 });
    let mut out = Vec::new();

    let snapshot = state.shared.snapshot.read().await.clone();
    for node in &snapshot.nodes {
        out.push(event_json("node", Some(node.clone())));
    }

    let messages = match runtime::sync_messages(&state.shared.store, data.since_ms) {
        Ok(messages) => messages,
        Err(e) => return vec![error_json(id, "internal", &e.to_string())],
    };
    let count = messages.len();
    for record in messages {
        if record.is_group() {
            out.push(event_json(
                "group_message",
                Some(record.to_group_message_json()),
            ));
        } else {
            out.push(event_json("message", Some(record.to_message_json())));
        }
    }

    out.push(response_json("config", id, Some(snapshot.config.clone())));
    out.push(response_json(
        "sync_done",
        id,
        Some(SyncDoneJson {
            messages: count,
            nodes: snapshot.nodes.len(),
        }),
    ));
    out
}

#[derive(Clone, Debug, serde::Deserialize)]
struct NodeIdData {
    node_id: String,
}

async fn contact_qr_url(state: &Arc<HttpState>, node_id: &str) -> Option<String> {
    let node = parse_node_id(node_id)?;
    // The bridge's own node is not in its own peers/contacts maps, but clients need its contact
    // invite to add the bridge as a contact (so its signed messages survive client restarts).
    {
        let snapshot = state.shared.snapshot.read().await;
        if snapshot.node.node_id == node_id {
            if let Some(public_key) = snapshot.node.public_key.as_deref() {
                if let Some(ident) = identity_from_b64(public_key) {
                    return Some(runtime::contact_url(&ident, &snapshot.node.name));
                }
            }
        }
    }
    if let Ok(Some(contact)) = state.shared.store.get_contact(node) {
        if let Some(ident) = contact.ident() {
            return Some(runtime::contact_url(
                &ident,
                contact.name.as_deref().unwrap_or(""),
            ));
        }
    }
    let snapshot = state.shared.snapshot.read().await;
    let node_json = snapshot.nodes.iter().find(|n| n.node_id == node_id)?;
    let ident = identity_from_b64(&node_json.public_key)?;
    Some(runtime::contact_url(&ident, &node_json.name))
}

/// Resolve a node id string for tests / callers.
pub fn node_id_from_hex(hex: &str) -> Option<NodeId> {
    parse_node_id(hex)
}
