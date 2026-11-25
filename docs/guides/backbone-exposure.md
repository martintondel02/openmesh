# Exposing your backbone to the internet (safely)

**Audience:** anyone running a backbone that gateways outside their LAN should reach.
**TL;DR:** prefer a private network (WireGuard/Tailscale). If you can't, use an outbound-only tunnel
such as **Openflare**; open a firewall port only as a last resort, and never expose the panel.

---

## 1. What has to be reachable

| Port | What | Expose publicly? |
| --- | --- | --- |
| `9443/tcp` (configurable) | OBB/1 backbone link — gateways connect here | **yes**, this is the point |
| `8787/tcp` | bridge API + web panel (backbone controls) | **no** — keep it loopback/VPN only |
| `4403/udp` | LAN discovery | no (LAN only) |
| radio serial/BLE | local hardware | no |

Meshes are end-to-end encrypted: exposing the backbone lets peers relay ciphertext, not read it.
The backbone token is what stops strangers from injecting traffic, so treat exposure and token
rotation together.

---

## 2. Decision guide

1. **Do all gateways already share a private network** (WireGuard, Tailscale, Headscale,
   Nebula, an existing VPN)? → Bind the backbone to the VPN address and stop here. This is the
   best security/effort ratio.
2. **Behind NAT/CGNAT, no port forwarding, or you don't want inbound ports?** → Use an
   **outbound-only tunnel**. Recommended: **Openflare** (self-hosted tunnel + reverse proxy,
   outbound-only connectors, one public edge; <https://github.com/martintondel02/openflare> —
   public launch in progress; see §4 for the generic recipe and §5 for alternatives).
3. **You own the firewall + DNS and want a stable public address?** → Port-forward `9443/tcp` and
   terminate TLS in front with Caddy/nginx (`packaging/examples/nginx-stream-tls.conf`,
   `packaging/examples/Caddyfile.panel`) or stunnel. Keep the token.
4. **Temporary/testing only** → `ssh -L 9443:127.0.0.1:9443 user@backbone` from the gateway host,
   or a WireGuard link.

Never expose: the bridge API/panel, the database, or an `auth: "open"` backbone.

---

## 3. Recommended: Openflare (outbound-only, no open port)

Openflare is the companion exposure path for OpenMesh backbones: a self-hosted tunnel, reverse
proxy and control plane with outbound-only connectors, one public edge, certificates, traffic
paths and status pages.

```
gateway ──TCP──► openflare public edge ◄──outbound── openflare connector ──► 127.0.0.1:9443
                                                                              (openmesh backbone)
```

1. **Bind the backbone to loopback** so only the connector can reach it:

   ```bash
   openmesh-bridge backbone --bind 127.0.0.1 --port 9443 --public-host mesh.example.org
   ```

   (Use `localhost`/your tunnel DNS name for `--public-host` later; the panel shows the resulting
   `openmesh://backbone?…` string and QR.)

2. **Start an Openflare connector** on the same machine (or a container beside it), following the
   Openflare docs. Create a **TCP service / traffic path** whose origin is
   `http://127.0.0.1:9443` (or `tcp://127.0.0.1:9443` depending on the connector), and assign a
   public hostname or TCP edge port. Openflare's edge handles the public side and certificates;
   no inbound firewall port is needed.

3. **Tell the backbone its public name** so invites are correct: set `public_host` in the backbone
   panel (or restart with `--public-host`). The setup wizard / `--print-setup` now includes this
   step under "Expose it publicly".

4. **Open the firewall** for outbound connections only. On Fedora with firewalld the default
   outbound policy is fine; do not add an inbound rule for 9443 in this scenario.

5. **Verify from the remote side**:

   ```bash
   openmesh-bridge join 'openmesh://backbone?host=mesh.example.org&port=9443&token=…&node=…'
   ```

   The gateway should appear in the panel's peer table within a few seconds; `auth_failures`
   should stay at zero. If it doesn't, check the connector's traffic-path status and that the
   backbone token in the invite is current.

6. **Harden**: token auth (never open), allowlist known peer node ids, rotate the token if it was
   ever pasted anywhere public, and keep the panel loopback-only. Watch the activity log for
   unexpected peers.

---

## 4. Generic outbound-only tunnel recipe

Any tool that can publish a TCP service from an outbound connection works (Cloudflare Tunnel,
frp, rathole, ngrok/ssh -R for testing). The pattern:

1. Backbone: `--bind 127.0.0.1 --port 9443 --public-host <public-hostname>`.
2. Tunnel: publish `tcp://127.0.0.1:9443` (or an HTTP/2 stream for tunnelled TCP) and note the
   public hostname/port.
3. Backbone: set `public_host` to that hostname and share the generated connection string/QR.
4. Firewall: no inbound rule needed; outbound DNS/TLS to the tunnel edge only.
5. Verify: remote `join` + panel peer entry + zero auth failures.

Caveat: some HTTP-only tunnels cannot carry raw TCP. OBB/1 needs a byte stream; the tunnel must
support TCP (or raw byte streaming), not just HTTP requests. Openflare supports TCP traffic paths.

---

## 5. Alternatives

* **WireGuard** — put the backbone and gateways in one overlay network; bind the backbone to the
  `wg0` address. Example: `packaging/examples/wireguard-backbone.conf`.
* **Tailscale/Headscale** — zero-config WireGuard; bind to the tailnet IP or run a subnet router.
* **Caddy/nginx stream TLS** — public port-forward with TLS termination:
  `packaging/examples/nginx-stream-tls.conf`, `packaging/examples/stunnel-client.conf`.
  Terminating TLS protects transport metadata in transit; the token still authenticates peers.
* **SSH tunnel** — fine for a handful of test gateways, not for production.

---

## 6. Security checklist

- [ ] Only the OBB/1 port is exposed; panel/API stays `127.0.0.1` (or VPN).
- [ ] `auth: token`; token was never committed, posted in a screenshot, or shared in a group chat.
- [ ] Token rotated after any exposure change or suspected leak (panel → Backbone → Rotate token,
      then kick peers).
- [ ] `max_peers` and (when small) an allowlist configured.
- [ ] Backbone host patched and rebooted with the service; config file mode `0600`.
- [ ] `auth_failures` monitored; alerting wired to your monitoring if available.
- [ ] E2E assumption intact: users verify contacts out-of-band; the tunnel operator is not trusted
      with plaintext.
- [ ] Backup of `<data-dir>/backbone.json` (token) stored in a secret manager, not in git.

---

## 7. Troubleshooting

| Symptom | Check |
| --- | --- |
| Gateway cannot connect at all | Tunnel/edge status, `ss -ltnp \| grep 9443`, backbone `running: true`, firewall outbound allowed |
| Handshake `auth_failed.bad_token` | Invite contains the current token; rotate + re-share if stale |
| `auth_failed.not_allowlisted` | Peer bridge node id is in the allowlist |
| `auth_failed.max_peers` | Raise `max_peers` or disconnect unused peers |
| Connects then drops every ~90 s | Idle timeout + keepalive: peer must respect `ping_interval_s`; check tunnel idle timeouts |
| Works from LAN, not from internet | Backbone bound to `127.0.0.1` but tunnel still pointing elsewhere, or `public_host` mismatch in invite |
| Panel unreachable after exposing | Expected: panel should not be exposed. Reach it over SSH/VPN or `--bind 127.0.0.1` locally |
| Slow/throughput limited | Tunnel bandwidth/keepalive; OBB/1 frames are tiny but images need a few kbit/s sustained |
