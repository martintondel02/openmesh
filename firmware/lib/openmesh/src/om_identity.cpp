// SPDX-License-Identifier: AGPL-3.0-or-later
#include "openmesh/om_identity.h"

#include <cstring>

#include "../third_party/monocypher/monocypher-ed25519.h"
#include "../third_party/monocypher/monocypher.h"
#include "openmesh/om_sha256.h"

namespace om {

namespace {

constexpr char kNodeDomain[] = "openmesh/v1/node";          // 16 bytes
constexpr size_t kNodeDomainLen = sizeof(kNodeDomain) - 1;
constexpr char kDhSalt[] = "openmesh/v1/dh";                // 14 bytes
constexpr size_t kDhSaltLen = sizeof(kDhSalt) - 1;
constexpr char kDmSalt[] = "openmesh/v1/dm";                // 14 bytes
constexpr size_t kDmSaltLen = sizeof(kDmSalt) - 1;
constexpr char kChannelSalt[] = "openmesh/v1/channel";      // 17 bytes
constexpr size_t kChannelSaltLen = sizeof(kChannelSalt) - 1;
constexpr char kChanIdDomain[] = "openmesh/v1/chan";        // 16 bytes
constexpr size_t kChanIdDomainLen = sizeof(kChanIdDomain) - 1;
constexpr char kStaticX25519Info[] = "static-x25519";       // 14 bytes
constexpr size_t kStaticX25519InfoLen = sizeof(kStaticX25519Info) - 1;
constexpr char kFlashSalt[] = "openmesh/v1/flash";          // 17 bytes
constexpr size_t kFlashSaltLen = sizeof(kFlashSalt) - 1;

void deriveDhSecret(const uint8_t seed[32], uint8_t dh_sk[32]) {
    om_hkdf_sha256(kDhSalt, kDhSaltLen, seed, 32, kStaticX25519Info, kStaticX25519InfoLen, dh_sk,
                   32);
}

// Seals using a 12-byte random prefix followed by 12 zero bytes (PROTOCOL.md §5).
size_t sealWithKey(const uint8_t key[32], RngFn rng, void* ctx, uint8_t payload_flags,
                   uint32_t msg_ref, const uint8_t* identity, const uint8_t aad[kHeaderLen],
                   const uint8_t* plaintext, size_t plaintext_len, uint8_t* out,
                   size_t capacity) {
    size_t prefix = 1 + kMsgRefLen + (identity != nullptr ? kPublicIdentityLen : 0);
    const size_t need = prefix + kSealOverhead + plaintext_len;
    if (capacity < need) return 0;
    if (rng == nullptr) return 0;

    out[0] = payload_flags;
    writeU32Be(out + 1, msg_ref);
    size_t off = 1 + kMsgRefLen;
    if (identity != nullptr) {
        std::memcpy(out + off, identity, kPublicIdentityLen);
        off += kPublicIdentityLen;
    }
    uint8_t nonce[24];
    std::memset(nonce, 0, sizeof(nonce));
    rng(ctx, nonce, kNoncePrefixLen);
    std::memcpy(out + off, nonce, kNoncePrefixLen);
    off += kNoncePrefixLen;
    uint8_t mac[kAeadTagLen];
    crypto_aead_lock(out + off, mac, key, nonce, aad, kHeaderLen, plaintext, plaintext_len);
    std::memcpy(out + off + plaintext_len, mac, kAeadTagLen);
    off += plaintext_len + kAeadTagLen;
    crypto_wipe(nonce, sizeof(nonce));
    return off;
}

bool openWithKey(const uint8_t key[32], const SealedBodyView& body,
                 const uint8_t aad[kHeaderLen], uint8_t* out, size_t capacity, size_t& out_len) {
    if (body.sealed == nullptr || body.sealed_len < kSealOverhead) return false;
    const size_t ct_len = body.sealed_len - kSealOverhead;
    if (capacity < ct_len) return false;
    uint8_t nonce[24];
    std::memset(nonce, 0, sizeof(nonce));
    std::memcpy(nonce, body.sealed, kNoncePrefixLen);
    const uint8_t* mac = body.sealed + kNoncePrefixLen + ct_len;
    const int rc = crypto_aead_unlock(out, mac, key, nonce, aad, kHeaderLen,
                                     body.sealed + kNoncePrefixLen, ct_len);
    crypto_wipe(nonce, sizeof(nonce));
    if (rc != 0) return false;
    out_len = ct_len;
    return true;
}

}  // namespace

bool Identity::fromSeed(const uint8_t seed[32], Identity& out) {
    Identity id;
    std::memcpy(id.seed, seed, 32);
    // Derive the X25519 static secret before the Ed25519 helper consumes (wipes) its seed
    // argument; id.seed is kept for sealed-at-rest storage.
    deriveDhSecret(id.seed, id.dh_sk);
    crypto_x25519_public_key(id.pub.dh_pk, id.dh_sk);
    // RFC 8032 Ed25519 (SHA-512), not Monocypher's BLAKE2b EdDSA: OMP/1 signatures must
    // interoperate with ed25519-dalek on the Rust side.
    uint8_t seed_copy[32];
    std::memcpy(seed_copy, seed, 32);
    crypto_ed25519_key_pair(id.sign_sk, id.pub.sign_pk, seed_copy);
    crypto_wipe(seed_copy, sizeof(seed_copy));

    uint8_t digest[32];
    om_sha256_ctx ctx;
    om_sha256_init(&ctx);
    om_sha256_update(&ctx, kNodeDomain, kNodeDomainLen);
    om_sha256_update(&ctx, id.pub.sign_pk, 32);
    om_sha256_update(&ctx, id.pub.dh_pk, 32);
    om_sha256_final(&ctx, digest);
    std::memcpy(id.pub.node_id.b, digest, 8);
    // Address-class rule (PROTOCOL.md §3): unicast node ids have the high bit of byte 0 cleared.
    id.pub.node_id.b[0] &= 0x7F;
    id.valid = true;
    out = id;
    return true;
}

bool Identity::generate(RngFn rng, void* ctx, Identity& out) {
    if (rng == nullptr) return false;
    uint8_t seed[32];
    rng(ctx, seed, sizeof(seed));
    const bool ok = fromSeed(seed, out);
    crypto_wipe(seed, sizeof(seed));
    return ok;
}

void Identity::wipe() {
    crypto_wipe(sign_sk, sizeof(sign_sk));
    crypto_wipe(dh_sk, sizeof(dh_sk));
    crypto_wipe(seed, sizeof(seed));
    std::memset(&pub, 0, sizeof(pub));
    valid = false;
}

bool Identity::sign(const uint8_t* message, size_t len, uint8_t signature[kSignatureLen]) const {
    if (!valid) return false;
    crypto_ed25519_sign(signature, sign_sk, message, len);
    return true;
}

bool Identity::verifyPeer(const PublicIdentity& peer, const uint8_t* message, size_t len,
                          const uint8_t signature[kSignatureLen]) const {
    return crypto_ed25519_check(signature, peer.sign_pk, message, len) == 0;
}

bool Identity::dmKey(const PublicIdentity& peer, uint8_t key[32]) const {
    if (!valid) return false;
    uint8_t shared[32];
    crypto_x25519(shared, dh_sk, peer.dh_pk);
    const NodeId& me = pub.node_id;
    const NodeId& them = peer.node_id;
    uint8_t info[16];
    if (me < them) {
        std::memcpy(info, me.b, 8);
        std::memcpy(info + 8, them.b, 8);
    } else {
        std::memcpy(info, them.b, 8);
        std::memcpy(info + 8, me.b, 8);
    }
    om_hkdf_sha256(kDmSalt, kDmSaltLen, shared, sizeof(shared), info, sizeof(info), key, 32);
    crypto_wipe(shared, sizeof(shared));
    return true;
}

size_t Identity::sealDirectBody(RngFn rng, void* rng_ctx, const PublicIdentity& peer,
                                uint32_t msg_ref, uint8_t payload_flags, bool include_identity,
                                const uint8_t aad[kHeaderLen], const uint8_t* plaintext,
                                size_t plaintext_len, uint8_t* out, size_t capacity) const {
    uint8_t key[32];
    if (!dmKey(peer, key)) return 0;
    uint8_t identity_bytes[kPublicIdentityLen];
    const uint8_t* identity = nullptr;
    if (include_identity) {
        pub.toBytes(identity_bytes);
        identity = identity_bytes;
    }
    const size_t n = sealWithKey(key, rng, rng_ctx, payload_flags, msg_ref, identity, aad,
                                 plaintext, plaintext_len, out, capacity);
    crypto_wipe(key, sizeof(key));
    return n;
}

bool Identity::openDirectBody(const PublicIdentity& peer, const SealedBodyView& body,
                              const uint8_t aad[kHeaderLen], uint8_t* out, size_t capacity,
                              size_t& out_len) const {
    uint8_t key[32];
    if (!dmKey(peer, key)) return false;
    const bool ok = openWithKey(key, body, aad, out, capacity, out_len);
    crypto_wipe(key, sizeof(key));
    return ok;
}

bool ChannelKey::derive(const uint8_t psk[32], const char* name, size_t name_len, ChannelKey& out) {
    if (name_len > 64) return false;
    uint8_t info[72];
    const char prefix[] = "channel/";
    const size_t prefix_len = sizeof(prefix) - 1;
    std::memcpy(info, prefix, prefix_len);
    std::memcpy(info + prefix_len, name, name_len);
    om_hkdf_sha256(kChannelSalt, kChannelSaltLen, psk, 32, info, prefix_len + name_len, out.key,
                   32);
    uint8_t digest[32];
    om_sha256_ctx ctx;
    om_sha256_init(&ctx);
    om_sha256_update(&ctx, kChanIdDomain, kChanIdDomainLen);
    om_sha256_update(&ctx, out.key, 32);
    om_sha256_final(&ctx, digest);
    std::memcpy(out.id.b, digest, 8);
    // Address-class rule (PROTOCOL.md §3): channel ids have the high bit of byte 0 set.
    out.id.b[0] |= kAddressClassChannel;
    out.valid = true;
    return true;
}

size_t ChannelKey::sealBody(RngFn rng, void* rng_ctx, uint32_t msg_ref, uint8_t payload_flags,
                            const uint8_t aad[kHeaderLen], const uint8_t* plaintext,
                            size_t plaintext_len, uint8_t* out, size_t capacity) const {
    if (!valid) return 0;
    // Same layout as a direct body without the optional identity; the sender identity travels
    // inside the GroupMessage plaintext instead.
    return sealWithKey(key, rng, rng_ctx, payload_flags, msg_ref, nullptr, aad, plaintext,
                       plaintext_len, out, capacity);
}

bool ChannelKey::openBody(const SealedBodyView& body, const uint8_t aad[kHeaderLen], uint8_t* out,
                          size_t capacity, size_t& out_len) const {
    if (!valid) return false;
    return openWithKey(key, body, aad, out, capacity, out_len);
}

bool sealSeedAtRest(const uint8_t seed[32], const uint8_t device_key[32], RngFn rng, void* ctx,
                    uint8_t out[kSealedSeedLen]) {
    if (rng == nullptr) return false;
    uint8_t key[32];
    om_hkdf_sha256(kFlashSalt, kFlashSaltLen, device_key, 32, nullptr, 0, key, 32);
    uint8_t nonce[24];
    rng(ctx, nonce, sizeof(nonce));
    uint8_t mac[kAeadTagLen];
    crypto_aead_lock(out + 24, mac, key, nonce, nullptr, 0, seed, 32);
    std::memcpy(out, nonce, 24);
    std::memcpy(out + 24 + 32, mac, kAeadTagLen);
    crypto_wipe(key, sizeof(key));
    crypto_wipe(nonce, sizeof(nonce));
    return true;
}

bool openSeedAtRest(const uint8_t* sealed, size_t len, const uint8_t device_key[32],
                    uint8_t seed[32]) {
    if (len != kSealedSeedLen) return false;
    uint8_t key[32];
    om_hkdf_sha256(kFlashSalt, kFlashSaltLen, device_key, 32, nullptr, 0, key, 32);
    const int rc = crypto_aead_unlock(seed, sealed + 24 + 32, key, sealed, nullptr, 0, sealed + 24,
                                      32);
    crypto_wipe(key, sizeof(key));
    return rc == 0;
}

void secureWipe(void* data, size_t len) { crypto_wipe(data, len); }

bool verifyAnnounce(const Frame& frame, Announce& out) {
    if (frame.type != FrameType::Announce) return false;
    if (frame.body_len < kAnnounceBodyLen) return false;
    // Announces are originated with TTL 8; relays only decrement, so ttl > 8 is invalid.
    if (frame.ttl > kAnnounceTtl) return false;
    const uint8_t* signature = nullptr;
    if (!decodeAnnounceBody(frame.body, frame.body_len, out, signature)) return false;
    if (out.version != kProtoVersion) return false;
    if ((uint8_t)out.cls > (uint8_t)NodeClass::Tracker) return false;
    // PROTOCOL.md §3: node ids are self-certifying; the frame source must match and the
    // address class bit must be a unicast id (channel ids are never announces).
    if (!out.ident.isSelfConsistent()) return false;
    if (!(out.ident.node_id == frame.src)) return false;
    if (!isUnicastAddress(out.ident.node_id)) return false;
    const size_t unsigned_len = frame.body_len - kSignatureLen;
    uint8_t payload[kMtu + 32];
    const size_t payload_len = announceSignaturePayload(frame.src, frame.seq, frame.body,
                                                         unsigned_len, payload, sizeof(payload));
    if (payload_len == 0) return false;
    return crypto_ed25519_check(signature, out.ident.sign_pk, payload, payload_len) == 0;
}

bool verifyGroupMessage(const uint8_t signature[kSignatureLen], const NodeId& sender,
                        const NodeId& channel, const uint8_t* message_cbor,
                        size_t message_cbor_len, const uint8_t sign_pk[32]) {
    uint8_t payload[kMtu + 64];
    const size_t n = groupSignaturePayload(sender, channel, message_cbor, message_cbor_len,
                                           payload, sizeof(payload));
    if (n == 0) return false;
    return crypto_ed25519_check(signature, sign_pk, payload, n) == 0;
}

size_t sealResourceBlob(RngFn rng, void* rng_ctx, const uint8_t key[32], uint32_t res_id,
                        uint32_t msg_id, uint32_t total_len, uint16_t chunk_bytes,
                        const uint8_t* plaintext, size_t plaintext_len, uint8_t* out,
                        size_t capacity) {
    if (rng == nullptr) return 0;
    const size_t need = 1 + kSealOverhead + plaintext_len;
    if (capacity < need) return 0;
    uint8_t aad[kResourceAadLen];
    resourceAad(res_id, msg_id, total_len, chunk_bytes, aad);
    out[0] = 0;  // payload_flags MUST be 0 for resource blobs
    uint8_t nonce[24];
    std::memset(nonce, 0, sizeof(nonce));
    rng(rng_ctx, nonce, kNoncePrefixLen);
    std::memcpy(out + 1, nonce, kNoncePrefixLen);
    uint8_t mac[kAeadTagLen];
    crypto_aead_lock(out + 1 + kNoncePrefixLen, mac, key, nonce, aad, sizeof(aad), plaintext,
                     plaintext_len);
    std::memcpy(out + 1 + kNoncePrefixLen + plaintext_len, mac, kAeadTagLen);
    crypto_wipe(nonce, sizeof(nonce));
    return need;
}

bool openResourceBlob(const uint8_t key[32], uint32_t res_id, uint32_t msg_id,
                      uint32_t total_len, uint16_t chunk_bytes, const uint8_t* blob,
                      size_t blob_len, uint8_t* out, size_t capacity, size_t& out_len) {
    if (blob_len < 1 + kSealOverhead) return false;
    if (blob[0] != 0) return false;
    // total_len is the length of the whole sealed blob (PROTOCOL.md §9.1), not the plaintext.
    if (blob_len != total_len) return false;
    const size_t ct_len = blob_len - 1 - kSealOverhead;
    if (capacity < ct_len) return false;
    uint8_t aad[kResourceAadLen];
    resourceAad(res_id, msg_id, total_len, chunk_bytes, aad);
    uint8_t nonce[24];
    std::memset(nonce, 0, sizeof(nonce));
    std::memcpy(nonce, blob + 1, kNoncePrefixLen);
    const int rc = crypto_aead_unlock(out, blob + 1 + kNoncePrefixLen + ct_len, key, nonce, aad,
                                     sizeof(aad), blob + 1 + kNoncePrefixLen, ct_len);
    crypto_wipe(nonce, sizeof(nonce));
    if (rc != 0) return false;
    out_len = ct_len;
    return true;
}

}  // namespace om
