// SPDX-License-Identifier: AGPL-3.0-or-later
// OpenMesh identity, key agreement and payload sealing (PROTOCOL.md §3, §5, §6).
//
// Crypto primitives: Monocypher 4.0.2 (vendored, BSD-2-Clause/CC0) for Ed25519, X25519 and
// XChaCha20-Poly1305, plus om_sha256 for SHA-256/HMAC/HKDF.
//
// This header is Arduino-free; the embedder injects a CSPRNG callback.
#ifndef OPENMESH_OM_IDENTITY_H
#define OPENMESH_OM_IDENTITY_H

#include <cstddef>
#include <cstdint>

#include "openmesh/om_messages.h"
#include "openmesh/om_types.h"

namespace om {

// Constant-time wipe used for key material held by the embedder (e.g. the decrypted seed).
void secureWipe(void* data, size_t len);

// CSPRNG callback supplied by the HAL (nRF52: NRF_RNG, ESP32: esp_fill_random, host: OS).
using RngFn = void (*)(void* ctx, uint8_t* out, size_t len);

// Encrypts the stored seed at rest with a key derived from a device-unique value. This is
// obfuscation against casual flash extraction, not protection against an attacker with the
// physical device: see firmware/README.md "Key storage".
constexpr size_t kSealedSeedLen = 24 + 32 + kAeadTagLen;  // nonce || ciphertext || tag

struct Identity {
    PublicIdentity pub{};
    uint8_t seed[32]{};
    uint8_t sign_sk[64]{};
    uint8_t dh_sk[32]{};
    bool valid = false;

    static bool fromSeed(const uint8_t seed[32], Identity& out);
    static bool generate(RngFn rng, void* ctx, Identity& out);
    void wipe();

    const PublicIdentity& publicIdentity() const { return pub; }
    const NodeId& nodeId() const { return pub.node_id; }

    bool sign(const uint8_t* message, size_t len, uint8_t signature[kSignatureLen]) const;
    bool verifyPeer(const PublicIdentity& peer, const uint8_t* message, size_t len,
                    const uint8_t signature[kSignatureLen]) const;

    // X25519 + HKDF-SHA256 direct-message key for `peer` (both directions agree).
    bool dmKey(const PublicIdentity& peer, uint8_t key[32]) const;

    // Seal `plaintext` into an OMP body (PROTOCOL.md §5):
    //   payload_flags(1) || msg_ref(4 BE) || [identity(72)] || nonce_prefix(12) || ct || tag(16)
    // `aad` must be Frame::aad() of the final frame (TTL nibble zeroed); `rng` supplies the
    // 12-byte nonce prefix.
    size_t sealDirectBody(RngFn rng, void* rng_ctx, const PublicIdentity& peer, uint32_t msg_ref,
                          uint8_t payload_flags, bool include_identity,
                          const uint8_t aad[kHeaderLen], const uint8_t* plaintext,
                          size_t plaintext_len, uint8_t* out, size_t capacity) const;

    // Open a direct body addressed to this node. `peer` must carry the sender's DH key.
    bool openDirectBody(const PublicIdentity& peer, const SealedBodyView& body,
                        const uint8_t aad[kHeaderLen], uint8_t* out, size_t capacity,
                        size_t& out_len) const;
};

// Symmetric group channel (PROTOCOL.md §5.2).
struct ChannelKey {
    uint8_t key[32]{};
    NodeId id{};
    bool valid = false;

    static bool derive(const uint8_t psk[32], const char* name, size_t name_len, ChannelKey& out);

    size_t sealBody(RngFn rng, void* rng_ctx, uint32_t msg_ref, uint8_t payload_flags,
                    const uint8_t aad[kHeaderLen], const uint8_t* plaintext,
                    size_t plaintext_len, uint8_t* out, size_t capacity) const;
    bool openBody(const SealedBodyView& body, const uint8_t aad[kHeaderLen], uint8_t* out,
                  size_t capacity, size_t& out_len) const;
};

// Seed-at-rest helpers (used by the board storage HAL).
bool sealSeedAtRest(const uint8_t seed[32], const uint8_t device_key[32], RngFn rng, void* ctx,
                    uint8_t out[kSealedSeedLen]);
bool openSeedAtRest(const uint8_t* sealed, size_t len, const uint8_t device_key[32],
                    uint8_t seed[32]);

// Verify and decode a binary announce frame (PROTOCOL.md §6). On success `out` is populated and
// the reserved byte, version, name length, self-certifying id (including the address-class
// bit), frame src, TTL range and Ed25519 signature are all checked.
bool verifyAnnounce(const Frame& frame, Announce& out);

// Verify a GroupMessage signature given the sender's public key (resolved by the caller from
// its peer book or an IDENTITY_INCLUDED body).
bool verifyGroupMessage(const uint8_t signature[kSignatureLen], const NodeId& sender,
                        const NodeId& channel, const uint8_t* message_cbor,
                        size_t message_cbor_len, const uint8_t sign_pk[32]);

// Resource blob sealing (PROTOCOL.md §9.1): body = payload_flags(1)=0 || nonce_prefix(12) ||
// ct || tag(16), AAD as in resourceAad(). `key` is the DM or channel key.
size_t sealResourceBlob(RngFn rng, void* rng_ctx, const uint8_t key[32], uint32_t res_id,
                        uint32_t msg_id, uint32_t total_len, uint16_t chunk_bytes,
                        const uint8_t* plaintext, size_t plaintext_len, uint8_t* out,
                        size_t capacity);
bool openResourceBlob(const uint8_t key[32], uint32_t res_id, uint32_t msg_id,
                      uint32_t total_len, uint16_t chunk_bytes, const uint8_t* blob,
                      size_t blob_len, uint8_t* out, size_t capacity, size_t& out_len);

}  // namespace om

#endif  // OPENMESH_OM_IDENTITY_H
