//! UDP LAN transport: one datagram per OMP frame, broadcast to port 4403 by default.
//!
//! Every bridge on the LAN both broadcasts its own frames and listens for everyone else's, so
//! a set of bridges behaves like a set of radios sharing one channel. Datagrams that came from
//! our own node id are dropped: OMP dedup would catch them anyway, but not ingesting our own
//! echoes keeps counters honest.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};

use openmesh_core::{LinkId, LinkInfo, NodeId};

use super::{INBOUND_CHANNEL_CAPACITY, Inbound, TransportFrame, TransportHandle, make_link_stats};

/// Default UDP port used for LAN discovery and frame broadcast (`PROTOCOL.md` §14).
pub const DEFAULT_UDP_PORT: u16 = 4403;

#[derive(Clone, Debug)]
pub struct UdpConfig {
    pub port: u16,
}

impl Default for UdpConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_UDP_PORT,
        }
    }
}

fn bind_socket(port: u16) -> Result<std::net::UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .context("creating UDP socket")?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    {
        // Allows several bridges on one machine to share the LAN port during development.
        socket.set_reuse_port(true)?;
    }
    socket.set_broadcast(true)?;
    socket.set_nonblocking(true)?;
    let addr: SocketAddr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
    socket
        .bind(&addr.into())
        .with_context(|| format!("binding UDP port {port}"))?;
    Ok(socket.into())
}

/// Start the UDP transport task.
pub fn spawn(
    config: UdpConfig,
    node_id: NodeId,
    link: LinkId,
    inbound: mpsc::Sender<Inbound>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<TransportHandle> {
    let std_socket = bind_socket(config.port)?;
    let socket = Arc::new(UdpSocket::from_std(std_socket)?);
    let (tx, mut outbound) = mpsc::channel::<TransportFrame>(INBOUND_CHANNEL_CAPACITY);
    let stats = make_link_stats();
    let task_stats = stats.clone();
    let port = config.port;
    let warn_once = Arc::new(AtomicBool::new(false));
    tokio::spawn(async move {
        let mut buf = vec![0u8; 512];
        let broadcast_targets = [
            SocketAddr::from((Ipv4Addr::BROADCAST, port)),
            SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        ];
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                received = socket.recv_from(&mut buf) => {
                    let Ok((len, _from)) = received else { continue };
                    if len < openmesh_core::HEADER_LEN {
                        continue;
                    }
                    // Drop our own broadcasts (src is bytes 2..10 of the frame header).
                    if buf[2..10] == node_id.0 {
                        continue;
                    }
                    task_stats.record_rx(None, None);
                    let frame = Inbound {
                        bytes: buf[..len].to_vec(),
                        link: Some(LinkInfo { link, peer: None, rssi: None, snr: None }),
                    };
                    if inbound.send(frame).await.is_err() {
                        break;
                    }
                }
                frame = outbound.recv() => {
                    let Some(frame) = frame else { break };
                    if frame.link.is_some_and(|preferred| preferred != link) {
                        continue;
                    }
                    task_stats.record_tx();
                    for target in &broadcast_targets {
                        if let Err(e) = socket.send_to(&frame.bytes, target).await {
                            if !warn_once.swap(true, Ordering::Relaxed) {
                                tracing::debug!(error = %e, "UDP send failed (broadcast may be blocked)");
                            }
                        }
                    }
                }
            }
        }
        tracing::debug!("udp transport stopped");
    });
    Ok(TransportHandle {
        id: link,
        kind: "udp",
        label: format!("UDP LAN :{port}"),
        tx,
        stats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use openmesh_core::{Frame, FrameType, NodeId};

    fn frame_bytes(src: NodeId, dst: NodeId, seq: u32) -> Vec<u8> {
        let mut frame = Frame::new(FrameType::Data, src, dst, seq, vec![1, 2, 3]).unwrap();
        frame.ttl = 5;
        frame.flags.insert(openmesh_core::Flags::ENCRYPTED);
        frame.encode().unwrap()
    }

    #[tokio::test]
    async fn bound_socket_receives_datagrams() {
        let receiver = UdpSocket::from_std(bind_socket(0).unwrap()).unwrap();
        let addr = receiver.local_addr().unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let bytes = frame_bytes(NodeId::new([1; 8]), NodeId::new([2; 8]), 1);
        sender.send_to(&bytes, addr).await.unwrap();
        let mut buf = [0u8; 512];
        let (len, _) = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            receiver.recv_from(&mut buf),
        )
        .await
        .expect("datagram arrives")
        .expect("socket read succeeds");
        assert_eq!(&buf[..len], bytes.as_slice());
    }

    #[tokio::test]
    async fn spawn_returns_transport_and_stops_cleanly() {
        let (inbound_tx, _inbound_rx) = mpsc::channel(8);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let handle = spawn(
            UdpConfig { port: 0 },
            NodeId::new([1; 8]),
            1,
            inbound_tx,
            shutdown_rx,
        )
        .unwrap();
        assert_eq!(handle.kind, "udp");
        assert!(handle.label.starts_with("UDP LAN"));
        // A frame with a different preferred link is dropped by this transport, not queued.
        handle.send(&TransportFrame {
            bytes: frame_bytes(NodeId::new([1; 8]), NodeId::new([2; 8]), 1),
            mode: openmesh_core::SendMode::Flood,
            link: Some(99),
        });
        shutdown_tx.send(true).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}
