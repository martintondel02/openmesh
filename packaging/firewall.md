# Firewall notes for OpenMesh backbone mode

Default ports:

| Port | Protocol | Scope | Purpose |
| --- | --- | --- | --- |
| 9443 | TCP | **public / peer networks** | OBB/1 backbone transport (authenticated; wrap in TLS/WireGuard on hostile networks) |
| 8787 | TCP | **loopback only** | bridge REST/WS API and control panel |
| 4403 | UDP | LAN only (optional) | bridge LAN mesh transport (`--udp-port`, disable with `--no-udp`) |
| 22/80/443 | TCP | admin | SSH and your panel reverse proxy (Caddy/nginx), if used |

Never expose `8787` directly to the internet. The bridge API token is a bearer secret; put the
panel behind a TLS reverse proxy with authentication
(`packaging/examples/Caddyfile.panel`, `packaging/examples/nginx-panel.conf`) or reach it over
WireGuard/Tailscale/SSH tunnel.

## firewalld (Fedora/RHEL/CentOS)

```bash
sudo firewall-cmd --permanent --add-port=9443/tcp
sudo firewall-cmd --reload
sudo firewall-cmd --list-ports
```

Restrict peers to a VPN interface or source range if you do not want a public port:

```bash
sudo firewall-cmd --permanent --new-zone=openmesh-peers
sudo firewall-cmd --permanent --zone=openmesh-peers --add-source=10.44.0.0/24
sudo firewall-cmd --permanent --zone=openmesh-peers --add-port=9443/tcp
sudo firewall-cmd --reload
```

## ufw (Debian/Ubuntu)

```bash
sudo ufw allow 9443/tcp
sudo ufw status verbose
# Panel is loopback-only, so nothing to open for 8787. If you use a TLS reverse proxy:
sudo ufw allow 443/tcp
```

## nftables

```nft
table inet openmesh {
  chain input {
    type filter hook input priority 0; policy drop;
    ct state established,related accept
    iif lo accept
    tcp dport 22 accept comment "admin"
    tcp dport 9443 accept comment "OpenMesh OBB/1"
    tcp dport 443 accept comment "panel reverse proxy"
    udp dport 4403 accept comment "OpenMesh LAN (optional; restrict to LAN interfaces)"
  }
}
```

## Docker caveats

* Docker publishes ports in its own `DOCKER-USER`/nat chains **before** ufw/firewalld input
  rules; `ufw deny 9443` does not necessarily block a published container port. Use
  `network_mode: host` (recommended, see `packaging/docker-compose.yml`) or restrict the
  publish address (`127.0.0.1:8787:8787`) and manage `DOCKER-USER` rules.
* With host networking the container binds the host's ports directly; the host firewall applies
  normally. Keep the panel bound to `127.0.0.1` in that case.
* Cloud hosts (Hetzner/AWS/GCP/Oracle) have a second firewall/security-group layer; open 9443
  there too, or only the WireGuard port if you use a VPN.

## Verifying reachability

```bash
# From another host: TCP connect should succeed, then the OBB/1 handshake does the auth.
nc -vz mesh.example.org 9443

# On the backbone host: API health is public (loopback only by default).
curl -fsS http://127.0.0.1:8787/api/health
curl -fsS -H "Authorization: Bearer $(cat ~/.config/openmesh/bridge-token)" \
     http://127.0.0.1:8787/api/backbone | head
```
