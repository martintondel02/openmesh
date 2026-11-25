// SPDX-License-Identifier: AGPL-3.0-or-later
// OMP/1 core types: frame codec, identities, body layout helpers.

#include "openmesh/om_types.h"

#include "openmesh/om_sha256.h"

namespace om {

namespace {
constexpr char kHexDigits[] = "0123456789abcdef";

int hexNibble(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}
}  // namespace

void NodeId::toHex(char* out) const {
    for (int i = 0; i < 8; ++i) {
        out[i * 2] = kHexDigits[b[i] >> 4];
        out[i * 2 + 1] = kHexDigits[b[i] & 0x0F];
    }
    out[16] = '\0';
}

bool NodeId::fromHex(const char* s, NodeId& out) {
    if (s == nullptr) return false;
    if (s[0] == '0' && (s[1] == 'x' || s[1] == 'X')) s += 2;
    for (int i = 0; i < 8; ++i) {
        int hi = hexNibble(s[i * 2]);
        int lo = hexNibble(s[i * 2 + 1]);
        if (hi < 0 || lo < 0) return false;
        out.b[i] = (uint8_t)((hi << 4) | lo);
    }
    return true;
}

bool isStorable(FrameType t) { return t == FrameType::Data || t == FrameType::Resource; }

bool isKnownFrameType(uint8_t raw) { return raw <= kMaxKnownFrameType; }

bool isFloodable(FrameType t) {
    switch (t) {
        case FrameType::Announce:
        case FrameType::Data:
        case FrameType::Ack:
        case FrameType::Resource:
        case FrameType::RouteRequest:
        case FrameType::RouteResponse:
        case FrameType::Probe:
        case FrameType::ProbeResponse:
        case FrameType::StoreOffer:
        case FrameType::StoreWant:
            return true;
        case FrameType::Admin:
        default:
            return false;
    }
}

size_t Frame::encode(uint8_t* out, size_t capacity) const {
    if (body_len > kMaxBody) return 0;
    if (ttl > 0x0F) return 0;
    const size_t total = kHeaderLen + (size_t)body_len;
    if (capacity < total) return 0;
    out[0] = (uint8_t)((kProtoVersion << 5) | ((uint8_t)type & 0x1F));
    out[1] = (uint8_t)((ttl << 4) | (flags & 0x0F));
    std::memcpy(out + 2, src.b, 8);
    std::memcpy(out + 10, dst.b, 8);
    writeU32Be(out + 18, seq);
    if (body_len > 0) std::memcpy(out + kHeaderLen, body, body_len);
    return total;
}

void Frame::aad(uint8_t out[kHeaderLen]) const {
    // The only header field relays may modify is the TTL, so the TTL nibble is not
    // authenticated: senders and receivers both zero it (PROTOCOL.md §5).
    out[0] = (uint8_t)((kProtoVersion << 5) | ((uint8_t)type & 0x1F));
    out[1] = (uint8_t)(flags & 0x0F);
    std::memcpy(out + 2, src.b, 8);
    std::memcpy(out + 10, dst.b, 8);
    writeU32Be(out + 18, seq);
}

bool Frame::relayCopy(Frame& out) const {
    if (ttl == 0) return false;
    out = *this;
    out.ttl = (uint8_t)(ttl - 1);
    return true;
}

DecodeResult decodeFrame(const uint8_t* data, size_t len, Frame& out) {
    DecodeResult r;
    if (len < kHeaderLen) {
        r.error = DecodeError::TooShort;
        return r;
    }
    if (len > kMtu) {
        r.error = DecodeError::TooLong;
        return r;
    }
    const uint8_t version = (uint8_t)(data[0] >> 5);
    if (version != kProtoVersion) {
        r.error = DecodeError::BadVersion;
        return r;
    }
    const uint8_t raw_type = (uint8_t)(data[0] & 0x1F);
    if (!isKnownFrameType(raw_type)) {
        // Reserved types (11..31) must be ignored so a future peer does not black-hole the
        // network, but they are not forwarded by a v1 implementation.
        r.error = DecodeError::UnknownFrameType;
        return r;
    }
    out.ttl = (uint8_t)(data[1] >> 4);
    out.flags = (uint8_t)(data[1] & 0x0F);
    if ((out.flags & kFlagReserved) != 0) {
        // Bit 3 is reserved and MUST be zero (PROTOCOL.md §4.1).
        r.error = DecodeError::Invalid;
        return r;
    }
    out.type = (FrameType)raw_type;
    std::memcpy(out.src.b, data + 2, 8);
    std::memcpy(out.dst.b, data + 10, 8);
    out.seq = readU32Be(data + 18);
    out.body_len = (uint16_t)(len - kHeaderLen);
    if (out.body_len > 0) std::memcpy(out.body, data + kHeaderLen, out.body_len);
    return r;
}

void PublicIdentity::toBytes(uint8_t out[kPublicIdentityLen]) const {
    std::memcpy(out, node_id.b, 8);
    std::memcpy(out + 8, sign_pk, 32);
    std::memcpy(out + 40, dh_pk, 32);
}

bool PublicIdentity::fromBytes(const uint8_t* data, size_t len, PublicIdentity& out) {
    if (len != kPublicIdentityLen) return false;
    std::memcpy(out.node_id.b, data, 8);
    std::memcpy(out.sign_pk, data + 8, 32);
    std::memcpy(out.dh_pk, data + 40, 32);
    return true;
}

bool PublicIdentity::isSelfConsistent() const {
    uint8_t buf[72];
    toBytes(buf);
    uint8_t hash[32];
    om_sha256_ctx ctx;
    om_sha256_init(&ctx);
    om_sha256_update(&ctx, "openmesh/v1/node", 16);
    om_sha256_update(&ctx, sign_pk, 32);
    om_sha256_update(&ctx, dh_pk, 32);
    om_sha256_final(&ctx, hash);
    // Address-class rule: unicast node ids clear the high bit of byte 0 (PROTOCOL.md §3).
    hash[0] &= 0x7F;
    return std::memcmp(node_id.b, hash, 8) == 0;
}

bool PublicIdentity::operator==(const PublicIdentity& o) const {
    return node_id == o.node_id && std::memcmp(sign_pk, o.sign_pk, 32) == 0 &&
           std::memcmp(dh_pk, o.dh_pk, 32) == 0;
}

bool SealedBodyView::parse(const uint8_t* body, size_t len, SealedBodyView& out) {
    if (len < 1 + kMsgRefLen + kSealOverhead) return false;
    out.payload_flags = body[0];
    if ((out.payload_flags & (uint8_t)~kPayloadFlagMaskDefined) != 0) return false;
    out.msg_ref = readU32Be(body + 1);
    size_t off = 1 + kMsgRefLen;
    out.identity = nullptr;
    if ((out.payload_flags & kPayloadFlagIdentityIncluded) != 0) {
        if (len < off + kPublicIdentityLen + kSealOverhead) return false;
        out.identity = body + off;
        off += kPublicIdentityLen;
    }
    out.sealed = body + off;
    out.sealed_len = len - off;
    return out.sealed_len >= kSealOverhead;
}

void writeU32Be(uint8_t* out, uint32_t v) {
    out[0] = (uint8_t)(v >> 24);
    out[1] = (uint8_t)(v >> 16);
    out[2] = (uint8_t)(v >> 8);
    out[3] = (uint8_t)v;
}

void writeU16Be(uint8_t* out, uint16_t v) {
    out[0] = (uint8_t)(v >> 8);
    out[1] = (uint8_t)v;
}

uint32_t readU32Be(const uint8_t* in) {
    return ((uint32_t)in[0] << 24) | ((uint32_t)in[1] << 16) | ((uint32_t)in[2] << 8) |
           (uint32_t)in[3];
}

uint16_t readU16Be(const uint8_t* in) { return (uint16_t)(((uint16_t)in[0] << 8) | in[1]); }

uint64_t readU64Be(const uint8_t* in) {
    uint64_t v = 0;
    for (int i = 0; i < 8; ++i) v = (v << 8) | in[i];
    return v;
}

void writeU64Be(uint8_t* out, uint64_t v) {
    for (int i = 7; i >= 0; --i) {
        out[i] = (uint8_t)v;
        v >>= 8;
    }
}

}  // namespace om
