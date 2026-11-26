//! Backbone outbound link loop (`openmesh-bridge join <openmesh://backbone?...>` and the
//! `backbone-links.json` multi-link list).
//!
//! Keeps one persistent authenticated TCP link per configured backbone and reconnects with
//! exponential backoff (plus jitter) whenever the link drops. Several links may run at once; the
//! engine's `(src, seq)` dedup suppresses frames that arrive over more than one path.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::watch;

use super::wire::{self, ClientHandshakeParams};
use super::{BackboneManager, ClientLink, PeerDirection, WireError};

/// Run one outbound link's reconnect loop until the link stop flag or bridge shutdown fires.
pub async fn client_loop(
    manager: Arc<BackboneManager>,
    link: Arc<ClientLink>,
    mut shutdown: watch::Receiver<bool>,
) {
    let join = &link.target;
    let mut backoff = join.reconnect_min_ms;
    let address = join.connection.authority();
    loop {
        if link.stop.load(Ordering::Relaxed) || *shutdown.borrow() {
            break;
        }
        link.begin_attempt();
        let connect =
            tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(&address)).await;
        match connect {
            Ok(Ok(mut stream)) => {
                let params = ClientHandshakeParams {
                    node_id: manager.own_node_id(),
                    app: &join.app,
                    caps: 0,
                    token: join.token_key,
                    timeout: Duration::from_secs(10),
                };
                match wire::client_handshake(&mut stream, &params).await {
                    Ok(welcome) => {
                        backoff = join.reconnect_min_ms;
                        link.mark_connected();
                        let remote = stream
                            .peer_addr()
                            .map(|addr| addr.to_string())
                            .unwrap_or_else(|_| address.clone());
                        let auth = if join.connection.token.is_some() {
                            "token"
                        } else {
                            "open"
                        };
                        let (peer, outbound_rx) = manager
                            .make_peer(
                                welcome.node_id,
                                welcome.name.clone(),
                                PeerDirection::Outbound,
                                Some(link.name.clone()),
                                remote,
                                auth.to_string(),
                                "backbone-server".to_string(),
                            )
                            .await;
                        manager.publish_peer(peer.clone()).await;
                        tracing::info!(
                            backbone = %welcome.node_id,
                            name = %welcome.name,
                            link = %link.name,
                            "backbone outbound link established"
                        );
                        let (read_half, write_half) = stream.into_split();
                        let shutdown = manager.shutdown_receiver();
                        super::run_session(
                            manager.clone(),
                            peer,
                            read_half,
                            write_half,
                            outbound_rx,
                            welcome.ping_interval_s,
                            shutdown,
                        )
                        .await;
                        link.mark_disconnected();
                    }
                    Err(WireError::AuthFailed(reason)) => {
                        link.note_error(format!("backbone auth failed: {reason}"));
                        tracing::warn!(
                            link = %link.name,
                            reason,
                            "backbone server rejected our token"
                        );
                    }
                    Err(error) => {
                        link.note_error(format!("backbone handshake failed: {error}"));
                    }
                }
            }
            Ok(Err(error)) => {
                link.note_error(format!("cannot reach backbone {address}: {error}"));
            }
            Err(_) => {
                link.note_error(format!("cannot reach backbone {address}: timeout"));
            }
        }
        if link.stop.load(Ordering::Relaxed) || *shutdown.borrow() {
            break;
        }
        let jitter = rand::random::<u64>() % (backoff / 4 + 1);
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tokio::time::sleep(Duration::from_millis(backoff + jitter)) => {}
        }
        backoff = backoff
            .saturating_mul(2)
            .min(join.reconnect_max_ms.max(backoff));
    }
    link.mark_disconnected();
    manager.on_link_loop_exit(&link).await;
    tracing::debug!(link = %link.name, "backbone outbound link loop stopped");
}
