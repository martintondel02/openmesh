// SPDX-License-Identifier: AGPL-3.0-or-later
// OpenMesh firmware — OMP/1 core types.
//
// Portable C++17, no Arduino/PlatformIO dependencies. Mirrors
// `crates/openmesh-core/src/wire.rs` and docs/architecture/PROTOCOL.md §3-§4.
#ifndef OPENMESH_OM_TYPES_H
#define OPENMESH_OM_TYPES_H

#include <cstddef>
#include <cstdint>
#include <cstring>

namespace om {

// --- Protocol constants (PROTOCOL.md §4, §12) -----------------------------------------------
constexpr uint8_t kProtoVersion = 1;
constexpr size_t kMtu = 240;
constexpr size_t kHeaderLen = 22;
constexpr size_t kMaxBody = kMtu - kHeaderLen;  // 218
constexpr size_t kPublicIdentityLen = 72;      // node_id(8) || sign_pk(32) || dh_pk(32)
constexpr size_t kSignatureLen = 64;
constexpr size_t kAeadTagLen = 16;
constexpr size_t kNoncePrefixLen = 12;
constexpr size_t kSealOverhead = kNoncePrefixLen + kAeadTagLen;
constexpr size_t kMsgRefLen = 4;

constexpr uint8_t kDefaultTtl = 5;
constexpr uint8_t kAnnounceTtl = 8;


// Timing (PROTOCOL.md §12), milliseconds.
constexpr uint64_t kAnnounceIntervalClientMs = 1'800'000;
constexpr uint64_t kAnnounceIntervalRelayMs = 3'600'000;
constexpr uint64_t kAnnounceMinIntervalMs = 300'000;
constexpr uint64_t kDedupTtlMs = 1'800'000;
constexpr uint64_t kAckTimeoutMs = 8'000;
constexpr uint8_t kMaxMsgRetries = 3;
constexpr uint64_t kStoreOfferIntervalMs = 300'000;
constexpr uint64_t kStoreTtlMs = 259'200'000;  // 72 h
constexpr uint64_t kPeerExpiryMs = 86'400'000;  // 24 h

// --- Node ids -------------------------------------------------------------------------------
struct NodeId {
    uint8_t b[8];

    constexpr NodeId() : b{0, 0, 0, 0, 0, 0, 0, 0} {}
    constexpr NodeId(uint8_t b0, uint8_t b1, uint8_t b2, uint8_t b3, uint8_t b4, uint8_t b5,
                     uint8_t b6, uint8_t b7)
        : b{b0, b1, b2, b3, b4, b5, b6, b7} {}
    explicit constexpr NodeId(const uint8_t raw[8])
        : b{raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7]} {}

    static constexpr NodeId broadcast() { return NodeId{0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF}; }
    static constexpr NodeId zero() { return NodeId(); }

    bool isBroadcast() const {
        for (int i = 0; i < 8; ++i)
            if (b[i] != 0xFF) return false;
        return true;
    }
    bool isZero() const {
        for (int i = 0; i < 8; ++i)
            if (b[i] != 0) return false;
        return true;
    }
    bool operator==(const NodeId& o) const { return std::memcmp(b, o.b, 8) == 0; }
    bool operator!=(const NodeId& o) const { return !(*this == o); }
    bool operator<(const NodeId& o) const { return std::memcmp(b, o.b, 8) < 0; }

    // Writes exactly 16 lowercase hex chars + NUL into out (must hold >= 17 bytes).
    void toHex(char* out) const;
    static bool fromHex(const char* s, NodeId& out);
};

// Address-class bit (PROTOCOL.md §3): bit 7 of byte 0 is 0 for unicast node ids and 1 for
// channel ids; BROADCAST is all-ones. Relays use this bit to decide storability without
// holding any key material.
constexpr uint8_t kAddressClassChannel = 0x80;

inline bool isChannelAddress(const NodeId& id) {
    return (id.b[0] & kAddressClassChannel) != 0 && !id.isBroadcast();
}
inline bool isUnicastAddress(const NodeId& id) { return (id.b[0] & kAddressClassChannel) == 0; }

// --- Frame types (PROTOCOL.md §4.2) ---------------------------------------------------------
enum class FrameType : uint8_t {
    Announce = 0,
    Data = 1,
    Ack = 2,
    Resource = 3,
    RouteRequest = 4,
    RouteResponse = 5,
    Probe = 6,
    ProbeResponse = 7,
    StoreOffer = 8,
    StoreWant = 9,
    Admin = 10,
    // Values 11..31 are reserved: receivers must ignore them, not treat them as errors.
};

constexpr uint8_t kMaxKnownFrameType = 10;

// Header flags (low nibble of byte 1). Bit 3 is reserved and MUST be 0.
constexpr uint8_t kFlagEncrypted = 0x01;   // ENC
constexpr uint8_t kFlagAckRequest = 0x02;  // ACK_REQ
constexpr uint8_t kFlagPriority = 0x04;    // PRIORITY
constexpr uint8_t kFlagMaskDefined = 0x07;
constexpr uint8_t kFlagReserved = 0x08;

// Payload flags, first byte of every sealed body (PROTOCOL.md §5).
constexpr uint8_t kPayloadFlagIdentityIncluded = 0x01;
constexpr uint8_t kPayloadFlagIsAck = 0x02;
constexpr uint8_t kPayloadFlagIsManifest = 0x04;
constexpr uint8_t kPayloadFlagMaskDefined = 0x07;

bool isStorable(FrameType t);
// True for frame types that flooding relays forward. Unknown/reserved types are not
// forwarded by v1 implementations (PROTOCOL.md §16).
bool isFloodable(FrameType t);
bool isKnownFrameType(uint8_t raw);

// --- Frame ----------------------------------------------------------------------------------
struct Frame {
    uint8_t ttl = 0;
    uint8_t flags = 0;
    FrameType type = FrameType::Data;
    NodeId src{};
    NodeId dst{};
    uint32_t seq = 0;
    uint8_t body[kMaxBody]{};
    uint16_t body_len = 0;

    bool hasFlag(uint8_t f) const { return (flags & f) != 0; }
    bool isBroadcast() const { return dst.isBroadcast(); }

    // Encodes header+body into `out` (must hold `capacity` bytes). Returns bytes written, or
    // 0 on error (body too large / buffer too small / ttl out of range).
    size_t encode(uint8_t* out, size_t capacity) const;

    // AEAD associated data: the 22-byte header with the TTL nibble (high nibble of byte 1)
    // forced to zero. Must be identical on sender, relay and receiver. PROTOCOL.md §5.
    void aad(uint8_t out[kHeaderLen]) const;

    // TTL-decremented copy for relaying; false when the hop budget is exhausted.
    bool relayCopy(Frame& out) const;
};

enum class DecodeError : uint8_t {
    None = 0,
    TooShort,
    TooLong,
    BadVersion,
    UnknownFrameType,  // reserved 11..31: must be ignored, not an error
    Invalid,           // e.g. reserved flag bits set
};

struct DecodeResult {
    DecodeError error = DecodeError::None;
    bool ok() const { return error == DecodeError::None; }
    bool shouldIgnore() const { return error == DecodeError::UnknownFrameType; }
};

DecodeResult decodeFrame(const uint8_t* data, size_t len, Frame& out);

// --- Public identity (PROTOCOL.md §3) -------------------------------------------------------
struct PublicIdentity {
    NodeId node_id{};
    uint8_t sign_pk[32]{};
    uint8_t dh_pk[32]{};

    void toBytes(uint8_t out[kPublicIdentityLen]) const;
    static bool fromBytes(const uint8_t* data, size_t len, PublicIdentity& out);
    bool isSelfConsistent() const;
    bool operator==(const PublicIdentity& o) const;
};

// --- Sealed body layout (PROTOCOL.md §5) ----------------------------------------------------
struct SealedBodyView {
    uint8_t payload_flags = 0;
    uint32_t msg_ref = 0;
    const uint8_t* identity = nullptr;  // 72 bytes when IDENTITY_INCLUDED
    const uint8_t* sealed = nullptr;    // nonce_prefix(12) || ciphertext || tag(16)
    size_t sealed_len = 0;

    static bool parse(const uint8_t* body, size_t len, SealedBodyView& out);
};

// --- Helpers --------------------------------------------------------------------------------
void writeU32Be(uint8_t* out, uint32_t v);
void writeU16Be(uint8_t* out, uint16_t v);
uint32_t readU32Be(const uint8_t* in);
uint16_t readU16Be(const uint8_t* in);
uint64_t readU64Be(const uint8_t* in);
void writeU64Be(uint8_t* out, uint64_t v);

}  // namespace om

#endif  // OPENMESH_OM_TYPES_H
