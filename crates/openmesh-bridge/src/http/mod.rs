//! Loopback HTTP + WebSocket server.
//!
//! Hand-rolled on tokio + httparse: the bridge only needs GET/POST/OPTIONS on a trusted
//! loopback socket, a WebSocket upgrade, raw attachment uploads and static file serving, so a
//! full web framework would add dependency surface without buying anything. The WebSocket
//! protocol itself is handled by tokio-tungstenite.

pub mod mcp;
pub mod ws;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use crate::activity::{self, ActivityLog};
use crate::attachments::MAX_ATTACHMENT_BYTES;
use crate::gateway::{GatewayManager, InboundOutcome};
use crate::json::{b64, error_json, event_json, parse_node_id};
use crate::runtime::{self, CommandError, CommandKind, Shared};
use crate::store::Store;
use crate::transport::backbone::BackboneManager;

use serde_json::Value;

/// Maximum HTTP header block we accept.
const MAX_HEADER_BYTES: usize = 64 * 1024;
/// Maximum body for non-attachment endpoints.
const MAX_JSON_BODY: usize = 1024 * 1024;

/// Everything a connection needs.
pub struct HttpState {
    pub token: Arc<String>,
    pub shared: Arc<Shared>,
    pub backbone: Arc<BackboneManager>,
    /// Bounded transparency log for mutating API calls (AI-NATIVE.md §2).
    pub activity: Arc<ActivityLog>,
    /// Authenticated webhook gateways (remote-operations.md §2).
    pub gateways: Arc<GatewayManager>,
    /// Opt-in remote MCP transport (`docs/design/remote-operations.md` §1).
    pub mcp_http: Arc<mcp::McpHttp>,
    /// Local API address, used by the MCP HTTP dispatcher to reach the REST API in-process.
    pub local_addr: SocketAddr,
    pub web_root: Option<PathBuf>,
    pub shutdown: watch::Receiver<bool>,
}

/// Parsed request head plus body.
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(|s| s.as_str())
    }

    pub fn query_param(&self, name: &str) -> Option<&str> {
        self.query.get(name).map(|s| s.as_str())
    }
}

pub struct HttpResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    pub extra_headers: Vec<(String, String)>,
}

impl HttpResponse {
    pub fn json(status: u16, value: serde_json::Value) -> Self {
        Self {
            status,
            content_type: "application/json".to_string(),
            body: value.to_string().into_bytes(),
            extra_headers: Vec::new(),
        }
    }

    pub fn raw_json(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "application/json".to_string(),
            body: body.into_bytes(),
            extra_headers: Vec::new(),
        }
    }

    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8".to_string(),
            body: body.into().into_bytes(),
            extra_headers: Vec::new(),
        }
    }

    pub fn bytes(status: u16, content_type: impl Into<String>, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type: content_type.into(),
            body,
            extra_headers: Vec::new(),
        }
    }

    fn error(status: u16, code: &str) -> Self {
        Self::json(status, serde_json::json!({ "error": code }))
    }
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        426 => "Upgrade Required",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

enum ReadError {
    Malformed(String),
    TooLarge,
    Io,
}

/// Serve HTTP/WS on an already-bound listener until shutdown.
pub async fn serve(listener: TcpListener, state: Arc<HttpState>) -> anyhow::Result<()> {
    let mut shutdown = state.shutdown.clone();
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            accepted = listener.accept() => {
                let Ok((stream, peer)) = accepted else { continue };
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, peer, state).await {
                        tracing::debug!(%peer, error = %e, "http connection error");
                    }
                });
            }
        }
    }
    tracing::debug!("http server stopped");
    Ok(())
}

async fn handle_connection(
    mut stream: TcpStream,
    peer: SocketAddr,
    state: Arc<HttpState>,
) -> std::io::Result<()> {
    let _ = stream.set_nodelay(true);
    let request = match read_request(
        &mut stream,
        MAX_HEADER_BYTES + MAX_ATTACHMENT_BYTES + MAX_JSON_BODY,
    )
    .await
    {
        Ok(Some(request)) => request,
        Ok(None) => return Ok(()),
        Err(ReadError::TooLarge) => {
            let resp = HttpResponse::error(413, "too_large");
            return write_response(&mut stream, resp).await;
        }
        Err(ReadError::Malformed(message)) => {
            tracing::debug!(%peer, message, "rejected malformed HTTP request");
            let resp = HttpResponse::error(400, "bad_request");
            return write_response(&mut stream, resp).await;
        }
        Err(ReadError::Io) => return Ok(()),
    };

    // WebSocket upgrade and SSE are streaming endpoints, handled before the JSON router.
    if request.path == "/api/ws" {
        if request.method != "GET" {
            return write_response(&mut stream, HttpResponse::error(405, "method_not_allowed"))
                .await;
        }
        return ws::upgrade(stream, request, state).await;
    }
    if request.path == "/api/events" && request.method == "GET" {
        if !authorized(&request, &state.token) {
            return write_response(&mut stream, HttpResponse::error(401, "unauthorized")).await;
        }
        return stream_sse(stream, state).await;
    }
    // Remote MCP over HTTP is opt-in and handles its own status/auth/SSE responses. The path is
    // reserved even when disabled so it returns 404 instead of the web SPA fallback.
    if request.path == state.mcp_http.path() {
        return mcp::handle(&mut stream, &request, &state).await;
    }

    let response = route(&request, &state).await;
    record_activity(&request, &response, &state);
    write_response(&mut stream, response).await
}

/// Token check for REST + WS: `Authorization: Bearer <token>` or `?token=<token>`.
pub fn authorized(request: &HttpRequest, token: &str) -> bool {
    if let Some(header) = request.header("authorization") {
        let header = header.trim();
        if let Some(bearer) = header
            .strip_prefix("Bearer ")
            .or_else(|| header.strip_prefix("bearer "))
        {
            if constant_time_eq(bearer.trim().as_bytes(), token.as_bytes()) {
                return true;
            }
        }
    }
    if let Some(query_token) = request.query_param("token") {
        return constant_time_eq(query_token.as_bytes(), token.as_bytes());
    }
    false
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

async fn route(request: &HttpRequest, state: &Arc<HttpState>) -> HttpResponse {
    let path = request.path.as_str();

    // CORS preflight for the dev-mode web client.
    if request.method == "OPTIONS" {
        return HttpResponse {
            status: 204,
            content_type: "text/plain".to_string(),
            body: Vec::new(),
            extra_headers: vec![
                (
                    "Access-Control-Allow-Methods".into(),
                    "GET, POST, PUT, PATCH, DELETE, OPTIONS".into(),
                ),
                (
                    "Access-Control-Allow-Headers".into(),
                    "Authorization, Content-Type, X-OpenMesh-Filename, X-OpenMesh-Duration-Ms, \
                     X-OpenMesh-Agent, X-OpenMesh-Client, X-OpenMesh-Signature"
                        .into(),
                ),
                ("Access-Control-Max-Age".into(), "86400".into()),
            ],
        };
    }

    if path == "/api/health" {
        return HttpResponse::json(
            200,
            serde_json::json!({ "ok": true, "version": openmesh_core::CRATE_VERSION }),
        );
    }
    // Inbound gateway webhooks authenticate with their own HMAC signature over the raw body
    // (adapters do not hold the bridge API token); everything else under /api/ needs the token.
    let gateway_inbound =
        request.method == "POST" && gateway_route_id(path, Some("/inbound")).is_some();
    if path.starts_with("/api/") && !authorized(request, &state.token) && !gateway_inbound {
        return HttpResponse::error(401, "unauthorized");
    }

    match (request.method.as_str(), path) {
        ("GET", "/api/status") => {
            let snapshot = state.shared.snapshot.read().await;
            HttpResponse::json(200, crate::json::json_value(&snapshot.status))
        }
        ("GET", "/api/config") => {
            let snapshot = state.shared.snapshot.read().await;
            HttpResponse::json(200, crate::json::json_value(&snapshot.config))
        }
        ("GET", "/api/nodes") => {
            let snapshot = state.shared.snapshot.read().await;
            HttpResponse::json(
                200,
                serde_json::to_value(&snapshot.nodes).unwrap_or_default(),
            )
        }
        ("GET", "/api/contacts") => match runtime::contacts_json(&state.shared.store) {
            Ok(contacts) => HttpResponse::json(200, crate::json::json_value(&contacts)),
            Err(e) => internal_error(e),
        },
        ("GET", "/api/channels") => match runtime::channels_json(&state.shared.store) {
            Ok(channels) => HttpResponse::json(200, crate::json::json_value(&channels)),
            Err(e) => internal_error(e),
        },
        ("GET", "/api/messages") => {
            let peer = request.query_param("peer");
            let limit = request
                .query_param("limit")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(100);
            let before_ms = request
                .query_param("before_ms")
                .and_then(|v| v.parse::<u64>().ok());
            match runtime::messages_payload(&state.shared.store, peer, None, before_ms, limit) {
                Ok(payload) => HttpResponse::json(200, payload),
                Err(e) => HttpResponse::error(400, &format!("{e}")),
            }
        }
        ("POST", "/api/attachments") => upload_attachment(request, state).await,
        ("GET", "/") => serve_static(request, state).await,
        ("GET", _) if path.starts_with("/api/nodes/") => {
            let id = &path["/api/nodes/".len()..];
            let snapshot = state.shared.snapshot.read().await;
            match snapshot.nodes.iter().find(|n| n.node_id == id) {
                Some(node) => HttpResponse::json(200, crate::json::json_value(node)),
                None => HttpResponse::error(404, "not_found"),
            }
        }
        ("GET", _) if path.starts_with("/api/messages/channel/") => {
            let channel = &path["/api/messages/channel/".len()..];
            let limit = request
                .query_param("limit")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(100);
            let before_ms = request
                .query_param("before_ms")
                .and_then(|v| v.parse::<u64>().ok());
            match runtime::messages_payload(
                &state.shared.store,
                None,
                Some(channel),
                before_ms,
                limit,
            ) {
                Ok(payload) => HttpResponse::json(200, payload),
                Err(e) => HttpResponse::error(400, &format!("{e}")),
            }
        }
        ("GET", _) if path.starts_with("/api/attachments/") => {
            let id = &path["/api/attachments/".len()..];
            match state.shared.attachments.read(id).await {
                Ok(Some((meta, bytes))) => HttpResponse::bytes(200, meta.mime, bytes),
                Ok(None) => HttpResponse::error(404, "not_found"),
                Err(e) => internal_error(e),
            }
        }
        ("GET", "/api/backbone") => HttpResponse::json(200, state.backbone.describe().await),
        ("GET", "/api/backbone/peers") => {
            HttpResponse::json(200, state.backbone.peers_view().await)
        }
        // --- v0.7.0 multiple outbound links (remote-operations.md §3) ---------------------
        ("GET", "/api/backbone/links") => {
            HttpResponse::json(200, state.backbone.links_view().await)
        }
        ("PUT", _) if backbone_link_name(path).is_some() => {
            let name = backbone_link_name(path).unwrap_or_default().to_string();
            let body = match parse_json_body(request) {
                Ok(value) => value,
                Err(response) => return response,
            };
            match state.backbone.upsert_link(&name, &body).await {
                Ok(view) => HttpResponse::json(200, view),
                Err(e) => bad_request(&e),
            }
        }
        ("DELETE", _) if backbone_link_name(path).is_some() => {
            let name = backbone_link_name(path).unwrap_or_default().to_string();
            match state.backbone.delete_link(&name).await {
                Ok(true) => {
                    HttpResponse::json(200, serde_json::json!({ "deleted": true, "name": name }))
                }
                Ok(false) => HttpResponse::json(
                    404,
                    serde_json::json!({ "error": "not_found", "name": name }),
                ),
                Err(e) => HttpResponse::json(
                    500,
                    serde_json::json!({ "error": "internal", "message": e }),
                ),
            }
        }
        ("POST", "/api/backbone/config") => {
            let body: serde_json::Value = match serde_json::from_slice(&request.body) {
                Ok(value) => value,
                Err(e) => {
                    return HttpResponse::json(
                        400,
                        serde_json::json!({ "error": "bad_request", "message": e.to_string() }),
                    );
                }
            };
            match state.backbone.update_config(&body).await {
                Ok((mut view, restarted)) => {
                    if let Some(object) = view.as_object_mut() {
                        object.insert("restarted".into(), serde_json::json!(restarted));
                    }
                    HttpResponse::json(200, view)
                }
                Err(e) => HttpResponse::json(
                    400,
                    serde_json::json!({ "error": "bad_request", "message": e }),
                ),
            }
        }
        ("POST", "/api/backbone/token/rotate") => match state.backbone.rotate_token().await {
            Ok(Some(token)) => HttpResponse::json(200, serde_json::json!({ "token": token })),
            Ok(None) => HttpResponse::json(200, serde_json::json!({ "enabled": false })),
            Err(e) => HttpResponse::json(
                500,
                serde_json::json!({ "error": "internal", "message": e }),
            ),
        },
        ("GET", "/api/backbone/setup") => HttpResponse::json(200, state.backbone.setup().await),
        ("GET", "/api/backbone/qr.svg") => match state.backbone.setup_qr_svg().await {
            Ok(Some(svg)) => HttpResponse::bytes(200, "image/svg+xml", svg.into_bytes()),
            Ok(None) => HttpResponse::json(200, serde_json::json!({ "enabled": false })),
            Err(e) => HttpResponse::json(
                500,
                serde_json::json!({ "error": "internal", "message": e }),
            ),
        },
        ("DELETE", _) if path.starts_with("/api/backbone/peers/") => {
            let id = &path["/api/backbone/peers/".len()..];
            let Some(node) = crate::json::parse_node_id(id) else {
                return HttpResponse::json(
                    400,
                    serde_json::json!({ "error": "bad_request", "message": "invalid node id" }),
                );
            };
            match state.backbone.kick(node).await {
                Some(true) => HttpResponse::json(200, serde_json::json!({ "disconnected": true })),
                Some(false) => HttpResponse::json(
                    404,
                    serde_json::json!({ "disconnected": false, "error": "not_found" }),
                ),
                None => HttpResponse::json(200, serde_json::json!({ "enabled": false })),
            }
        }
        // --- v0.7.0 gateways: authenticated webhooks (remote-operations.md §2) ------------
        ("GET", "/api/gateways") => HttpResponse::json(200, state.gateways.list_view().await),
        ("PUT", _) if gateway_route_id(path, None).is_some() => {
            let id = gateway_route_id(path, None).unwrap_or_default().to_string();
            let body = match parse_json_body(request) {
                Ok(value) => value,
                Err(response) => return response,
            };
            match state.gateways.upsert(&id, &body).await {
                Ok(view) => HttpResponse::json(200, view),
                Err(e) => bad_request(&e),
            }
        }
        ("DELETE", _) if gateway_route_id(path, None).is_some() => {
            let id = gateway_route_id(path, None).unwrap_or_default().to_string();
            match state.gateways.delete(&id).await {
                Ok(true) => {
                    HttpResponse::json(200, serde_json::json!({ "deleted": true, "id": id }))
                }
                Ok(false) => {
                    HttpResponse::json(404, serde_json::json!({ "error": "not_found", "id": id }))
                }
                Err(e) => HttpResponse::json(
                    500,
                    serde_json::json!({ "error": "internal", "message": e }),
                ),
            }
        }
        ("POST", _) if gateway_route_id(path, Some("/test")).is_some() => {
            let id = gateway_route_id(path, Some("/test"))
                .unwrap_or_default()
                .to_string();
            match state.gateways.test(&id).await {
                Ok(result) => HttpResponse::json(200, result),
                Err(e) if e.contains("not found") => HttpResponse::json(
                    404,
                    serde_json::json!({ "error": "not_found", "message": e }),
                ),
                Err(e) => bad_request(&e),
            }
        }
        ("POST", _) if gateway_route_id(path, Some("/inbound")).is_some() => {
            let id = gateway_route_id(path, Some("/inbound"))
                .unwrap_or_default()
                .to_string();
            let outcome: InboundOutcome = state
                .gateways
                .handle_inbound(
                    &state.shared,
                    &id,
                    request.header("x-openmesh-signature"),
                    &request.body,
                )
                .await;
            HttpResponse::json(outcome.status, outcome.body)
        }
        // --- v0.3.0: AI-native + help surface (AI-NATIVE.md, ux-help-system.md §5) ---------
        ("GET", "/api/help") => HttpResponse::json(200, crate::help::index_value()),
        ("GET", _) if path.starts_with("/api/help/") => {
            let slug = &path["/api/help/".len()..];
            match crate::help::topic_value(slug) {
                Some(topic) => HttpResponse::json(200, topic),
                None => HttpResponse::json(
                    404,
                    serde_json::json!({
                        "error": "not_found",
                        "code": "not_found",
                        "message": format!("unknown help topic '{slug}'"),
                        "help_topic": "getting-started",
                        "hint": "GET /api/help lists every topic; MCP resources openmesh://help/{slug}",
                    }),
                ),
            }
        }
        ("GET", "/api/setup/state") => {
            let state_json = crate::setup::state(
                &state.shared,
                &state.backbone,
                &state.activity,
                !state.token.is_empty(),
            )
            .await;
            HttpResponse::json(200, state_json)
        }
        ("GET", "/api/setup/recommend") => {
            let country = request.query_param("country");
            let role = request.query_param("role");
            let snapshot = state.shared.snapshot.read().await;
            let value = crate::setup::recommend(country, role, &snapshot.config);
            HttpResponse::json(200, value)
        }
        ("POST", "/api/setup/quick") => {
            let body = match parse_json_body(request) {
                Ok(value) => value,
                Err(response) => return response,
            };
            match crate::setup::quick(&state.shared, &body).await {
                Ok(value) => HttpResponse::json(200, value),
                Err(e) => command_error_response(e),
            }
        }
        ("GET", "/api/activity") => {
            let limit = request
                .query_param("limit")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(100);
            HttpResponse::json(200, state.activity.payload(limit))
        }
        ("GET", "/api/openapi.json") => HttpResponse::bytes(
            200,
            "application/json",
            crate::help::openapi_json().as_bytes().to_vec(),
        ),
        // Write endpoints used by the MCP server and CLI/agent clients.
        ("POST", "/api/messages") => {
            let body = match parse_json_body(request) {
                Ok(value) => value,
                Err(response) => return response,
            };
            let data: crate::json::SendMessageData = match serde_json::from_value(body) {
                Ok(data) => data,
                Err(e) => return bad_request(&e.to_string()),
            };
            command_response(state, CommandKind::SendMessage(data)).await
        }
        ("POST", _) if path.starts_with("/api/channels/") && path.ends_with("/messages") => {
            let channel_id =
                path["/api/channels/".len()..path.len() - "/messages".len()].to_string();
            let body = match parse_json_body(request) {
                Ok(value) => value,
                Err(response) => return response,
            };
            let data = crate::json::SendGroupMessageData {
                channel_id,
                text: body
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                attachment_id: body
                    .get("attachment_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            };
            command_response(state, CommandKind::SendGroupMessage(data)).await
        }
        ("POST", "/api/position") => {
            let body = match parse_json_body(request) {
                Ok(value) => value,
                Err(response) => return response,
            };
            let data: crate::json::SendPositionData = match serde_json::from_value(body) {
                Ok(data) => data,
                Err(e) => return bad_request(&e.to_string()),
            };
            command_response(state, CommandKind::SendPosition(data)).await
        }
        ("POST", "/api/announce") => {
            match runtime::queue_command(&state.shared, CommandKind::Announce).await {
                Ok(()) => {
                    HttpResponse::json(200, serde_json::json!({ "ok": true, "queued": true }))
                }
                Err(e) => command_error_response(e),
            }
        }
        ("PATCH", "/api/config") | ("POST", "/api/config") => {
            let body = match parse_json_body(request) {
                Ok(value) => value,
                Err(response) => return response,
            };
            command_response(state, CommandKind::SetConfig(body)).await
        }
        ("POST", "/api/channels") => {
            let body = match parse_json_body(request) {
                Ok(value) => value,
                Err(response) => return response,
            };
            let data: crate::json::ChannelCreateData = match serde_json::from_value(body) {
                Ok(data) => data,
                Err(e) => return bad_request(&e.to_string()),
            };
            match runtime::run_command(&state.shared, CommandKind::CreateChannel(data)).await {
                Ok(created) => {
                    let invite_url = created
                        .get("channel_id")
                        .and_then(|v| v.as_str())
                        .and_then(crate::json::parse_node_id)
                        .zip(
                            created
                                .get("psk")
                                .and_then(|v| v.as_str())
                                .and_then(crate::json::b64_decode)
                                .and_then(|raw| <[u8; 32]>::try_from(raw).ok()),
                        )
                        .map(|(id, psk)| {
                            runtime::channel_url(
                                id,
                                created
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("channel"),
                                &psk,
                            )
                        });
                    HttpResponse::json(
                        200,
                        serde_json::json!({
                            "channel_id": created.get("channel_id"),
                            "name": created.get("name"),
                            "psk_once": created.get("psk"),
                            "invite_url": invite_url,
                            "shown_once": true,
                        }),
                    )
                }
                Err(e) => command_error_response(e),
            }
        }
        ("POST", "/api/channels/join") => {
            let body = match parse_json_body(request) {
                Ok(value) => value,
                Err(response) => return response,
            };
            let (name, psk) = if let Some(url) = body.get("url").and_then(|v| v.as_str()) {
                match runtime::parse_channel_url(url) {
                    Some((name, psk)) => (name, psk),
                    None => return bad_request("invalid channel invite URL"),
                }
            } else {
                let Some(name) = body.get("name").and_then(|v| v.as_str()) else {
                    return bad_request("url or name+psk required");
                };
                let Some(psk) = body
                    .get("psk")
                    .and_then(|v| v.as_str())
                    .and_then(crate::json::b64_decode)
                    .and_then(|raw| <[u8; 32]>::try_from(raw).ok())
                else {
                    return bad_request("psk must be base64 of 32 bytes");
                };
                (name.to_string(), psk)
            };
            let data = crate::json::ChannelCreateData {
                name,
                psk: Some(crate::json::b64(&psk)),
            };
            command_response(state, CommandKind::JoinChannel(data)).await
        }
        ("POST", "/api/contacts") => {
            let body = match parse_json_body(request) {
                Ok(value) => value,
                Err(response) => return response,
            };
            let data = if let Some(url) = body.get("invite_url").and_then(|v| v.as_str()) {
                match runtime::parse_contact_url(url) {
                    Some((node, name, identity)) => crate::json::AddContactData {
                        node_id: node.to_hex(),
                        name,
                        identity: Some(crate::json::identity_to_b64(&identity)),
                        verify_key: true,
                    },
                    None => return bad_request("invalid contact invite URL"),
                }
            } else {
                match serde_json::from_value::<crate::json::AddContactData>(body) {
                    Ok(mut data) => {
                        data.verify_key = true;
                        data
                    }
                    Err(e) => return bad_request(&e.to_string()),
                }
            };
            command_response(state, CommandKind::AddContact(data)).await
        }
        ("POST", _) if path.starts_with("/api/messages/") && path.ends_with("/read") => {
            let id = &path["/api/messages/".len()..path.len() - "/read".len()];
            let Some(msg_id) = id.parse::<u32>().ok() else {
                return bad_request("invalid message id");
            };
            match runtime::queue_command(
                &state.shared,
                CommandKind::MarkRead(crate::json::MsgIdData { msg_id }),
            )
            .await
            {
                Ok(()) => {
                    HttpResponse::json(200, serde_json::json!({ "ok": true, "msg_id": msg_id }))
                }
                Err(e) => command_error_response(e),
            }
        }
        ("POST", _) if path.starts_with("/api/messages/") && path.ends_with("/retry") => {
            let id = &path["/api/messages/".len()..path.len() - "/retry".len()];
            let Some(msg_id) = id.parse::<u32>().ok() else {
                return bad_request("invalid message id");
            };
            command_response(state, CommandKind::Retry(crate::json::MsgIdData { msg_id })).await
        }
        ("GET", _) if path.starts_with("/api/") => HttpResponse::error(404, "not_found"),
        ("GET", _) | ("HEAD", _) => serve_static(request, state).await,
        _ if path.starts_with("/api/") => HttpResponse::error(405, "method_not_allowed"),
        _ => HttpResponse::error(404, "not_found"),
    }
}

/// Extract `{id}` from `/api/gateways/{id}[suffix]`; rejects extra path segments.
fn gateway_route_id<'a>(path: &'a str, suffix: Option<&str>) -> Option<&'a str> {
    let rest = path.strip_prefix("/api/gateways/")?;
    let id = match suffix {
        Some(suffix) => rest.strip_suffix(suffix)?,
        None => rest,
    };
    (!id.is_empty() && !id.contains('/')).then_some(id)
}

/// Extract `{name}` from `/api/backbone/links/{name}`; rejects extra path segments.
fn backbone_link_name(path: &str) -> Option<&str> {
    let name = path.strip_prefix("/api/backbone/links/")?;
    (!name.is_empty() && !name.contains('/')).then_some(name)
}

fn parse_json_body(request: &HttpRequest) -> Result<Value, HttpResponse> {
    serde_json::from_slice(&request.body).map_err(|e| {
        HttpResponse::json(
            400,
            serde_json::json!({
                "error": "bad_request",
                "code": "bad_request",
                "message": e.to_string(),
                "help_topic": "getting-started",
            }),
        )
    })
}

fn bad_request(message: &str) -> HttpResponse {
    HttpResponse::json(
        400,
        serde_json::json!({
            "error": "bad_request",
            "code": "bad_request",
            "message": message,
            "help_topic": "getting-started",
        }),
    )
}

/// Map an engine command error to an HTTP response with the MCP-style code/help_topic fields.
fn command_error_response(error: CommandError) -> HttpResponse {
    let regulatory = error.code.starts_with("regulatory_override_");
    let status = match error.code.as_str() {
        "bad_request" => 400,
        "not_found" | "unknown_peer" => 404,
        "radio_busy" => 409,
        "unavailable" | "timeout" => 503,
        // Regulatory gating and profile/region legality are validation failures (contract §2.3).
        _ if regulatory || error.code == "profile_not_allowed_in_region" => 422,
        _ => 400,
    };
    let mut body = serde_json::json!({
        "error": error.code,
        "code": error.code,
        "message": error.message,
        "help_topic": help_topic_for_code(&error.code),
    });
    if regulatory {
        body["warning"] = serde_json::json!(openmesh_core::OVERRIDE_WARNING);
    }
    HttpResponse::json(status, body)
}

fn help_topic_for_code(code: &str) -> &'static str {
    match code {
        "unknown_peer" | "not_contact" => "contacts-identity",
        "not_found" => "troubleshooting",
        "radio_busy" | "radio_unavailable" => "radio-and-region",
        "timeout" | "unavailable" | "internal" => "troubleshooting",
        "confirm_required" => "agents",
        "profile_not_allowed_in_region" => "radio-and-region",
        code if code.starts_with("regulatory_override_") => "regulatory-and-emergency",
        _ => "getting-started",
    }
}

async fn command_response(state: &Arc<HttpState>, kind: CommandKind) -> HttpResponse {
    match runtime::run_command(&state.shared, kind).await {
        Ok(value) => HttpResponse::json(200, value),
        Err(e) => command_error_response(e),
    }
}

/// Record one mutating REST call in the bounded activity log (never bodies or secrets).
fn record_activity(request: &HttpRequest, response: &HttpResponse, state: &HttpState) {
    if !activity::is_write_method(&request.method) {
        return;
    }
    if !request.path.starts_with("/api/") || request.path == "/api/health" {
        return;
    }
    // Only authenticated mutations are logged; failed auth attempts are not actions.
    if !authorized(request, &state.token) {
        return;
    }
    let source = activity::source_from_headers(&request.headers);
    let ok = (200..300).contains(&response.status);
    let error = if ok {
        None
    } else {
        let value: Option<Value> = serde_json::from_slice(&response.body).ok();
        value
            .as_ref()
            .and_then(|value| value.get("message").and_then(|v| v.as_str()))
            .map(|message| message.chars().take(200).collect())
    };
    let summary = summarize_activity(request);
    state
        .activity
        .record(&source, &request.method, &request.path, summary, ok, error);
}

fn summarize_activity(request: &HttpRequest) -> String {
    let path = request.path.as_str();
    let body: Option<Value> = if request.body.len() <= MAX_JSON_BODY {
        serde_json::from_slice(&request.body).ok()
    } else {
        None
    };
    let text = |key: &str| {
        body.as_ref()
            .and_then(|body| body.get(key))
            .and_then(|value| value.as_str())
    };
    match (request.method.as_str(), path) {
        ("POST", "/api/messages") => format!(
            "send direct message to {} ({} bytes)",
            text("to").unwrap_or("?"),
            text("text").map(str::len).unwrap_or(0)
        ),
        ("POST", _) if path.starts_with("/api/channels/") && path.ends_with("/messages") => {
            format!(
                "send channel message ({} bytes)",
                text("text").map(str::len).unwrap_or(0)
            )
        }
        ("POST", "/api/attachments") => format!(
            "upload attachment ({} bytes, {})",
            request.body.len(),
            request.header("content-type").unwrap_or("unknown")
        ),
        ("POST", "/api/position") => "share position".to_string(),
        ("POST", "/api/announce") => "announce".to_string(),
        ("PATCH", "/api/config") | ("POST", "/api/config") => {
            let override_patch = body
                .as_ref()
                .and_then(|body| body.get("regulatory_override"))
                .filter(|value| value.is_object());
            match override_patch {
                Some(patch) => {
                    let enabled = patch.get("enabled").and_then(|v| v.as_bool());
                    let mut summary = String::from("regulatory override");
                    if let Some(enabled) = enabled {
                        summary.push_str(&format!(" enabled={enabled}"));
                    }
                    if let Some(duty) = patch.get("duty_cycle_pct").and_then(|v| v.as_u64()) {
                        summary.push_str(&format!(" duty={duty}%"));
                    }
                    if let Some(power) = patch.get("max_power_dbm").and_then(|v| v.as_i64()) {
                        summary.push_str(&format!(" power={power}dBm"));
                    }
                    if let Some(frequency) = patch.get("frequency_khz").and_then(|v| v.as_u64()) {
                        summary.push_str(&format!(" frequency={frequency}kHz"));
                    }
                    if patch.get("acknowledged").and_then(|v| v.as_bool()) == Some(true)
                        || patch.get("confirm").and_then(|v| v.as_bool()) == Some(true)
                    {
                        summary.push_str(" acknowledged=true");
                    }
                    if let Some(reason) = patch.get("reason").and_then(|v| v.as_str()) {
                        let reason: String = reason.chars().take(80).collect();
                        summary.push_str(&format!(" reason={reason:?}"));
                    }
                    summary
                }
                None => "update config".to_string(),
            }
        }
        ("POST", "/api/channels") => {
            format!("create channel {}", text("name").unwrap_or("?"))
        }
        ("POST", "/api/channels/join") => "join channel".to_string(),
        ("POST", "/api/contacts") => format!(
            "add contact {}",
            text("node_id")
                .or_else(|| text("invite_url"))
                .map(|v| { v.chars().take(40).collect::<String>() })
                .unwrap_or_else(|| "via invite".to_string())
        ),
        ("PUT", _) if path.starts_with("/api/gateways/") => format!(
            "upsert gateway {}",
            gateway_route_id(path, None).unwrap_or("?")
        ),
        ("DELETE", _) if path.starts_with("/api/gateways/") => format!(
            "remove gateway {}",
            gateway_route_id(path, None).unwrap_or("?")
        ),
        ("POST", _) if path.starts_with("/api/gateways/") && path.ends_with("/test") => format!(
            "test gateway {}",
            gateway_route_id(path, Some("/test")).unwrap_or("?")
        ),
        ("PUT", _) if backbone_link_name(path).is_some() => format!(
            "upsert backbone link {}",
            backbone_link_name(path).unwrap_or("?")
        ),
        ("DELETE", _) if backbone_link_name(path).is_some() => format!(
            "remove backbone link {}",
            backbone_link_name(path).unwrap_or("?")
        ),
        ("POST", "/api/backbone/config") => "update backbone config".to_string(),
        ("POST", "/api/backbone/token/rotate") => {
            "rotate backbone token (returned once)".to_string()
        }
        ("DELETE", _) if path.starts_with("/api/backbone/peers/") => format!(
            "kick backbone peer {}",
            &path["/api/backbone/peers/".len()..]
        ),
        ("POST", "/api/setup/quick") => format!(
            "quick setup (role={}, country={})",
            text("role").unwrap_or("client"),
            text("country").unwrap_or("?")
        ),
        ("POST", _) if path.starts_with("/api/messages/") && path.ends_with("/read") => {
            format!(
                "mark message {} read",
                &path["/api/messages/".len()..path.len() - "/read".len()]
            )
        }
        ("POST", _) if path.starts_with("/api/messages/") && path.ends_with("/retry") => {
            format!(
                "retry message {}",
                &path["/api/messages/".len()..path.len() - "/retry".len()]
            )
        }
        (method, path) => format!("{method} {path}"),
    }
}

fn internal_error(error: anyhow::Error) -> HttpResponse {
    tracing::warn!(%error, "request failed");
    HttpResponse::json(
        500,
        serde_json::json!({ "error": "internal", "message": error.to_string() }),
    )
}

async fn upload_attachment(request: &HttpRequest, state: &Arc<HttpState>) -> HttpResponse {
    if request.body.len() > MAX_ATTACHMENT_BYTES {
        return HttpResponse::error(413, "too_large");
    }
    let Some(content_type) = request.header("content-type") else {
        return HttpResponse::json(
            400,
            serde_json::json!({ "error": "bad_request", "message": "Content-Type required" }),
        );
    };
    let content_type = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if content_type.is_empty() {
        return HttpResponse::json(
            400,
            serde_json::json!({ "error": "bad_request", "message": "Content-Type required" }),
        );
    }
    let filename = request.header("x-openmesh-filename").map(|s| s.to_string());
    let duration_ms = request
        .header("x-openmesh-duration-ms")
        .and_then(|v| v.parse::<u32>().ok());
    match state
        .shared
        .attachments
        .put(
            &content_type,
            filename.as_deref(),
            duration_ms,
            &request.body,
        )
        .await
    {
        Ok(meta) => HttpResponse::json(
            200,
            serde_json::json!({ "id": meta.id, "size": meta.size, "sha256": meta.sha256 }),
        ),
        Err(e) => internal_error(e),
    }
}

async fn serve_static(request: &HttpRequest, state: &Arc<HttpState>) -> HttpResponse {
    let Some(root) = state.web_root.as_ref() else {
        if request.path == "/" {
            return HttpResponse::text(
                200,
                format!(
                    "OpenMesh bridge {} — API at /api/health. Build apps/web and restart with \
                     --serve-web apps/web/dist to serve the UI.\n",
                    openmesh_core::CRATE_VERSION
                ),
            );
        }
        return HttpResponse::error(404, "not_found");
    };
    let decoded = percent_decode(request.path.trim_start_matches('/'));
    let mut relative = PathBuf::new();
    for component in Path::new(&decoded).components() {
        match component {
            std::path::Component::Normal(part) => relative.push(part),
            std::path::Component::CurDir => {}
            _ => return HttpResponse::error(400, "bad_request"),
        }
    }
    let direct = root.join(&relative);
    let candidate = if direct.is_file() {
        Some(direct)
    } else {
        // SPA fallback: unknown routes serve index.html when it exists.
        let index = root.join("index.html");
        index.is_file().then_some(index)
    };
    let Some(path) = candidate else {
        return HttpResponse::error(404, "not_found");
    };
    match tokio::fs::read(&path).await {
        Ok(bytes) => HttpResponse::bytes(200, mime_for(&path), bytes),
        Err(e) => {
            tracing::debug!(path = %path.display(), error = %e, "static file read failed");
            HttpResponse::error(404, "not_found")
        }
    }
}

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "webmanifest" => "application/json",
        "map" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        "mp3" => "audio/mpeg",
        "ogg" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

async fn stream_sse(mut stream: TcpStream, state: Arc<HttpState>) -> std::io::Result<()> {
    let header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nX-Accel-Buffering: no\r\n\r\n";
    stream.write_all(header.as_bytes()).await?;
    let mut events = state.shared.events.subscribe();
    let mut shutdown = state.shutdown.clone();
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            event = events.recv() => match event {
                Ok(json) => {
                    if stream.write_all(format!("data: {json}\n\n").as_bytes()).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    }
    Ok(())
}

async fn write_response(stream: &mut TcpStream, response: HttpResponse) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\
         Access-Control-Allow-Origin: *\r\nX-Content-Type-Options: nosniff\r\n",
        response.status,
        reason_phrase(response.status),
        response.content_type,
        response.body.len()
    );
    for (name, value) in &response.extra_headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    if response.status != 204 && !response.body.is_empty() {
        stream.write_all(&response.body).await?;
    }
    stream.flush().await
}

/// Parsed HTTP request head, before the body is read.
struct RequestHead {
    consumed: usize,
    method: String,
    target: String,
    headers: HashMap<String, String>,
    content_length: usize,
}

/// Read one HTTP request (headers + body) from a connection.
async fn read_request(
    stream: &mut TcpStream,
    max_total: usize,
) -> Result<Option<HttpRequest>, ReadError> {
    let mut buf: Vec<u8> = Vec::with_capacity(8192);
    let mut read_buf = [0u8; 8192];
    let mut head: Option<RequestHead> = None;

    loop {
        if head.is_none() {
            let mut header_objs = [httparse::EMPTY_HEADER; 64];
            let mut parsed = httparse::Request::new(&mut header_objs);
            match parsed.parse(&buf) {
                Ok(httparse::Status::Complete(consumed)) => {
                    let method = parsed.method.unwrap_or("GET").to_string();
                    let target = parsed.path.unwrap_or("/").to_string();
                    let mut headers = HashMap::new();
                    for header in parsed.headers {
                        headers.insert(
                            header.name.to_ascii_lowercase(),
                            String::from_utf8_lossy(header.value).trim().to_string(),
                        );
                    }
                    let content_length = headers
                        .get("content-length")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(0);
                    if content_length > max_total || consumed + content_length > max_total {
                        return Err(ReadError::TooLarge);
                    }
                    head = Some(RequestHead {
                        consumed,
                        method,
                        target,
                        headers,
                        content_length,
                    });
                }
                Ok(httparse::Status::Partial) => {
                    if buf.len() > MAX_HEADER_BYTES {
                        return Err(ReadError::TooLarge);
                    }
                }
                Err(_) => return Err(ReadError::Malformed("invalid HTTP request".into())),
            }
        }
        if let Some(head) = &head {
            if buf.len() >= head.consumed + head.content_length {
                break;
            }
        }
        let read = stream
            .read(&mut read_buf)
            .await
            .map_err(|_| ReadError::Io)?;
        if read == 0 {
            return if buf.is_empty() {
                Ok(None)
            } else {
                Err(ReadError::Malformed("truncated request".into()))
            };
        }
        buf.extend_from_slice(&read_buf[..read]);
    }

    let RequestHead {
        consumed,
        method,
        target,
        headers,
        content_length,
    } = head.ok_or_else(|| ReadError::Malformed("no request head".into()))?;
    let body = buf[consumed..consumed + content_length].to_vec();
    let (raw_path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_string(), query.to_string()),
        None => (target, String::new()),
    };
    let path = percent_decode(&raw_path);
    Ok(Some(HttpRequest {
        method,
        path,
        query: parse_query(&query),
        headers,
        body,
    }))
}

fn parse_query(query: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        out.insert(percent_decode(key), percent_decode(value));
    }
    out
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Resolve the web root: explicit flag, else `apps/web/dist` when the workspace is present.
pub fn resolve_web_root(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(root) = explicit {
        return root.is_dir().then_some(root);
    }
    for candidate in ["apps/web/dist", "../apps/web/dist", "../../apps/web/dist"] {
        let path = PathBuf::from(candidate);
        if path.join("index.html").is_file() {
            return Some(path);
        }
    }
    None
}

/// Build the small JSON response used when a client probes an unknown API path.
pub fn api_not_found() -> HttpResponse {
    HttpResponse::json(404, serde_json::json!({ "error": "not_found" }))
}

/// Utility used by tests and the SSE layer to create a status event envelope.
pub fn static_event(typ: &str, data: serde_json::Value) -> String {
    event_json(typ, Some(data))
}

/// Utility used by tests: base64 encoding of an identity as used in contact records.
pub fn encode_identity(ident: &openmesh_core::PublicIdentity) -> String {
    b64(&ident.to_bytes())
}

/// Utility used by tests: parse a node id from hex.
pub fn parse_id(hex: &str) -> Option<openmesh_core::NodeId> {
    parse_node_id(hex)
}

/// Utility used by tests and diagnostics: attach a DB path to a store.
pub fn store_path(store: &Store) -> PathBuf {
    store.path().to_path_buf()
}

/// Utility used by tests: produce a JSON error envelope.
pub fn error_envelope(id: Option<&str>, code: &str, message: &str) -> String {
    error_json(id, code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parsing_and_percent_decoding() {
        let q = parse_query("peer=aabb&limit=50&name=hello%20world&token=abc");
        assert_eq!(q.get("peer").map(|s| s.as_str()), Some("aabb"));
        assert_eq!(q.get("limit").map(|s| s.as_str()), Some("50"));
        assert_eq!(q.get("name").map(|s| s.as_str()), Some("hello world"));
    }

    #[test]
    fn percent_decode_handles_utf8() {
        assert_eq!(percent_decode("a%2Fb%3Dc"), "a/b=c");
        assert_eq!(percent_decode("caf%C3%A9"), "café");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn constant_time_eq_matches_semantics() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }

    #[test]
    fn mime_types_are_assigned() {
        assert_eq!(
            mime_for(Path::new("index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            mime_for(Path::new("app.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(mime_for(Path::new("x.unknown")), "application/octet-stream");
    }

    #[tokio::test]
    async fn parses_http_get_with_query() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            stream
                .write_all(b"GET /api/status?x=1 HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await
                .unwrap();
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).await.unwrap();
            buf
        });
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket, 1024 * 1024)
            .await
            .ok()
            .flatten()
            .unwrap();
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/status");
        assert_eq!(request.query.get("x").map(|s| s.as_str()), Some("1"));
        drop(socket);
        let response = client.await.unwrap();
        assert!(response.is_empty());
    }

    #[tokio::test]
    async fn parses_post_body() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            stream
                .write_all(
                    b"POST /api/attachments HTTP/1.1\r\nHost: x\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello",
                )
                .await
                .unwrap();
        });
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket, 1024 * 1024)
            .await
            .ok()
            .flatten()
            .unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.body, b"hello");
        assert_eq!(request.header("content-type"), Some("text/plain"));
    }

    #[test]
    fn web_root_resolution_handles_missing() {
        assert!(resolve_web_root(Some(PathBuf::from("/definitely/missing"))).is_none());
    }
}
