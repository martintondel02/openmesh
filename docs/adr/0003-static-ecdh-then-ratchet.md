# ADR 0003 — Static ECDH for v1, ratchet for OMP/2

* **Status:** accepted (v0.1.0); forward secrecy scheduled for OMP/2
* **Date:** 2026-11-25
* **Decision record:** summary row 3 in
  [ARCHITECTURE.md §4](../architecture/ARCHITECTURE.md#4-key-decisions)
* **Related:** [ADR 0001](0001-own-protocol-vs-reticulum.md),
  [PROTOCOL.md §3/§5/§17](../architecture/PROTOCOL.md),
  [Reticulum notes](../research/reticulum-lxmf-notes.md) §2.4/§2.6

## 1. Context

OpenMesh's security model needs end-to-end encryption between peers, with relays seeing only
ciphertext and metadata. Two broad designs:

1. **Static key agreement:** each node has a long-term X25519 key, advertised in its announce.
   A direct message key is `HKDF(X25519(my_sk, peer_pk), salt, info)`; no handshake; each message
   is one AEAD-sealed frame. Encryption is immediate, works while offline (store-and-forward),
   and has zero setup cost. **No forward secrecy:** if a long-term private key leaks, all past
   messages to that peer become decryptable.
2. **Session establishment (handshake):** X3DH/double-ratchet (Signal), or Reticulum-style links
   with ephemeral ECDH. Provides forward secrecy and post-compromise security, but requires
   round trips and state, and each round trip costs airtime and delivery latency.

Constraints specific to OpenMesh v0.1:

* One-frame messages are the whole point; a handshake is 2–5 extra frames plus path/link
  discovery on a 240-byte LoRa medium.
* Store-and-forward must work while the receiver is offline for days. Asynchronous session
  establishment (prekeys) is the standard answer, but it is a substantial protocol and client
  UX surface (prekey bundles, one-time key exhaustion, multi-device).
* The store-and-forward design is explicitly "relays cannot read"; it says nothing about
  forward secrecy, and PROTOCOL §17 lists the absence as a known limitation.
* Voice calls (OMP/2) need sessions and forward secrecy anyway; but calls are not in v1.

### 1.1 What the alternatives cost, measured

Using the frame/airtime model in [lora-capacity-and-media.md](../research/lora-capacity-and-media.md):

| Approach | Extra frames per conversation start | Extra airtime at `MEDIUM_FAST` |
| --- | --- | --- |
| Static ECDH (chosen) | 0 | 0 |
| Signal-style X3DH with prekeys | 2–3 frames (bundle fetch + handshake) + prekey publication in announce | 1.5–2.5 s, one-time per session/peer |
| Reticulum-style link (ephemeral ECDH) | link request 83 B + proof 115 B + RTT 99 B (+ path discovery) | ~0.9–2.3 s before data, plus per-hop relay cost |
| Full double ratchet with per-message header keys | small header overhead | fine, but complex state |

On a 1 %-duty channel, 2 s of handshake airtime is comparable to four text messages. For a
network whose primary use is text, that is a real cost. For calls, links/sessions are
unavoidable.

## 2. Decision

**OMP/1 uses static X25519 ECDH derived keys and XChaCha20-Poly1305 for direct messages.
Forward secrecy is explicitly out of scope for v0.1 and scheduled for OMP/2 as a ratchet
mechanism.**

### 2.1 Key schedule (v1, normative in PROTOCOL.md §5.1)

```
seed(32) → Ed25519 signing key (SigningKey::from_bytes(seed))
         → X25519 static secret = HKDF-SHA256(salt="openmesh/v1/dh", ikm=seed,
                                              info="static-x25519")
node_id  = SHA-256("openmesh/v1/node" || sign_pk || dh_pk)[0..8] with high bit cleared
shared   = X25519(my_dh_sk, peer_dh_pk)
dm_key   = HKDF-SHA256(ikm=shared, salt="openmesh/v1/dm",
                       info=min(id_a,id_b) || max(id_a,id_b))
nonce    = random 12-byte prefix || 0^12;  AAD = 22-byte header with TTL nibble zeroed
```

Properties this gives us:

* Both directions derive the same key; keys are cached per peer and dropped when the peer is
  forgotten.
* Self-certifying identity: an announce can never claim a node id it does not own (verified by
  hashing the keys), so the DH key used for a DM is bound to the address.
* One-frame DMs, including while a peer is offline (store-and-forward).
* Channel messages use a PSK-derived key (`HKDF(psk, "openmesh/v1/channel", "channel/"||name)`)
  with an inner Ed25519 signature for authorship; group key distribution is out of band.

### 2.2 What OMP/2 must add (ratchet)

The minimum viable forward-secrecy increment, in order of preference:

1. **Per-peer symmetric ratchet first** (cheap): derive a chain key from the static shared
   secret, advance per message, include a short key epoch/sequence hint in the header. Gives
   forward secrecy against future compromise for messages already sent, but not
   post-compromise security.
2. **Receiving-direction ratchets with announced ratchet keys** (Reticulum-style): announcements
   carry a current ratchet public key; senders mix it into the KDF; receivers keep a bounded
   ring of ratchet private keys (Reticulum uses 512 keys / 30-minute rotation / 30-day expiry).
   Asynchronous, no round trip, works with store-and-forward. This is the most promising design
   for our constraints.
3. **Signal-style X3DH + double ratchet** with signed prekey bundles published in announces:
   strongest properties, but the largest protocol/state/UX surface and a prekey-exhaustion
   failure mode; likely not worth it until calls are real.

**Voice calls are gated on ratchets.** ARCHITECTURE.md is explicit: no real-time calls until
OMP/2 ratchets land, because calls without forward secrecy are a bad trade for a tool aimed at
at-risk users.

## 3. Consequences

### 3.1 Positive

* **Cheapest possible messages.** No handshake, no prekey fetch, no session state; a DM is one
  sealed frame with 33 bytes of envelope overhead (plus 72 bytes identity on first contact).
* **Asynchronous store-and-forward works naturally.** A relay can hold the ciphertext; the
  receiver decrypts days later because the key derives from long-term keys.
* **Simple client and firmware state.** No session store, no ratchet persistence, no prekey
  server; key caching is a per-peer `HashMap`.
* **The limitation is documented and testable.** PROTOCOL §17 is explicit; the threat model in
  ARCHITECTURE §6 is explicit; users are told.

### 3.2 Negative

* **No forward secrecy.** A stolen device or leaked seed decrypts all past direct messages to
  that node. This is a genuine loss versus Signal/Reticulum-links; "harvest now, decrypt later"
  applies.
* **No post-compromise security.** There is no healing after a compromise.
* **Identity-key binding is only as good as the announce chain.** TOFU on first contact; a
  malicious announce could be accepted if the user does not verify keys. Self-certifying ids
  prevent impersonation of an id, not a first-contact MITM.
* **Calls impossible without a new mechanism.** Deferring ratchets also defers calls.
* **Revocation is unsolved.** No key revocation in v1; changing keys changes the node id.

### 3.3 Mitigations for v1

* Store the 32-byte seed encrypted at rest (OS keystore on phones, derived-key flash storage on
  MCUs) and zeroize key material in memory (`zeroize` is used in the core).
* Document the threat model in the UI onboarding ("messages are encrypted in transit and at
  rest by relays; a stolen device can read old messages unless you use disappearing messages /
  rotate identity" — rotation is a future feature; the honest text is "a stolen device can read
  them").
* Plan OMP/2 ratchets now: reserve header bits/fields if needed (the version field and reserved
  payload flags exist), and keep the key schedule modular (HKDF salt/info strings can gain a
  ratchet input without changing XChaCha20-Poly1305).
* For calls, require OMP/2 and a non-duty-limited transport; see
  [reticulum-lxmf-notes.md](../research/reticulum-lxmf-notes.md) §4.2.

## 4. Alternatives considered

| Alternative | Why not for v1 |
| --- | --- |
| Link/session handshake (Reticulum/Noise-style) | Costs ~0.9–2.3 s of airtime and multiple frames before the first message; bad on LoRa, and our chat is one-frame. Good for OMP/2 calls. |
| Signal X3DH + double ratchet with prekeys | Strongest, but a large protocol/state/UX surface; prekey publication via announces is itself a design project; not justified until OMP/2. |
| Noise XX/IK handshakes | Same round-trip cost problem; IK needs a prekey, i.e. a bundle mechanism. |
| Reticulum ratchets inside announces | Best fit for our asynchronous model; reserved for OMP/2, not ignored. |
| No encryption, rely on channels | Unacceptable; direct messages need confidentiality. |
| **Static ECDH + XChaCha20-Poly1305 now, ratchet in OMP/2 (chosen)** | Optimal for v1 airtime, store-and-forward and simplicity; limitation explicit; path to more. |

## 5. Follow-ups

1. **OMP/2 design doc** for ratchets: choose between symmetric-chain ratchet, announced
   ratchet keys (Reticulum-style), and X3DH; define announce extension field, key ring bounds,
   persistence and expiry; add golden vectors.
2. **Calls** only after ratchets and only over transports that can carry them (Wi-Fi/TCP/fast
   short-range); see the capacity analysis for why 1 %-duty bands cannot do real-time.
3. **UX:** onboarding must say plainly that v1 has no forward secrecy and what that means;
   consider a "new identity" flow (which changes the node id and must be explained).
4. **Review:** have an external reviewer look at the composition (nonce prefix strategy, AAD
   construction, group signature domain separation) before claiming security in public docs.
