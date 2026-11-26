//! Backbone server listener: accept loop, per-IP rate limiting, OBB/1 handshake, sessions.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use super::wire::{self, ServerHandshakeParams};
use super::{BackboneManager, PeerDirection, WireError};

/// Accept inbound backbone connections until the listener generation is stopped.
pub async fn accept_loop(
    manager: Arc<BackboneManager>,
    listener: TcpListener,
    mut stop: watch::Receiver<bool>,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        let accepted = tokio::select! {
            _ = stop.changed() => break,
            _ = shutdown.changed() => break,
            accepted = listener.accept() => accepted,
        };
        let Ok((stream, addr)) = accepted else {
            continue;
        };
        if !manager.allow_handshake_from(addr.ip()).await {
            tracing::debug!(%addr, "backbone handshake rate limit exceeded; dropping connection");
            drop(stream);
            continue;
        }
        let Some(guard) = manager.begin_handshake() else {
            tracing::debug!(%addr, "too many pending backbone handshakes; dropping connection");
            drop(stream);
            continue;
        };
        let manager = manager.clone();
        tokio::spawn(async move {
            let _guard = guard;
            handle_incoming(manager, stream, addr).await;
        });
    }
    tracing::debug!("backbone accept loop stopped");
}

async fn handle_incoming(manager: Arc<BackboneManager>, mut stream: TcpStream, addr: SocketAddr) {
    let config = manager.handshake_config().await;
    let peers_connected = manager.peer_total().await;
    let session = manager.new_session_id().await;
    let params = ServerHandshakeParams {
        node_id: config.node_id,
        server_name: &config.name,
        auth: config.auth,
        token: config.token,
        allowlist: &config.allowlist,
        max_peers: config.max_peers,
        peers_connected,
        session,
        timeout: Duration::from_secs(10),
    };
    let info = match wire::server_handshake(&mut stream, &params).await {
        Ok(info) => info,
        Err(WireError::AuthFailed(reason)) => {
            manager.note_auth_failure(addr, &reason).await;
            tracing::info!(%addr, reason, "backbone handshake rejected");
            return;
        }
        Err(error) => {
            manager
                .note_handshake_error(addr, &format!("{error}"))
                .await;
            tracing::debug!(%addr, error = %error, "backbone handshake failed");
            return;
        }
    };

    // Re-check capacity after the handshake (concurrent handshakes may have filled the table).
    if manager.peer_total().await >= config.max_peers {
        let _ = wire::write_control(
            &mut stream,
            &wire::ControlMsg::Close {
                reason: "max_peers".into(),
            },
        )
        .await;
        manager.note_auth_failure(addr, "max_peers").await;
        return;
    }

    let remote_addr = addr.to_string();
    let app = info.app.clone();
    let (peer, outbound_rx) = manager
        .make_peer(
            info.node_id,
            app.clone(),
            PeerDirection::Inbound,
            None,
            remote_addr,
            info.auth.to_string(),
            app,
        )
        .await;
    manager.publish_peer(peer.clone()).await;
    tracing::info!(%addr, node = %info.node_id, "backbone peer connected");
    let (read_half, write_half) = stream.into_split();
    let shutdown = manager.shutdown_receiver();
    super::run_session(
        manager,
        peer,
        read_half,
        write_half,
        outbound_rx,
        config.ping_interval_s,
        shutdown,
    )
    .await;
}
