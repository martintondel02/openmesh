//! Optional TCP transport for WAN bridges: 2-byte big-endian length prefix per frame.
//!
//! The bridge listens on an explicit address (`--tcp-listen`) and fans every outbound frame to
//! all connected peers. Inbound frames are tagged with the link and their source address.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, mpsc, watch};

use openmesh_core::{LinkId, LinkInfo};

use super::{INBOUND_CHANNEL_CAPACITY, Inbound, TransportFrame, TransportHandle, make_link_stats};

/// Maximum accepted frame length on a length-prefixed link.
const MAX_TCP_FRAME: usize = 4096;

type PeerWriters = Arc<Mutex<HashMap<SocketAddr, mpsc::Sender<Vec<u8>>>>>;

/// Start the TCP listener transport task.
pub fn spawn(
    listen: SocketAddr,
    link: LinkId,
    inbound: mpsc::Sender<Inbound>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<TransportHandle> {
    let (tx, mut outbound) = mpsc::channel::<TransportFrame>(INBOUND_CHANNEL_CAPACITY);
    let stats = make_link_stats();
    let peers: PeerWriters = Arc::new(Mutex::new(HashMap::new()));

    // Bind synchronously so configuration errors surface before the daemon claims to be up.
    let listener = std::net::TcpListener::bind(listen)
        .with_context(|| format!("binding TCP listener {listen}"))?;
    listener.set_nonblocking(true)?;

    let stats_task = stats.clone();
    let peers_task = peers.clone();
    let link_id = link;
    tokio::spawn(async move {
        let listener = match TcpListener::from_std(listener) {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(error = %e, "failed to adopt TCP listener");
                return;
            }
        };
        const SHUTDOWN_TICK_MS: u64 = 1_000;
        loop {
            if *shutdown.borrow() {
                break;
            }
            tokio::select! {
                _ = shutdown.changed() => break,
                accepted = listener.accept() => {
                    let Ok((stream, addr)) = accepted else { continue };
                    tracing::info!(%addr, "tcp peer connected");
                    let (write_tx, mut write_rx) = mpsc::channel::<Vec<u8>>(INBOUND_CHANNEL_CAPACITY);
                    peers_task.lock().await.insert(addr, write_tx);
                    let inbound = inbound.clone();
                    let stats_conn = stats_task.clone();
                    let peers_conn = peers_task.clone();
                    tokio::spawn(async move {
                        let (mut read_half, mut write_half) = stream.into_split();
                        let writer = tokio::spawn(async move {
                            while let Some(bytes) = write_rx.recv().await {
                                let len = bytes.len() as u16;
                                if write_half.write_all(&len.to_be_bytes()).await.is_err() {
                                    break;
                                }
                                if write_half.write_all(&bytes).await.is_err() {
                                    break;
                                }
                            }
                        });
                        let mut len_buf = [0u8; 2];
                        loop {
                            if read_half.read_exact(&mut len_buf).await.is_err() {
                                break;
                            }
                            let len = u16::from_be_bytes(len_buf) as usize;
                            if len == 0 || len > MAX_TCP_FRAME {
                                tracing::warn!(%addr, len, "tcp frame length out of range, closing");
                                break;
                            }
                            let mut frame = vec![0u8; len];
                            if read_half.read_exact(&mut frame).await.is_err() {
                                break;
                            }
                            stats_conn.record_rx(None, None);
                            let msg = Inbound {
                                bytes: frame,
                                link: Some(LinkInfo { link: link_id, peer: None, rssi: None, snr: None }),
                            };
                            if inbound.send(msg).await.is_err() {
                                break;
                            }
                        }
                        writer.abort();
                        peers_conn.lock().await.remove(&addr);
                        tracing::info!(%addr, "tcp peer disconnected");
                    });
                }
                frame = outbound.recv() => {
                    let Some(frame) = frame else { break };
                    if frame.link.is_some_and(|preferred| preferred != link) {
                        continue;
                    }
                    stats_task.record_tx();
                    let peers = peers_task.lock().await;
                    for writer in peers.values() {
                        let _ = writer.send(frame.bytes.clone()).await;
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(SHUTDOWN_TICK_MS)) => {}
            }
        }
        peers_task.lock().await.clear();
        tracing::debug!("tcp transport stopped");
    });
    Ok(TransportHandle {
        id: link,
        kind: "tcp",
        label: format!("TCP {listen}"),
        tx,
        stats,
    })
}
