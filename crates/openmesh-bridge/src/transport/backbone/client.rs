//! Backbone outbound client (`openmesh-bridge join <openmesh://backbone?...>`).
//!
//! Keeps a persistent authenticated TCP link to a backbone server and reconnects with
//! exponential backoff (plus jitter) whenever the link drops.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::watch;

use super::wire::{self, ClientHandshakeParams};
use super::{BackboneManager, JoinTarget, PeerDirection, WireError};

/// Run the outbound link loop until `stop` or the bridge shutdown fires.
pub async fn client_loop(
    manager: Arc<BackboneManager>,
    join: JoinTarget,
    stop: Arc<std::sync::atomic::AtomicBool>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut backoff = join.reconnect_min_ms;
    let address = join.connection.authority();
    loop {
        if stop.load(Ordering::Relaxed) || *shutdown.borrow() {
            break;
        }
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
                        manager.clear_last_error().await;
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
                                remote,
                                auth.to_string(),
                                "backbone-server".to_string(),
                            )
                            .await;
                        manager.publish_peer(peer.clone()).await;
                        tracing::info!(
                            backbone = %welcome.node_id,
                            name = %welcome.name,
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
                    }
                    Err(WireError::AuthFailed(reason)) => {
                        manager
                            .note_outbound_failure(format!("backbone auth failed: {reason}"))
                            .await;
                        tracing::warn!(reason, "backbone server rejected our token");
                    }
                    Err(error) => {
                        manager
                            .note_outbound_failure(format!("backbone handshake failed: {error}"))
                            .await;
                    }
                }
            }
            Ok(Err(error)) => {
                manager
                    .note_outbound_failure(format!("cannot reach backbone {address}: {error}"))
                    .await;
            }
            Err(_) => {
                manager
                    .note_outbound_failure(format!("cannot reach backbone {address}: timeout"))
                    .await;
            }
        }
        if stop.load(Ordering::Relaxed) || *shutdown.borrow() {
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
    manager.on_client_loop_exit().await;
    tracing::debug!("backbone outbound link loop stopped");
}
