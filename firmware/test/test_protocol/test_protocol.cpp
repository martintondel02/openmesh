// SPDX-License-Identifier: AGPL-3.0-or-later
// OMP/1 firmware unit + integration tests (no radio, no Arduino, no filesystem).
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <memory>
#include <string>
#include <vector>

#ifdef OM_USE_UNITY
#include <unity.h>
#else
#include "test_shim.h"
#endif

#include "board.h"
#include "openmesh/om_companion.h"
#include "openmesh/om_dedup.h"
#include "openmesh/om_engine.h"
#include "openmesh/om_gps.h"
#include "openmesh/om_identity.h"
#include "openmesh/om_messages.h"
#include "openmesh/om_power.h"
#include "openmesh/om_region.h"
#include "openmesh/om_sha256.h"
#include "openmesh/om_store.h"
#include "openmesh/om_types.h"
#include "tests.h"

// Raw Monocypher primitive used by the RFC 7748 known-answer test (linked from the vendored
// monocyper.c build; declared here to keep the test independent of include paths).
extern "C" void crypto_x25519(uint8_t raw_shared_secret[32], const uint8_t your_secret_key[32],
                              const uint8_t their_public_key[32]);

namespace {

using namespace om;

int hexNib(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

bool hexToBytes(const char* hex, uint8_t* out, size_t cap, size_t& len) {
    if (hex == nullptr) return false;
    size_t n = std::strlen(hex);
    if (n % 2 != 0 || n / 2 > cap) return false;
    for (size_t i = 0; i < n / 2; ++i) {
        const int hi = hexNib(hex[i * 2]);
        const int lo = hexNib(hex[i * 2 + 1]);
        if (hi < 0 || lo < 0) return false;
        out[i] = (uint8_t)((hi << 4) | lo);
    }
    len = n / 2;
    return true;
}

std::string bytesToHex(const uint8_t* data, size_t len) {
    static const char digits[] = "0123456789abcdef";
    std::string out;
    out.reserve(len * 2);
    for (size_t i = 0; i < len; ++i) {
        out.push_back(digits[data[i] >> 4]);
        out.push_back(digits[data[i] & 0x0F]);
    }
    return out;
}

struct RngState {
    uint64_t state = 0x9E3779B97F4A7C15ull;
};

void testRng(void* ctx, uint8_t* out, size_t len) {
    RngState* rng = static_cast<RngState*>(ctx);
    for (size_t i = 0; i < len; ++i) {
        rng->state ^= rng->state << 13;
        rng->state ^= rng->state >> 7;
        rng->state ^= rng->state << 17;
        out[i] = (uint8_t)(rng->state >> 24);
    }
}

struct Node {
    Identity id;
    Engine* engine = nullptr;
    ~Node() {
        delete engine;
    }
};

Identity makeIdentity(uint8_t seed_byte) {
    uint8_t seed[32];
    std::memset(seed, seed_byte, sizeof(seed));
    Identity id;
    Identity::fromSeed(seed, id);
    return id;
}

// RAM-only storage backend for config-persistence tests.
class InMemoryStorage : public StorageBackend {
public:
    bool load(const char* name, uint8_t* out, size_t capacity, size_t& len) override {
        for (const auto& file : files_) {
            if (std::strcmp(file.name, name) != 0) continue;
            if (file.len > capacity) return false;
            std::memcpy(out, file.data, file.len);
            len = file.len;
            return true;
        }
        len = 0;
        return false;
    }
    bool save(const char* name, const uint8_t* data, size_t len) override {
        if (len > sizeof(File::data)) return false;
        for (auto& file : files_) {
            if (std::strcmp(file.name, name) == 0) {
                std::memcpy(file.data, data, len);
                file.len = len;
                return true;
            }
        }
        File file;
        std::snprintf(file.name, sizeof(file.name), "%s", name);
        std::memcpy(file.data, data, len);
        file.len = len;
        files_.push_back(file);
        return true;
    }
    bool remove(const char* name) override {
        for (size_t i = 0; i < files_.size(); ++i) {
            if (std::strcmp(files_[i].name, name) == 0) {
                files_.erase(files_.begin() + (long)i);
                return true;
            }
        }
        return false;
    }

private:
    struct File {
        char name[40];
        uint8_t data[512];
        size_t len = 0;
    };
    std::vector<File> files_;
};

EngineConfig defaultEngineConfig(uint8_t seed_byte, bool relay, bool store, bool channels) {
    EngineConfig config;
    config.cls = store ? NodeClass::Repeater : (relay ? NodeClass::Repeater : NodeClass::Client);
    config.name_len = (uint8_t)std::snprintf(config.name, sizeof(config.name), "node-%u",
                                             (unsigned)seed_byte);
    config.caps = 0;
    config.default_ttl = kDefaultTtl;
    config.profile = &kMediumFast;
    config.region = &kRegionEu868;
    config.relay_role = relay;
    config.store_role = store;
    config.accept_channels = channels;
    config.position_sharing = true;
    config.announce_interval_ms = (uint32_t)kAnnounceIntervalClientMs;
    config.telemetry_interval_ms = 3600000;
    config.hardware_max_power_dbm = board::kMaxTxPowerSx1262Dbm;
    config.tx_power_dbm = effectivePowerDbm(*config.region, config.hardware_max_power_dbm,
                                            kNoTxPowerOverrideDbm, false);
    return config;
}

std::unique_ptr<Node> makeNodeWithConfig(uint8_t seed_byte, const EngineConfig& config,
                                         StorageBackend* storage = nullptr) {
    auto node = std::unique_ptr<Node>(new Node());
    node->id = makeIdentity(seed_byte);
    static RngState rng_state;
    node->engine = new Engine();
    TEST_ASSERT_TRUE(node->engine->begin(node->id, config, testRng, &rng_state, storage));
    node->engine->start(1000);
    return node;
}

std::unique_ptr<Node> makeNode(uint8_t seed_byte, bool relay, bool store, bool channels = false) {
    return makeNodeWithConfig(seed_byte, defaultEngineConfig(seed_byte, relay, store, channels));
}

Frame makeSealedDirectFrame(const Identity& from, const PublicIdentity& to, const NodeId& dst,
                            const uint8_t* plaintext, size_t plaintext_len, uint32_t msg_ref,
                            uint32_t seq, uint8_t flags, bool include_identity) {
    Frame frame;
    frame.type = FrameType::Data;
    frame.flags = flags;
    frame.ttl = kDefaultTtl;
    frame.src = from.nodeId();
    frame.dst = dst;
    frame.seq = seq;
    uint8_t aad[kHeaderLen];
    frame.aad(aad);
    static RngState rng_state;
    const size_t body_len = from.sealDirectBody(testRng, &rng_state, to, msg_ref,
                                                include_identity ? kPayloadFlagIdentityIncluded
                                                                 : 0,
                                                include_identity, aad, plaintext,
                                                plaintext_len, frame.body, sizeof(frame.body));
    frame.body_len = (uint16_t)body_len;
    return frame;
}

size_t makeAppMessageText(uint32_t id, const char* text, uint8_t* out, size_t cap) {
    AppMessage msg;
    msg.id = id;
    msg.kind = MessageKind::Text;
    msg.sent_at_ms = 1700000000000ull;
    msg.has_text = true;
    msg.text.len = std::strlen(text);
    std::memcpy(msg.text.data, text, msg.text.len);
    return encodeAppMessage(msg, out, cap);
}

Frame makeStoredCandidate(const Identity& from, const NodeId& dst, uint32_t msg_ref,
                          uint32_t seq) {
    Frame frame;
    frame.type = FrameType::Data;
    frame.flags = kFlagEncrypted;
    frame.ttl = kDefaultTtl;
    frame.src = from.nodeId();
    frame.dst = dst;
    frame.seq = seq;
    frame.body[0] = 0;  // payload_flags: no identity
    writeU32Be(frame.body + 1, msg_ref);
    for (size_t i = 5; i < 5 + kSealOverhead; ++i) frame.body[i] = (uint8_t)(i * 7);
    frame.body_len = (uint16_t)(5 + kSealOverhead);
    return frame;
}

std::string nmeaWithChecksum(const std::string& body) {
    uint8_t checksum = 0;
    for (char c : body) checksum ^= (uint8_t)c;
    char tail[8];
    std::snprintf(tail, sizeof(tail), "*%02X\r\n", checksum);
    return "$" + body + tail;
}

}  // namespace

// --- Crypto ---------------------------------------------------------------------------------
void test_sha256_vectors() {
    uint8_t out[32];
    om_sha256("abc", 3, out);
    TEST_ASSERT_EQUAL_STRING(
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        bytesToHex(out, 32).c_str());
    om_sha256("", 0, out);
    TEST_ASSERT_EQUAL_STRING(
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        bytesToHex(out, 32).c_str());
    // Multi-block input exercises the length padding path (1,000,000 x 'a').
    static uint8_t million[1000];
    std::memset(million, 'a', sizeof(million));
    om_sha256_ctx ctx;
    om_sha256_init(&ctx);
    for (int i = 0; i < 1000; ++i) om_sha256_update(&ctx, million, sizeof(million));
    om_sha256_final(&ctx, out);
    TEST_ASSERT_EQUAL_STRING(
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
        bytesToHex(out, 32).c_str());
}

void test_hmac_sha256_vectors() {
    // RFC 4231 test case 2.
    uint8_t mac[32];
    om_hmac_sha256("Jefe", 4, "what do ya want for nothing?", 28, mac);
    TEST_ASSERT_EQUAL_STRING(
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
        bytesToHex(mac, 32).c_str());
}

void test_hkdf_sha256_vectors() {
    // RFC 5869 test case 1.
    uint8_t ikm[22];
    std::memset(ikm, 0x0b, sizeof(ikm));
    const uint8_t salt[] = {0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06,
                             0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c};
    const uint8_t info[] = {0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9};
    uint8_t okm[42];
    om_hkdf_sha256(salt, sizeof(salt), ikm, sizeof(ikm), info, sizeof(info), okm, sizeof(okm));
    TEST_ASSERT_EQUAL_STRING(
        "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf"
        "34007208d5b887185865",
        bytesToHex(okm, sizeof(okm)).c_str());
}

void test_ed25519_rfc8032() {
    // RFC 8032 §7.1 TEST 1.
    uint8_t seed[32];
    size_t seed_len = 0;
    TEST_ASSERT_TRUE(hexToBytes("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
                                 seed, sizeof(seed), seed_len));
    Identity id;
    TEST_ASSERT_TRUE(Identity::fromSeed(seed, id));
    TEST_ASSERT_EQUAL_STRING(
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        bytesToHex(id.publicIdentity().sign_pk, 32).c_str());
    uint8_t signature[64];
    TEST_ASSERT_TRUE(id.sign(nullptr, 0, signature));
    TEST_ASSERT_EQUAL_STRING(
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155"
        "5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
        bytesToHex(signature, 64).c_str());
    TEST_ASSERT_TRUE(id.verifyPeer(id.publicIdentity(), nullptr, 0, signature));
    signature[0] ^= 0x01;
    TEST_ASSERT_FALSE(id.verifyPeer(id.publicIdentity(), nullptr, 0, signature));
}

void test_x25519_rfc7748() {
    uint8_t scalar[32];
    uint8_t u[32];
    uint8_t expected[32];
    size_t n = 0;
    TEST_ASSERT_TRUE(hexToBytes(
        "a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4", scalar,
        sizeof(scalar), n));
    TEST_ASSERT_TRUE(hexToBytes(
        "e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c", u, sizeof(u), n));
    TEST_ASSERT_TRUE(hexToBytes(
        "c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552", expected,
        sizeof(expected), n));
    uint8_t shared[32];
    crypto_x25519(shared, scalar, u);
    TEST_ASSERT_EQUAL_MEMORY(expected, shared, 32);
}

void test_identity_node_id_address_class() {
    const Identity a = makeIdentity(0x11);
    const Identity b = makeIdentity(0x11);
    const Identity c = makeIdentity(0x22);
    TEST_ASSERT_TRUE(a.publicIdentity().isSelfConsistent());
    TEST_ASSERT_EQUAL_MEMORY(a.publicIdentity().sign_pk, b.publicIdentity().sign_pk, 32);
    TEST_ASSERT_TRUE(a.nodeId() == b.nodeId());
    TEST_ASSERT_FALSE(a.nodeId() == c.nodeId());
    // Address-class rule: unicast node ids never have the high bit set.
    TEST_ASSERT_EQUAL_UINT8(0, a.nodeId().b[0] & 0x80);
    // Public identity serialises to exactly 72 bytes and round-trips.
    uint8_t bytes[kPublicIdentityLen];
    a.publicIdentity().toBytes(bytes);
    PublicIdentity parsed;
    TEST_ASSERT_TRUE(PublicIdentity::fromBytes(bytes, sizeof(bytes), parsed));
    TEST_ASSERT_TRUE(parsed == a.publicIdentity());
    // Direct-message keys agree in both directions and differ per peer pair.
    uint8_t k1[32];
    uint8_t k2[32];
    uint8_t k3[32];
    TEST_ASSERT_TRUE(a.dmKey(b.publicIdentity(), k1));
    TEST_ASSERT_TRUE(b.dmKey(a.publicIdentity(), k2));
    TEST_ASSERT_EQUAL_MEMORY(k1, k2, 32);
    TEST_ASSERT_TRUE(a.dmKey(c.publicIdentity(), k3));
    TEST_ASSERT_FALSE(std::memcmp(k1, k3, 32) == 0);
}

void test_channel_key_and_id() {
    uint8_t psk[32];
    std::memset(psk, 0x42, sizeof(psk));
    ChannelKey one;
    ChannelKey two;
    ChannelKey other;
    TEST_ASSERT_TRUE(ChannelKey::derive(psk, "family", 6, one));
    TEST_ASSERT_TRUE(ChannelKey::derive(psk, "family", 6, two));
    TEST_ASSERT_TRUE(ChannelKey::derive(psk, "friends", 7, other));
    TEST_ASSERT_EQUAL_MEMORY(one.key, two.key, 32);
    TEST_ASSERT_TRUE(one.id == two.id);
    TEST_ASSERT_FALSE(one.id == other.id);
    // Address-class rule: channel ids always have the high bit set.
    TEST_ASSERT_TRUE((one.id.b[0] & 0x80) != 0);
    TEST_ASSERT_TRUE(isChannelAddress(one.id));
}

void test_seed_at_rest_roundtrip() {
    RngState rng;
    uint8_t seed[32];
    uint8_t device[32];
    std::memset(seed, 0x5A, sizeof(seed));
    std::memset(device, 0xAB, sizeof(device));
    uint8_t sealed[kSealedSeedLen];
    TEST_ASSERT_TRUE(sealSeedAtRest(seed, device, testRng, &rng, sealed));
    uint8_t opened[32];
    TEST_ASSERT_TRUE(openSeedAtRest(sealed, sizeof(sealed), device, opened));
    TEST_ASSERT_EQUAL_MEMORY(seed, opened, 32);
    sealed[30] ^= 0xFF;
    TEST_ASSERT_FALSE(openSeedAtRest(sealed, sizeof(sealed), device, opened));
}

// --- Frame codec -----------------------------------------------------------------------------
void test_frame_known_bytes() {
    Frame frame;
    frame.type = FrameType::Data;
    frame.flags = kFlagEncrypted | kFlagAckRequest;
    frame.ttl = 5;
    TEST_ASSERT_TRUE(NodeId::fromHex("0102030405060708", frame.src));
    TEST_ASSERT_TRUE(NodeId::fromHex("1112131415161718", frame.dst));
    frame.seq = 0xDEADBEEF;
    const uint8_t body[] = {1, 2, 3, 4, 5};
    std::memcpy(frame.body, body, sizeof(body));
    frame.body_len = sizeof(body);
    uint8_t out[kMtu];
    const size_t len = frame.encode(out, sizeof(out));
    TEST_ASSERT_EQUAL_UINT32(27, (unsigned)len);
    TEST_ASSERT_EQUAL_STRING("215301020304050607081112131415161718deadbeef0102030405",
                             bytesToHex(out, len).c_str());
    Frame decoded;
    TEST_ASSERT_TRUE(decodeFrame(out, len, decoded).ok());
    TEST_ASSERT_TRUE(decoded.type == FrameType::Data);
    TEST_ASSERT_EQUAL_UINT8(5, decoded.ttl);
    TEST_ASSERT_EQUAL_UINT8(frame.flags, decoded.flags);
    TEST_ASSERT_TRUE(decoded.src == frame.src);
    TEST_ASSERT_TRUE(decoded.dst == frame.dst);
    TEST_ASSERT_EQUAL_UINT32(frame.seq, decoded.seq);
    TEST_ASSERT_EQUAL_MEMORY(frame.body, decoded.body, frame.body_len);
}

void test_frame_aad_ignores_ttl() {
    Frame frame;
    frame.type = FrameType::Announce;
    frame.flags = 0;
    frame.ttl = 5;
    TEST_ASSERT_TRUE(NodeId::fromHex("0102030405060708", frame.src));
    frame.dst = NodeId::broadcast();
    frame.seq = 0x01020304;
    uint8_t aad1[kHeaderLen];
    frame.aad(aad1);
    const std::string aad1_hex = bytesToHex(aad1, sizeof(aad1));
    frame.ttl = 9;
    uint8_t aad2[kHeaderLen];
    frame.aad(aad2);
    TEST_ASSERT_EQUAL_STRING(aad1_hex.c_str(), bytesToHex(aad2, sizeof(aad2)).c_str());
    // Byte 1 contains flags only (TTL nibble zeroed).
    TEST_ASSERT_EQUAL_UINT8(0, aad1[1] >> 4);
    // Changing any other authenticated field changes the AAD.
    frame.flags = kFlagPriority;
    frame.aad(aad2);
    TEST_ASSERT_FALSE(aad1_hex == bytesToHex(aad2, sizeof(aad2)));
}

void test_frame_decode_rejects() {
    Frame out;
    uint8_t buf[kMtu + 1];
    std::memset(buf, 0, sizeof(buf));
    TEST_ASSERT_TRUE(decodeFrame(buf, 4, out).error == DecodeError::TooShort);
    buf[0] = (uint8_t)(2 << 5);  // version 2
    TEST_ASSERT_TRUE(decodeFrame(buf, kHeaderLen, out).error == DecodeError::BadVersion);
    buf[0] = (uint8_t)((kProtoVersion << 5) | 11);  // reserved frame type
    TEST_ASSERT_TRUE(decodeFrame(buf, kHeaderLen, out).shouldIgnore());
    buf[0] = (uint8_t)((kProtoVersion << 5) | (uint8_t)FrameType::Data);
    buf[1] = 0x08;  // reserved header flag bit
    TEST_ASSERT_TRUE(decodeFrame(buf, kHeaderLen, out).error == DecodeError::Invalid);
    buf[1] = 0;
    TEST_ASSERT_TRUE(decodeFrame(buf, kMtu + 1, out).error == DecodeError::TooLong);
}

void test_frame_relay_ttl() {
    Frame frame;
    frame.type = FrameType::Data;
    frame.ttl = 3;
    frame.seq = 7;
    Frame relay;
    TEST_ASSERT_TRUE(frame.relayCopy(relay));
    TEST_ASSERT_EQUAL_UINT8(2, relay.ttl);
    TEST_ASSERT_EQUAL_UINT32(frame.seq, relay.seq);
    frame.ttl = 0;
    TEST_ASSERT_FALSE(frame.relayCopy(relay));
}

void test_sealed_body_layout() {
    uint8_t body[220];
    std::memset(body, 0, sizeof(body));
    // flags=0, msg_ref, sealed = 12+16 bytes.
    body[0] = 0;
    writeU32Be(body + 1, 0xAABBCCDD);
    SealedBodyView view;
    TEST_ASSERT_TRUE(SealedBodyView::parse(body, 1 + 4 + kSealOverhead, view));
    TEST_ASSERT_EQUAL_UINT8(0, view.payload_flags);
    TEST_ASSERT_EQUAL_UINT32(0xAABBCCDD, view.msg_ref);
    TEST_ASSERT_TRUE(view.identity == nullptr);
    TEST_ASSERT_EQUAL_UINT32(kSealOverhead, (unsigned)view.sealed_len);
    // IDENTITY_INCLUDED shifts the sealed payload by 72 bytes.
    body[0] = kPayloadFlagIdentityIncluded;
    SealedBodyView with_identity;
    TEST_ASSERT_TRUE(
        SealedBodyView::parse(body, 1 + 4 + kPublicIdentityLen + kSealOverhead, with_identity));
    TEST_ASSERT_TRUE(with_identity.identity != nullptr);
    TEST_ASSERT_EQUAL_UINT32(kSealOverhead, (unsigned)with_identity.sealed_len);
    // Reserved payload flag bits are rejected.
    body[0] = 0x08;
    TEST_ASSERT_FALSE(SealedBodyView::parse(body, 1 + 4 + kSealOverhead, view));
    // Too short.
    TEST_ASSERT_FALSE(SealedBodyView::parse(body, 10, view));
}

// --- Announce -------------------------------------------------------------------------------
void test_announce_roundtrip_and_verify() {
    const Identity id = makeIdentity(0x33);
    Announce announce;
    announce.cls = NodeClass::Tracker;
    announce.caps = kCapGps | kCapUsb | kCapBle;
    announce.setName("ridge-tracker", 13);
    announce.interval_s = 1800;
    announce.timestamp_ms = 1700000000123ull;
    announce.ident = id.publicIdentity();
    uint8_t unsigned_body[kAnnounceBodyLen];
    const size_t unsigned_len = encodeAnnounceUnsigned(announce, unsigned_body,
                                                       sizeof(unsigned_body));
    TEST_ASSERT_EQUAL_UINT32(88 + 13, (unsigned)unsigned_len);
    // Byte layout pinned by PROTOCOL.md §6.
    TEST_ASSERT_EQUAL_UINT8(1, unsigned_body[0]);
    TEST_ASSERT_EQUAL_UINT8((uint8_t)NodeClass::Tracker, unsigned_body[1]);
    TEST_ASSERT_EQUAL_UINT8(0, unsigned_body[4]);  // reserved
    TEST_ASSERT_EQUAL_UINT8(13, unsigned_body[5]);
    const uint32_t seq = 0x11223344;
    uint8_t payload[kMtu];
    const size_t payload_len = announceSignaturePayload(id.nodeId(), seq, unsigned_body,
                                                         unsigned_len, payload, sizeof(payload));
    TEST_ASSERT_TRUE(payload_len > 0);
    uint8_t signature[kSignatureLen];
    TEST_ASSERT_TRUE(id.sign(payload, payload_len, signature));
    Frame frame;
    frame.type = FrameType::Announce;
    frame.ttl = kAnnounceTtl;
    frame.flags = 0;
    frame.src = id.nodeId();
    frame.dst = NodeId::broadcast();
    frame.seq = seq;
    frame.body_len = (uint16_t)encodeAnnounceBody(announce, signature, frame.body,
                                                  sizeof(frame.body));
    TEST_ASSERT_EQUAL_UINT32(152 + 13, (unsigned)frame.body_len);

    Announce parsed;
    TEST_ASSERT_TRUE(verifyAnnounce(frame, parsed));
    TEST_ASSERT_TRUE(parsed.cls == NodeClass::Tracker);
    TEST_ASSERT_EQUAL_UINT16(announce.caps, parsed.caps);
    TEST_ASSERT_EQUAL_STRING("ridge-tracker", parsed.name);
    TEST_ASSERT_EQUAL_UINT64(announce.timestamp_ms, parsed.timestamp_ms);
    TEST_ASSERT_TRUE(parsed.ident == id.publicIdentity());
}

void test_announce_rejects_tampering() {
    const Identity id = makeIdentity(0x44);
    Announce announce;
    announce.cls = NodeClass::Repeater;
    announce.setName("relay", 5);
    announce.interval_s = 3600;
    announce.ident = id.publicIdentity();
    uint8_t unsigned_body[kAnnounceBodyLen];
    const size_t unsigned_len = encodeAnnounceUnsigned(announce, unsigned_body,
                                                       sizeof(unsigned_body));
    uint8_t payload[kMtu];
    const size_t payload_len = announceSignaturePayload(id.nodeId(), 5, unsigned_body,
                                                         unsigned_len, payload, sizeof(payload));
    uint8_t signature[kSignatureLen];
    TEST_ASSERT_TRUE(id.sign(payload, payload_len, signature));
    Frame frame;
    frame.type = FrameType::Announce;
    frame.ttl = kAnnounceTtl;
    frame.src = id.nodeId();
    frame.dst = NodeId::broadcast();
    frame.seq = 5;
    frame.body_len = (uint16_t)encodeAnnounceBody(announce, signature, frame.body,
                                                  sizeof(frame.body));

    Announce parsed;
    TEST_ASSERT_TRUE(verifyAnnounce(frame, parsed));
    // Tampered signed byte.
    Frame tampered = frame;
    tampered.body[kAnnounceFixedLen] ^= 0x01;
    TEST_ASSERT_FALSE(verifyAnnounce(tampered, parsed));
    // Tampered signature.
    tampered = frame;
    tampered.body[frame.body_len - 1] ^= 0x01;
    TEST_ASSERT_FALSE(verifyAnnounce(tampered, parsed));
    // Frame source does not match the announced identity.
    tampered = frame;
    tampered.src.b[3] ^= 0x01;
    TEST_ASSERT_FALSE(verifyAnnounce(tampered, parsed));
    // TTL above the announce origin value is impossible.
    tampered = frame;
    tampered.ttl = 9;
    TEST_ASSERT_FALSE(verifyAnnounce(tampered, parsed));
    // Reserved byte must be zero.
    tampered = frame;
    tampered.body[4] = 1;
    TEST_ASSERT_FALSE(verifyAnnounce(tampered, parsed));
}

void test_announce_hops_from_ttl() {
    TEST_ASSERT_EQUAL_UINT8(0, announceHops(8));
    TEST_ASSERT_EQUAL_UINT8(3, announceHops(5));
    TEST_ASSERT_EQUAL_UINT8(8, announceHops(0));
    TEST_ASSERT_EQUAL_UINT8(0, announceHops(9));  // clamped
}

// --- Messages -------------------------------------------------------------------------------
void test_app_message_cbor_roundtrip() {
    AppMessage message;
    message.id = 0xDEADBEEF;
    message.kind = MessageKind::Text;
    message.sent_at_ms = 1700000000000ull;
    message.has_text = true;
    message.text.len = std::strlen("hello mesh");
    std::memcpy(message.text.data, "hello mesh", message.text.len);
    message.has_reply_to = true;
    message.reply_to = 42;
    uint8_t cbor[kMaxBody];
    const size_t len = encodeAppMessage(message, cbor, sizeof(cbor));
    TEST_ASSERT_TRUE(len > 0);
    // Deterministic minimal-integer CBOR: map(6) starts with 0xa6.
    TEST_ASSERT_EQUAL_HEX8(0xA5, cbor[0]);  // 5 fields, no optionals
    AppMessage decoded;
    TEST_ASSERT_TRUE(decodeAppMessage(cbor, len, decoded));
    TEST_ASSERT_EQUAL_UINT32(message.id, decoded.id);
    TEST_ASSERT_TRUE(decoded.kind == MessageKind::Text);
    TEST_ASSERT_EQUAL_UINT64(message.sent_at_ms, decoded.sent_at_ms);
    TEST_ASSERT_EQUAL_STRING("hello mesh", decoded.text.data);
    TEST_ASSERT_EQUAL_UINT32(42, decoded.reply_to);

    // Position variant.
    message = AppMessage{};
    message.id = 7;
    message.kind = MessageKind::Position;
    message.has_position = true;
    message.position.lat_e7 = 599137000;
    message.position.lon_e7 = 107522000;
    message.position.has_alt = true;
    message.position.alt_m = 45;
    message.position.timestamp_ms = 123;
    const size_t len2 = encodeAppMessage(message, cbor, sizeof(cbor));
    TEST_ASSERT_TRUE(len2 > 0);
    AppMessage decoded2;
    TEST_ASSERT_TRUE(decodeAppMessage(cbor, len2, decoded2));
    TEST_ASSERT_TRUE(decoded2.has_position);
    TEST_ASSERT_EQUAL_INT(599137000, decoded2.position.lat_e7);
    TEST_ASSERT_EQUAL_INT(45, decoded2.position.alt_m);
}

void test_ack_cbor_roundtrip() {
    Ack ack;
    ack.kind = AckKind::Delivered;
    ack.msg_id = 0xCAFEBABE;
    ack.timestamp_ms = 1700000000000ull;
    uint8_t cbor[128];
    const size_t len = encodeAck(ack, cbor, sizeof(cbor));
    TEST_ASSERT_TRUE(len > 0);
    Ack decoded;
    TEST_ASSERT_TRUE(decodeAck(cbor, len, decoded));
    TEST_ASSERT_TRUE(decoded.kind == AckKind::Delivered);
    TEST_ASSERT_EQUAL_UINT32(ack.msg_id, decoded.msg_id);
    TEST_ASSERT_FALSE(decoded.has_res_id);

    ack.kind = AckKind::ResourceWindow;
    ack.has_res_id = true;
    ack.res_id = 5;
    ack.has_next_expected = true;
    ack.next_expected = 16;
    ack.has_bitmap = true;
    ack.bitmap = 0x00FF00FF;
    const size_t len2 = encodeAck(ack, cbor, sizeof(cbor));
    TEST_ASSERT_TRUE(decodeAck(cbor, len2, decoded));
    TEST_ASSERT_TRUE(decoded.kind == AckKind::ResourceWindow);
    TEST_ASSERT_EQUAL_UINT32(5, decoded.res_id);
    TEST_ASSERT_EQUAL_UINT16(16, decoded.next_expected);
    TEST_ASSERT_EQUAL_UINT32(0x00FF00FF, decoded.bitmap);
}

void test_manifest_cbor_roundtrip() {
    ResourceManifest manifest;
    manifest.res_id = 7;
    manifest.msg_id = 9;
    manifest.kind = ResourceKind::LongText;
    manifest.mime_len = 10;
    std::memcpy(manifest.mime, "text/plain", 10);
    manifest.total_len = 5000;
    manifest.chunk_bytes = 180;
    manifest.chunk_count = 28;
    for (size_t i = 0; i < sizeof(manifest.sha256); ++i) manifest.sha256[i] = (uint8_t)i;
    uint8_t cbor[256];
    const size_t len = encodeResourceManifest(manifest, cbor, sizeof(cbor));
    TEST_ASSERT_TRUE(len > 0);
    ResourceManifest decoded;
    TEST_ASSERT_TRUE(decodeResourceManifest(cbor, len, decoded));
    TEST_ASSERT_EQUAL_UINT32(7, decoded.res_id);
    TEST_ASSERT_TRUE(decoded.kind == ResourceKind::LongText);
    TEST_ASSERT_EQUAL_STRING("text/plain", decoded.mime);
    TEST_ASSERT_EQUAL_UINT32(5000, decoded.total_len);
    TEST_ASSERT_EQUAL_UINT16(28, decoded.chunk_count);
    TEST_ASSERT_EQUAL_MEMORY(manifest.sha256, decoded.sha256, 32);
}

void test_store_offer_want_cbor() {
    StoreOffer offer;
    offer.count = 3;
    offer.ids[0] = 1;
    offer.ids[1] = 0xDEADBEEF;
    offer.ids[2] = 42;
    uint8_t cbor[128];
    const size_t len = encodeStoreOffer(offer, cbor, sizeof(cbor));
    TEST_ASSERT_TRUE(len > 0);
    StoreOffer decoded_offer;
    TEST_ASSERT_TRUE(decodeStoreOffer(cbor, len, decoded_offer));
    TEST_ASSERT_EQUAL_UINT8(3, decoded_offer.count);
    TEST_ASSERT_EQUAL_UINT32(0xDEADBEEF, decoded_offer.ids[1]);

    StoreWant want;
    want.count = 2;
    want.ids[0] = 7;
    want.ids[1] = 8;
    const size_t len2 = encodeStoreWant(want, cbor, sizeof(cbor));
    TEST_ASSERT_TRUE(len2 > 0);
    StoreWant decoded_want;
    TEST_ASSERT_TRUE(decodeStoreWant(cbor, len2, decoded_want));
    TEST_ASSERT_EQUAL_UINT8(2, decoded_want.count);
    TEST_ASSERT_EQUAL_UINT32(8, decoded_want.ids[1]);
}

void test_group_message_roundtrip() {
    const Identity sender = makeIdentity(0x55);
    uint8_t psk[32];
    std::memset(psk, 0x42, sizeof(psk));
    ChannelKey channel;
    TEST_ASSERT_TRUE(ChannelKey::derive(psk, "family", 6, channel));
    AppMessage message;
    message.id = 77;
    message.kind = MessageKind::Text;
    message.has_text = true;
    message.text.len = 2;
    std::memcpy(message.text.data, "hi", 2);
    uint8_t message_cbor[256];
    const size_t message_len = encodeAppMessage(message, message_cbor, sizeof(message_cbor));
    uint8_t payload[256];
    const size_t payload_len = groupSignaturePayload(sender.nodeId(), channel.id, message_cbor,
                                                     message_len, payload, sizeof(payload));
    uint8_t signature[kSignatureLen];
    TEST_ASSERT_TRUE(sender.sign(payload, payload_len, signature));
    uint8_t cbor[512];
    const size_t cbor_len = encodeGroupMessage(sender.nodeId(), signature, message, cbor,
                                               sizeof(cbor));
    TEST_ASSERT_TRUE(cbor_len > 0);
    GroupMessageView view;
    TEST_ASSERT_TRUE(decodeGroupMessage(cbor, cbor_len, view));
    TEST_ASSERT_TRUE(view.sender == sender.nodeId());
    TEST_ASSERT_EQUAL_UINT32(kSignatureLen, (unsigned)view.signature_len);
    TEST_ASSERT_TRUE(verifyGroupMessage(view.signature, view.sender, channel.id,
                                        view.message_cbor, view.message_cbor_len,
                                        sender.publicIdentity().sign_pk));
    // Wrong channel id breaks verification.
    NodeId other_channel = channel.id;
    other_channel.b[7] ^= 0x01;
    TEST_ASSERT_FALSE(verifyGroupMessage(view.signature, view.sender, other_channel,
                                         view.message_cbor, view.message_cbor_len,
                                         sender.publicIdentity().sign_pk));
    AppMessage decoded;
    TEST_ASSERT_TRUE(decodeAppMessage(view.message_cbor, view.message_cbor_len, decoded));
    TEST_ASSERT_EQUAL_STRING("hi", decoded.text.data);
}

// --- Services -------------------------------------------------------------------------------
void test_dedup_cache() {
    DedupCache cache(60000);
    const NodeId src = makeIdentity(0x66).nodeId();
    TEST_ASSERT_TRUE(cache.checkAndInsert(src, 1, 0));
    TEST_ASSERT_FALSE(cache.checkAndInsert(src, 1, 100));
    TEST_ASSERT_TRUE(cache.checkAndInsert(src, 2, 200));
    // Expiry after the TTL.
    TEST_ASSERT_TRUE(cache.checkAndInsert(src, 1, 61000));
    // Ring capacity: fill 2048 unique entries, the first one is then evicted.
    cache.clear();
    for (uint32_t i = 0; i < (uint32_t)DedupCache::kCapacity; ++i) {
        TEST_ASSERT_TRUE(cache.checkAndInsert(src, 1000 + i, 100000));
    }
    TEST_ASSERT_FALSE(cache.checkAndInsert(src, 1000, 100000));
    TEST_ASSERT_TRUE(cache.checkAndInsert(src, 999999, 100000));  // wraps the ring
    TEST_ASSERT_TRUE(cache.checkAndInsert(src, 1000, 100000));    // evicted earlier
}

void test_store_queue() {
    StoreQueue store;
    const Identity sender = makeIdentity(0x77);
    NodeId dst;
    TEST_ASSERT_TRUE(NodeId::fromHex("0001020304050607", dst));
    dst.b[0] &= 0x7F;
    const NodeId me = makeIdentity(0x78).nodeId();
    Frame frame = makeStoredCandidate(sender, dst, 0x11111111, 1);
    TEST_ASSERT_TRUE(store.retain(frame, me, 0x11111111, 1000));
    TEST_ASSERT_TRUE(store.contains(dst, 0x11111111));
    TEST_ASSERT_FALSE(store.retain(frame, me, 0x11111111, 1000));  // duplicate
    uint32_t ids[4];
    TEST_ASSERT_EQUAL_UINT32(1, (unsigned)store.idsFor(dst, ids, 4));
    TEST_ASSERT_EQUAL_UINT32(0x11111111, ids[0]);
    // Channel-addressed, broadcast and non-storable frames are never retained.
    Frame channel_frame = frame;
    channel_frame.dst.b[0] |= 0x80;
    TEST_ASSERT_FALSE(store.retain(channel_frame, me, 0x22222222, 1000));
    Frame broadcast = frame;
    broadcast.dst = NodeId::broadcast();
    TEST_ASSERT_FALSE(store.retain(broadcast, me, 0x33333333, 1000));
    Frame announce;
    announce.type = FrameType::Announce;
    announce.dst = dst;
    TEST_ASSERT_FALSE(store.retain(announce, me, 0x44444444, 1000));
    // Taking removes it.
    StoredEnvelope env;
    TEST_ASSERT_TRUE(store.take(dst, 0x11111111, env));
    TEST_ASSERT_FALSE(store.take(dst, 0x11111111, env));
    TEST_ASSERT_EQUAL_UINT32(0, (unsigned)store.size());
    // TTL expiry.
    TEST_ASSERT_TRUE(store.retain(frame, me, 0x55555555, 1000));
    store.prune(1000 + kStoreTtlMs + 1);
    TEST_ASSERT_EQUAL_UINT32(0, (unsigned)store.size());
}

void test_store_serialization() {
    StoreQueue store;
    const Identity sender = makeIdentity(0x79);
    NodeId dst;
    TEST_ASSERT_TRUE(NodeId::fromHex("1011121314151617", dst));
    dst.b[0] &= 0x7F;
    const NodeId me = makeIdentity(0x7A).nodeId();
    Frame frame = makeStoredCandidate(sender, dst, 0xCAFEBABE, 9);
    TEST_ASSERT_TRUE(store.retain(frame, me, 0xCAFEBABE, 1000));
    static std::vector<uint8_t> serialized(StoreQueue::kSerializedSize);
    const size_t len = store.serialize(serialized.data(), serialized.size());
    TEST_ASSERT_EQUAL_UINT32(StoreQueue::kSerializedSize, (unsigned)len);
    StoreQueue restored;
    TEST_ASSERT_TRUE(restored.deserialize(serialized.data(), len, 2000));
    TEST_ASSERT_EQUAL_UINT32(store.size(), (unsigned)restored.size());
    TEST_ASSERT_TRUE(restored.contains(dst, 0xCAFEBABE));
    // Corrupt one record: it is skipped instead of crashing.
    serialized[20] ^= 0xFF;
    StoreQueue corrupted;
    TEST_ASSERT_TRUE(corrupted.deserialize(serialized.data(), len, 2000));
}

void test_airtime_and_budget() {
    const uint32_t airtime = loraAirtimeMs(kMediumFast, 240);
    TEST_ASSERT_TRUE(airtime >= 450 && airtime <= 900);
    TEST_ASSERT_TRUE(loraAirtimeMs(kLongSlow, 240) > loraAirtimeMs(kShortFast, 240));
    TEST_ASSERT_TRUE(loraAirtimeMs(kMediumFast, 40) < loraAirtimeMs(kMediumFast, 240));
    AirtimeBudget budget;
    budget.configure(kRegionEu868);
    TEST_ASSERT_TRUE(budget.enabled());
    TEST_ASSERT_EQUAL_INT(10, (int)budget.dutyPct());
    budget.reset(0);
    // v2 legal EU868 duty is 10 %: 360 s of airtime per hour.
    TEST_ASSERT_EQUAL_UINT32(360000, budget.capacityMs());
    TEST_ASSERT_TRUE(budget.tryConsume(360000, 0));
    TEST_ASSERT_FALSE(budget.tryConsume(1, 0));
    // Refill: 100 ms of airtime budget per second of wall clock at 10 %.
    TEST_ASSERT_TRUE(budget.tryConsume(10000, 100000));
    TEST_ASSERT_FALSE(budget.tryConsume(1, 100000));
    AirtimeBudget unlimited;
    unlimited.configure(kRegionUs915);
    TEST_ASSERT_FALSE(unlimited.enabled());
    TEST_ASSERT_TRUE(unlimited.tryConsume(1000000, 0));
    // Origination rate limit (ACKs bypass it).
    OriginateLimiter limiter;
    limiter.reset(0);
    for (uint32_t i = 0; i < kMaxOriginatePerMin; ++i) {
        TEST_ASSERT_TRUE(limiter.allow(10, false));
    }
    TEST_ASSERT_FALSE(limiter.allow(20, false));
    TEST_ASSERT_TRUE(limiter.allow(30, true));
    TEST_ASSERT_TRUE(limiter.allow(61000, false));
}

void test_gps_nmea() {
    NmeaParser parser;
    const std::string gga = nmeaWithChecksum(
        "GPGGA,123519,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,");
    uint64_t now = 5000;
    bool parsed = false;
    for (char c : gga) parsed = parser.feed(c, now) || parsed;
    TEST_ASSERT_TRUE(parsed);
    TEST_ASSERT_TRUE(parser.fix().valid);
    TEST_ASSERT_TRUE(parser.fix().has_position);
    TEST_ASSERT_EQUAL_INT(481173000, parser.fix().lat_e7);
    TEST_ASSERT_EQUAL_INT(115166667, parser.fix().lon_e7);
    TEST_ASSERT_EQUAL_UINT8(8, parser.fix().satellites);
    TEST_ASSERT_TRUE(parser.fix().has_alt);
    TEST_ASSERT_EQUAL_INT(545, parser.fix().alt_m);
    TEST_ASSERT_TRUE(parser.fix().has_accuracy);

    // RMC sets speed/heading and position.
    const std::string rmc = nmeaWithChecksum(
        "GPRMC,123520,A,4807.038,N,01131.000,E,022.4,084.4,230394,003.1,W");
    for (char c : rmc) parsed = parser.feed(c, ++now) || parsed;
    TEST_ASSERT_TRUE(parser.fix().has_speed);
    TEST_ASSERT_TRUE(parser.fix().has_heading);
    TEST_ASSERT_EQUAL_UINT16(84, parser.fix().heading_deg);

    // Bad checksum is counted and ignored.
    const uint32_t before = parser.fix().checksum_errors;
    const std::string bad = "$GPGGA,123519,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,*00\r\n";
    for (char c : bad) parser.feed(c, now);
    TEST_ASSERT_EQUAL_UINT32(before + 1, parser.fix().checksum_errors);

    // Void RMC (status V) must not declare a fix.
    NmeaParser void_parser;
    const std::string rmc_void = nmeaWithChecksum(
        "GPRMC,123520,V,4807.038,N,01131.000,E,022.4,084.4,230394,003.1,W");
    for (char c : rmc_void) void_parser.feed(c, now);
    TEST_ASSERT_FALSE(void_parser.fix().valid);
}

void test_battery_ocv() {
    TEST_ASSERT_EQUAL_UINT8(100, batteryPercentFromMv(4200, kOcvT1000E, kOcvT1000ECount));
    TEST_ASSERT_EQUAL_UINT8(0, batteryPercentFromMv(3000, kOcvT1000E, kOcvT1000ECount));
    const uint8_t a = batteryPercentFromMv(3820, kOcvT1000E, kOcvT1000ECount);
    const uint8_t b = batteryPercentFromMv(3776, kOcvT1000E, kOcvT1000ECount);
    TEST_ASSERT_TRUE(a > b);
    TEST_ASSERT_TRUE(b < 100);
    BatteryReading reading;
    reading.valid = true;
    reading.battery_mv = 3300;
    const PowerPolicy policy = evaluatePower(reading, true, true);
    TEST_ASSERT_TRUE(policy.low_power);
    TEST_ASSERT_FALSE(policy.position_sharing);
}

void test_companion_chunk_roundtrip() {
    CompanionMsg hello;
    hello.type = CompanionType::Hello;
    hello.proto = 1;
    std::snprintf(hello.app_version, sizeof(hello.app_version), "openmesh-android/0.1.0");
    hello.wants_events = true;
    uint8_t chunk[2048];
    const size_t len = encodeCompanionChunk(hello, chunk, sizeof(chunk));
    TEST_ASSERT_TRUE(len > 0);
    CompanionMsg decoded;
    TEST_ASSERT_TRUE(decodeCompanionChunk(chunk, len, decoded));
    TEST_ASSERT_TRUE(decoded.type == CompanionType::Hello);
    TEST_ASSERT_EQUAL_UINT8(1, decoded.proto);
    TEST_ASSERT_EQUAL_STRING("openmesh-android/0.1.0", decoded.app_version);
    TEST_ASSERT_TRUE(decoded.wants_events);

    // Rx message carries a frame, rssi/snr, link and timestamp.
    CompanionMsg rx;
    rx.type = CompanionType::Rx;
    rx.frame_len = 27;
    for (uint16_t i = 0; i < rx.frame_len; ++i) rx.frame[i] = (uint8_t)(i * 3);
    rx.rssi = -87;
    rx.snr = 9;
    std::snprintf(rx.link, sizeof(rx.link), "lora");
    rx.at_ms = 1700000000000ull;
    const size_t len2 = encodeCompanionChunk(rx, chunk, sizeof(chunk));
    CompanionMsg decoded_rx;
    TEST_ASSERT_TRUE(decodeCompanionChunk(chunk, len2, decoded_rx));
    TEST_ASSERT_EQUAL_UINT16(27, decoded_rx.frame_len);
    TEST_ASSERT_EQUAL_MEMORY(rx.frame, decoded_rx.frame, 27);
    TEST_ASSERT_EQUAL_INT(-87, decoded_rx.rssi);
    TEST_ASSERT_EQUAL_INT(9, decoded_rx.snr);
    TEST_ASSERT_EQUAL_STRING("lora", decoded_rx.link);
    TEST_ASSERT_EQUAL_UINT64(1700000000000ull, decoded_rx.at_ms);

    // Stats entries use numeric values.
    CompanionMsg stats;
    stats.type = CompanionType::Stats;
    stats.entry_count = 1;
    std::snprintf(stats.entries[0].key, sizeof(stats.entries[0].key), "rx_frames");
    std::snprintf(stats.entries[0].value, sizeof(stats.entries[0].value), "123456789");
    const size_t len3 = encodeCompanionChunk(stats, chunk, sizeof(chunk));
    CompanionMsg decoded_stats;
    TEST_ASSERT_TRUE(decodeCompanionChunk(chunk, len3, decoded_stats));
    TEST_ASSERT_EQUAL_UINT8(1, decoded_stats.entry_count);
    TEST_ASSERT_EQUAL_STRING("123456789", decoded_stats.entries[0].value);
}

void test_companion_slip_escaping() {
    CompanionMsg tx;
    tx.type = CompanionType::Tx;
    tx.frame_len = 8;
    const uint8_t tricky[] = {0xC0, 0xDB, 0x00, 0xC0, 0xDB, 0xDC, 0xDD, 0x7F};
    std::memcpy(tx.frame, tricky, sizeof(tricky));
    uint8_t chunk[2048];
    const size_t chunk_len = encodeCompanionChunk(tx, chunk, sizeof(chunk));
    TEST_ASSERT_TRUE(chunk_len > 0);
    uint8_t slip[4096];
    const size_t slip_len = slipEncode(chunk, chunk_len, slip, sizeof(slip));
    TEST_ASSERT_TRUE(slip_len > 0);
    TEST_ASSERT_EQUAL_HEX8(kSlipEnd, slip[0]);
    TEST_ASSERT_EQUAL_HEX8(kSlipEnd, slip[slip_len - 1]);
    // No unescaped 0xC0 inside the frame.
    for (size_t i = 1; i + 1 < slip_len; ++i) {
        TEST_ASSERT_TRUE(slip[i] != kSlipEnd);
    }
    SlipDecoder decoder;
    bool got = false;
    for (size_t i = 0; i < slip_len; ++i) got = decoder.feed(slip[i]) || got;
    TEST_ASSERT_TRUE(got);
    TEST_ASSERT_EQUAL_UINT32(1, decoder.framesDecoded());
    TEST_ASSERT_TRUE(decoder.frameValid());
    TEST_ASSERT_EQUAL_UINT32(chunk_len, (unsigned)decoder.chunkLen());
    TEST_ASSERT_EQUAL_MEMORY(chunk, decoder.chunk(), chunk_len);
    CompanionMsg decoded;
    TEST_ASSERT_TRUE(decodeCompanionChunk(decoder.chunk(), decoder.chunkLen(), decoded));
    TEST_ASSERT_EQUAL_MEMORY(tricky, decoded.frame, sizeof(tricky));
}

void test_companion_crc_rejects_corruption() {
    CompanionMsg ping;
    ping.type = CompanionType::Ping;
    ping.nonce = 0x12345678;
    uint8_t chunk[2048];
    const size_t len = encodeCompanionChunk(ping, chunk, sizeof(chunk));
    TEST_ASSERT_TRUE(len > 6);
    CompanionMsg decoded;
    TEST_ASSERT_TRUE(decodeCompanionChunk(chunk, len, decoded));
    chunk[3] ^= 0x01;  // corrupt the length field -> CRC and length checks fail
    TEST_ASSERT_FALSE(decodeCompanionChunk(chunk, len, decoded));
    chunk[3] ^= 0x01;
    chunk[len - 1] ^= 0xFF;  // corrupt CRC
    TEST_ASSERT_FALSE(decodeCompanionChunk(chunk, len, decoded));
}

void test_resource_blob_roundtrip() {
    RngState rng;
    uint8_t key[32];
    std::memset(key, 0x99, sizeof(key));
    const char* plaintext = "resource payload bytes";
    const size_t plaintext_len = std::strlen(plaintext);
    // total_len covers the whole sealed blob: flags(1) + nonce(12) + ciphertext + tag(16).
    const uint32_t total_len = (uint32_t)(1 + kNoncePrefixLen + plaintext_len + kAeadTagLen);
    uint8_t blob[512];
    const size_t blob_len = sealResourceBlob(testRng, &rng, key, 7, 9, total_len, 180,
                                             (const uint8_t*)plaintext, plaintext_len, blob,
                                             sizeof(blob));
    TEST_ASSERT_EQUAL_UINT32(total_len, (unsigned)blob_len);
    TEST_ASSERT_EQUAL_UINT8(0, blob[0]);
    // AAD binding: 20-byte domain + 14 bytes of manifest parameters; no sha256 (circular).
    uint8_t aad[kResourceAadLen];
    resourceAad(7, 9, total_len, 180, aad);
    TEST_ASSERT_EQUAL_UINT32(34, (unsigned)kResourceAadLen);
    TEST_ASSERT_EQUAL_MEMORY("openmesh/v1/resource", aad, 20);
    uint8_t out[512];
    size_t out_len = 0;
    TEST_ASSERT_TRUE(openResourceBlob(key, 7, 9, total_len, 180, blob, blob_len, out,
                                      sizeof(out), out_len));
    TEST_ASSERT_EQUAL_UINT32(plaintext_len, (unsigned)out_len);
    TEST_ASSERT_EQUAL_MEMORY(plaintext, out, plaintext_len);
    // Different manifest parameters break decryption (AAD mismatch).
    TEST_ASSERT_FALSE(openResourceBlob(key, 8, 9, total_len, 180, blob, blob_len, out,
                                       sizeof(out), out_len));
    blob[20] ^= 0xFF;
    TEST_ASSERT_FALSE(openResourceBlob(key, 7, 9, total_len, 180, blob, blob_len, out,
                                       sizeof(out), out_len));
}

// --- Engine integration ---------------------------------------------------------------------
void test_engine_announce_and_peer_book() {
    auto a = makeNode(0x11, true, false);
    auto b = makeNode(0x22, true, false);
    Frame announce;
    TEST_ASSERT_TRUE(a->engine->buildAnnounce(2000, announce));
    TEST_ASSERT_TRUE(announce.type == FrameType::Announce);
    TEST_ASSERT_EQUAL_UINT8(kAnnounceTtl, announce.ttl);
    TEST_ASSERT_TRUE(announce.isBroadcast());
    b->engine->onFrame(announce, Link::Lora, -50, 8, 2100);
    TEST_ASSERT_EQUAL_UINT32(1, (unsigned)b->engine->peerCount());
    const PeerInfo* peer = b->engine->findPeer(a->id.nodeId());
    TEST_ASSERT_NOT_NULL(peer);
    TEST_ASSERT_EQUAL_STRING("node-17", peer->name);
    TEST_ASSERT_TRUE(peer->ident == a->id.publicIdentity());
    TEST_ASSERT_EQUAL_UINT8(0, peer->hops);
    // The peer book entry is learned from the announce only after signature verification.
    Frame tampered = announce;
    tampered.body[kAnnounceFixedLen] ^= 0x01;
    const size_t before = b->engine->peerCount();
    b->engine->onFrame(tampered, Link::Lora, -50, 8, 2200);
    TEST_ASSERT_EQUAL_UINT32((unsigned)before, (unsigned)b->engine->peerCount());
}

void test_engine_direct_message_and_ack() {
    auto a = makeNode(0x31, true, false);
    auto b = makeNode(0x32, true, false);
    Frame a_announce;
    Frame b_announce;
    TEST_ASSERT_TRUE(a->engine->buildAnnounce(2000, a_announce));
    TEST_ASSERT_TRUE(b->engine->buildAnnounce(2000, b_announce));
    b->engine->onFrame(a_announce, Link::Lora, -50, 8, 2100);
    a->engine->onFrame(b_announce, Link::Lora, -51, 7, 2100);
    a->engine->outbox().clear();
    b->engine->outbox().clear();
    b->engine->events().clear();

    uint8_t plaintext[256];
    const size_t plaintext_len = makeAppMessageText(0xABCD1234, "hello b", plaintext,
                                                     sizeof(plaintext));
    TEST_ASSERT_TRUE(plaintext_len > 0);
    Frame frame = makeSealedDirectFrame(a->id, b->id.publicIdentity(), b->id.nodeId(), plaintext,
                                        plaintext_len, 0xABCD1234, 500,
                                        kFlagEncrypted | kFlagAckRequest, true);
    TEST_ASSERT_TRUE(frame.body_len > 0);
    b->engine->onFrame(frame, Link::Lora, -60, 6, 3000);
    const EngineEvent* event = nullptr;
    for (size_t i = 0; i < b->engine->events().size(); ++i) {
        if (b->engine->events().at(i).type == EventType::AppMessage)
            event = &b->engine->events().at(i);
    }
    TEST_ASSERT_NOT_NULL(event);
    if (event == nullptr) return;
    TEST_ASSERT_EQUAL_UINT32(0xABCD1234, event->message.id);
    TEST_ASSERT_EQUAL_STRING("hello b", event->message.text.data);
    // ACK_REQ produced a delivery ACK addressed back to A (B may also relay the frame).
    const Frame* ack = nullptr;
    for (size_t i = 0; i < b->engine->outbox().size(); ++i) {
        if (b->engine->outbox().at(i).frame.type == FrameType::Ack)
            ack = &b->engine->outbox().at(i).frame;
    }
    TEST_ASSERT_NOT_NULL(ack);
    if (ack == nullptr) return;
    TEST_ASSERT_TRUE(ack->dst == a->id.nodeId());
    TEST_ASSERT_TRUE(ack->hasFlag(kFlagPriority));
    TEST_ASSERT_FALSE(ack->hasFlag(kFlagAckRequest));
    a->engine->onFrame(*ack, Link::Lora, -61, 5, 3100);
    TEST_ASSERT_EQUAL_UINT32(1, a->engine->stats().acks_rx);
    TEST_ASSERT_EQUAL_UINT32(0, a->engine->stats().decrypt_errors);
}

void test_engine_relay_decrements_ttl() {
    auto a = makeNode(0x41, false, false);
    auto relay = makeNode(0x42, true, false);
    Frame announce;
    TEST_ASSERT_TRUE(a->engine->buildAnnounce(2000, announce));
    relay->engine->onFrame(announce, Link::Lora, -70, 4, 2100);
    TEST_ASSERT_EQUAL_UINT32(1, (unsigned)relay->engine->outbox().size());
    const Frame& forwarded = relay->engine->outbox().at(0).frame;
    TEST_ASSERT_EQUAL_UINT8(kAnnounceTtl - 1, forwarded.ttl);
    TEST_ASSERT_TRUE(forwarded.src == announce.src);
    TEST_ASSERT_EQUAL_MEMORY(announce.body, forwarded.body, announce.body_len);
    TEST_ASSERT_EQUAL_UINT32(1, relay->engine->stats().relayed);
    // Duplicates are not relayed twice.
    relay->engine->onFrame(announce, Link::Lora, -70, 4, 2150);
    TEST_ASSERT_EQUAL_UINT32(1, (unsigned)relay->engine->outbox().size());
    TEST_ASSERT_EQUAL_UINT32(1, relay->engine->stats().duplicates);
}

void test_engine_channel_position() {
    auto a = makeNode(0x51, false, false, true);
    auto b = makeNode(0x52, true, false, true);
    uint8_t psk[32];
    std::memset(psk, 0x42, sizeof(psk));
    a->engine->setChannel("family", psk);
    b->engine->setChannel("family", psk);
    Frame a_announce;
    Frame b_announce;
    TEST_ASSERT_TRUE(a->engine->buildAnnounce(2000, a_announce));
    TEST_ASSERT_TRUE(b->engine->buildAnnounce(2000, b_announce));
    b->engine->onFrame(a_announce, Link::Lora, -50, 8, 2100);
    a->engine->onFrame(b_announce, Link::Lora, -51, 7, 2100);
    b->engine->outbox().clear();
    b->engine->events().clear();

    // A short channel message fits in one signed GroupMessage frame and is addressed to the
    // channel id (address-class bit set).
    AppMessage text;
    text.id = 0x1234;
    text.kind = MessageKind::Text;
    text.sent_at_ms = 1700000000000ull;
    text.has_text = true;
    text.text.len = 2;
    std::memcpy(text.text.data, "hi", 2);
    Frame channel_frame;
    TEST_ASSERT_TRUE(a->engine->buildChannelAppMessage(text, 0, 4000, channel_frame));
    TEST_ASSERT_TRUE(isChannelAddress(channel_frame.dst));
    TEST_ASSERT_TRUE(channel_frame.hasFlag(kFlagEncrypted));
    b->engine->onFrame(channel_frame, Link::Lora, -55, 7, 4100);
    const EngineEvent* channel_event = nullptr;
    for (size_t i = 0; i < b->engine->events().size(); ++i) {
        if (b->engine->events().at(i).type == EventType::AppMessage)
            channel_event = &b->engine->events().at(i);
    }
    TEST_ASSERT_NOT_NULL(channel_event);
    if (channel_event == nullptr) return;
    TEST_ASSERT_EQUAL_STRING("hi", channel_event->message.text.data);
    TEST_ASSERT_EQUAL_UINT32(0, b->engine->stats().decrypt_errors);

    // A position GroupMessage is larger than the MTU (64-byte signature); the engine must
    // fall back to a direct message rather than emitting an invalid frame.
    b->engine->events().clear();
    Position position;
    position.lat_e7 = 599137000;
    position.lon_e7 = 107522000;
    position.has_alt = true;
    position.alt_m = 45;
    position.timestamp_ms = 1700000000000ull;
    Frame frame;
    TEST_ASSERT_TRUE(a->engine->buildPosition(position, 0x77777777, 0, 5000, frame));
    TEST_ASSERT_TRUE(isUnicastAddress(frame.dst));
    b->engine->onFrame(frame, Link::Lora, -55, 7, 5100);
    const EngineEvent* position_event = nullptr;
    for (size_t i = 0; i < b->engine->events().size(); ++i) {
        if (b->engine->events().at(i).type == EventType::AppMessage)
            position_event = &b->engine->events().at(i);
    }
    TEST_ASSERT_NOT_NULL(position_event);
    if (position_event == nullptr) return;
    TEST_ASSERT_TRUE(position_event->message.kind == MessageKind::Position);
    TEST_ASSERT_EQUAL_INT(599137000, position_event->message.position.lat_e7);
}

void test_engine_store_forward_handshake() {
    auto relay = makeNode(0x61, true, true, true);
    auto d = makeNode(0x62, false, false, true);
    auto sender = makeNode(0x63, false, false);
    uint32_t stored_msg_ref = 0;
    {
        Frame frame = makeStoredCandidate(sender->id, d->id.nodeId(), 0xFEEDFACE, 77);
        stored_msg_ref = readU32Be(frame.body + 1);
        relay->engine->onFrame(frame, Link::Lora, -80, 3, 2000);
    }
    TEST_ASSERT_EQUAL_UINT32(1, (unsigned)relay->engine->store().size());
    // The addressee's announce triggers a StoreOffer.
    Frame d_announce;
    TEST_ASSERT_TRUE(d->engine->buildAnnounce(3000, d_announce));
    relay->engine->onFrame(d_announce, Link::Lora, -70, 4, 3100);
    const Frame* offer = nullptr;
    for (size_t i = 0; i < relay->engine->outbox().size(); ++i) {
        if (relay->engine->outbox().at(i).frame.type == FrameType::StoreOffer) {
            offer = &relay->engine->outbox().at(i).frame;
        }
    }
    TEST_ASSERT_NOT_NULL(offer);
    if (offer == nullptr) return;
    TEST_ASSERT_TRUE(offer->dst == d->id.nodeId());
    StoreOffer parsed_offer;
    TEST_ASSERT_TRUE(decodeStoreOffer(offer->body, offer->body_len, parsed_offer));
    TEST_ASSERT_EQUAL_UINT8(1, parsed_offer.count);
    TEST_ASSERT_EQUAL_UINT32(stored_msg_ref, parsed_offer.ids[0]);
    // Addressee replies with StoreWant, relay replays the original frame.
    d->engine->onFrame(*offer, Link::Lora, -70, 4, 3200);
    const Frame* want = nullptr;
    for (size_t i = 0; i < d->engine->outbox().size(); ++i) {
        if (d->engine->outbox().at(i).frame.type == FrameType::StoreWant) {
            want = &d->engine->outbox().at(i).frame;
        }
    }
    TEST_ASSERT_NOT_NULL(want);
    if (want == nullptr) return;
    TEST_ASSERT_TRUE(want->dst == relay->id.nodeId());
    relay->engine->outbox().clear();
    relay->engine->onFrame(*want, Link::Lora, -70, 4, 3300);
    const Frame* replay = nullptr;
    for (size_t i = 0; i < relay->engine->outbox().size(); ++i) {
        const Frame& f = relay->engine->outbox().at(i).frame;
        if (f.type == FrameType::Data && f.dst == d->id.nodeId()) replay = &f;
    }
    TEST_ASSERT_NOT_NULL(replay);
    if (replay == nullptr) return;
    TEST_ASSERT_EQUAL_UINT8(relay->engine->config().default_ttl, replay->ttl);
    TEST_ASSERT_EQUAL_UINT32(stored_msg_ref, readU32Be(replay->body + 1));
    TEST_ASSERT_EQUAL_UINT32(0, (unsigned)relay->engine->store().size());
    TEST_ASSERT_EQUAL_UINT32(1, relay->engine->stats().store_delivered);
}

void test_engine_config_and_companion_views() {
    auto node = makeNode(0x71, true, true);
    TEST_ASSERT_TRUE(node->engine->applyConfig("node.name", "ridge-repeater"));
    TEST_ASSERT_EQUAL_STRING("ridge-repeater", node->engine->config().name);
    TEST_ASSERT_TRUE(node->engine->applyConfig("mesh.ttl", "7"));
    TEST_ASSERT_EQUAL_UINT8(7, node->engine->config().default_ttl);
    TEST_ASSERT_FALSE(node->engine->applyConfig("mesh.ttl", "99"));
    TEST_ASSERT_TRUE(node->engine->applyConfig("radio.profile", "LONG_FAST"));
    TEST_ASSERT_EQUAL_STRING("LONG_FAST", node->engine->config().profile->name);
    TEST_ASSERT_TRUE(node->engine->applyConfig("radio.tx_power", "30"));
    // Legal-region ceiling 27 dBm, board PA cap 22 dBm -> 22; never 14 and never 30.
    TEST_ASSERT_EQUAL_INT(22, node->engine->config().tx_power_dbm);
    // mesh.relay toggles managed-flood participation and the announce caps bit.
    TEST_ASSERT_TRUE(node->engine->config().relay_role);
    TEST_ASSERT_TRUE((node->engine->config().caps & kCapRelay) != 0);
    TEST_ASSERT_TRUE(node->engine->applyConfig("mesh.relay", "0"));
    TEST_ASSERT_FALSE(node->engine->config().relay_role);
    TEST_ASSERT_EQUAL_UINT16(0, node->engine->config().caps & kCapRelay);
    TEST_ASSERT_TRUE(node->engine->applyConfig("mesh.relay", "1"));
    TEST_ASSERT_TRUE(node->engine->config().relay_role);
    TEST_ASSERT_TRUE((node->engine->config().caps & kCapRelay) != 0);
    TEST_ASSERT_FALSE(node->engine->applyConfig("nope.nope", "1"));

    CompanionMsg ack;
    node->engine->fillHelloAck(ack);
    TEST_ASSERT_TRUE(ack.type == CompanionType::HelloAck);
    TEST_ASSERT_EQUAL_UINT8(1, ack.proto);
    TEST_ASSERT_TRUE(ack.node_id == node->id.nodeId());
    TEST_ASSERT_EQUAL_UINT16(kMtu, ack.max_frame);
    TEST_ASSERT_EQUAL_STRING("EU868", ack.region);
    TEST_ASSERT_EQUAL_STRING("LONG_FAST", ack.profile);

    CompanionMsg config;
    node->engine->fillConfig(config);
    TEST_ASSERT_TRUE(config.entry_count >= 8);
    CompanionMsg stats;
    node->engine->fillStats(stats);
    TEST_ASSERT_TRUE(stats.entry_count >= 6);

    // Companion Tx accepts a valid frame and queues it for transmission.
    Frame tx_frame;
    tx_frame.type = FrameType::Announce;
    tx_frame.ttl = kAnnounceTtl;
    tx_frame.src = node->id.nodeId();
    tx_frame.dst = NodeId::broadcast();
    tx_frame.seq = 99;
    Announce announce;
    announce.ident = node->id.publicIdentity();
    uint8_t unsigned_body[kAnnounceBodyLen];
    const size_t unsigned_len = encodeAnnounceUnsigned(announce, unsigned_body,
                                                       sizeof(unsigned_body));
    uint8_t payload[kMtu];
    const size_t payload_len = announceSignaturePayload(tx_frame.src, tx_frame.seq,
                                                         unsigned_body, unsigned_len, payload,
                                                         sizeof(payload));
    uint8_t signature[kSignatureLen];
    TEST_ASSERT_TRUE(node->id.sign(payload, payload_len, signature));
    tx_frame.body_len = (uint16_t)encodeAnnounceBody(announce, signature, tx_frame.body,
                                                     sizeof(tx_frame.body));
    uint8_t encoded[kMtu];
    const size_t encoded_len = tx_frame.encode(encoded, sizeof(encoded));
    node->engine->outbox().clear();
    TEST_ASSERT_TRUE(node->engine->originateFromApp(encoded, encoded_len, 5000));
    TEST_ASSERT_EQUAL_UINT32(1, (unsigned)node->engine->outbox().size());
}

// --- Regulatory policy (v0.5.0) --------------------------------------------------------------
namespace {

const char* configValue(const CompanionMsg& msg, const char* key) {
    for (uint8_t i = 0; i < msg.entry_count; ++i) {
        if (std::strcmp(msg.entries[i].key, key) == 0) return msg.entries[i].value;
    }
    return nullptr;
}

}  // namespace

void test_region_table_v2() {
    struct RegionExpect {
        const char* name;
        uint32_t freq_khz;
        int8_t max_power_dbm;
        int8_t duty_cycle_pct;
        uint16_t bw_khz;
    };
    // Hardcoded normative values (docs/design/regulatory-and-regions.md §1; core util.rs).
    const RegionExpect expected[] = {
        {"EU868", 869525, 27, 10, 250}, {"EU433", 433920, 10, 10, 250},
        {"US915", 915000, 30, -1, 500}, {"AU915", 915000, 30, -1, 500},
        {"IN865", 865000, 30, -1, 250}, {"KR920", 920000, 23, -1, 250},
        {"AS923", 923200, 20, 10, 250},
    };
    for (const auto& e : expected) {
        const Region* region = regionByName(e.name);
        TEST_ASSERT_NOT_NULL(region);
        TEST_ASSERT_EQUAL_STRING(e.name, region->name);
        TEST_ASSERT_EQUAL_UINT32(e.freq_khz, region->freq_khz);
        TEST_ASSERT_EQUAL_INT(e.max_power_dbm, (int)region->max_power_dbm);
        TEST_ASSERT_EQUAL_INT(e.duty_cycle_pct, (int)region->duty_cycle_pct);
        TEST_ASSERT_EQUAL_UINT16(e.bw_khz, region->bw_khz);
    }
    TEST_ASSERT_TRUE(&kRegionEu433 == regionByName("EU433"));
    TEST_ASSERT_NULL(regionByName("EU999"));
    // Region names are unique and defaultRegion() resolves to a table entry.
    for (size_t i = 0; i < sizeof(expected) / sizeof(expected[0]); ++i) {
        for (size_t k = i + 1; k < sizeof(expected) / sizeof(expected[0]); ++k) {
            TEST_ASSERT_TRUE(std::strcmp(expected[i].name, expected[k].name) != 0);
        }
    }
    const Region& def = defaultRegion();
    TEST_ASSERT_NOT_NULL(regionByName(def.name));
    // Profiles are still resolvable and their bandwidths are the presets we claim.
    TEST_ASSERT_EQUAL_UINT16(500, profileByName("SHORT_TURBO")->bw_khz);
    TEST_ASSERT_EQUAL_UINT16(250, profileByName("MEDIUM_FAST")->bw_khz);
    TEST_ASSERT_EQUAL_UINT16(125, profileByName("LONG_SLOW")->bw_khz);
}

void test_profile_bandwidth_validation() {
    // SHORT_TURBO (500 kHz) is legal only in the 500 kHz regions (US915/AU915).
    for (const char* name : {"EU868", "EU433", "IN865", "KR920", "AS923"}) {
        const Region* region = regionByName(name);
        TEST_ASSERT_FALSE(profileFitsRegion(*profileByName("SHORT_TURBO"), *region));
        TEST_ASSERT_TRUE(profileFitsRegion(*profileByName("MEDIUM_FAST"), *region));
        TEST_ASSERT_TRUE(profileFitsRegion(*profileByName("LONG_FAST"), *region));
    }
    for (const char* name : {"US915", "AU915"}) {
        const Region* region = regionByName(name);
        TEST_ASSERT_TRUE(profileFitsRegion(*profileByName("SHORT_TURBO"), *region));
        TEST_ASSERT_TRUE(profileFitsRegion(*profileByName("MEDIUM_FAST"), *region));
    }
    // Engine rejects an illegal profile instead of silently clamping it.
    auto node = makeNode(0x72, false, false);
    TEST_ASSERT_FALSE(node->engine->applyConfig("radio.profile", "SHORT_TURBO"));
    TEST_ASSERT_EQUAL_STRING("MEDIUM_FAST", node->engine->config().profile->name);
    TEST_ASSERT_TRUE(node->engine->applyConfig("radio.region", "US915"));
    TEST_ASSERT_TRUE(node->engine->applyConfig("radio.profile", "SHORT_TURBO"));
    TEST_ASSERT_EQUAL_STRING("SHORT_TURBO", node->engine->config().profile->name);
    // A region change that invalidates the profile falls back safely (LONG_FAST on EU433).
    TEST_ASSERT_TRUE(node->engine->applyConfig("radio.region", "EU433"));
    TEST_ASSERT_EQUAL_STRING("LONG_FAST", node->engine->config().profile->name);
    TEST_ASSERT_EQUAL_STRING("EU433", node->engine->config().region->name);
    TEST_ASSERT_FALSE(node->engine->applyConfig("radio.region", "NOPE"));
}

void test_board_power_caps() {
    // Single source of truth: firmware/boards/board.h kPaCaps[].
    TEST_ASSERT_EQUAL_INT(15, board::paCapForBoard("t1000e")->max_tx_power_dbm);
    TEST_ASSERT_EQUAL_INT(22, board::paCapForBoard("rak4631_solar")->max_tx_power_dbm);
    TEST_ASSERT_EQUAL_INT(22, board::paCapForBoard("seeed_solar")->max_tx_power_dbm);
    TEST_ASSERT_EQUAL_INT(22, board::paCapForBoard("heltec_tracker")->max_tx_power_dbm);
    TEST_ASSERT_NULL(board::paCapForBoard("nope"));
    TEST_ASSERT_NULL(board::paCapForBoard(nullptr));
    TEST_ASSERT_EQUAL_INT(4, (int)board::kPaCapCount);
    // Default TX power is min(legal region max, board PA cap) for every board.
    const char* board_ids[] = {"t1000e", "rak4631_solar", "seeed_solar", "heltec_tracker"};
    for (const char* id : board_ids) {
        const int8_t cap = board::paCapForBoard(id)->max_tx_power_dbm;
        struct Case {
            const Region* region;
            int8_t expected;
        };
        const Case cases[] = {
            {&kRegionEu868, (int8_t)(cap < 27 ? cap : 27)},
            {&kRegionEu433, (int8_t)(cap < 10 ? cap : 10)},
            {&kRegionUs915, (int8_t)(cap < 30 ? cap : 30)},
            {&kRegionKr920, (int8_t)(cap < 23 ? cap : 23)},
            {&kRegionAs923, (int8_t)(cap < 20 ? cap : 20)},
        };
        for (const Case& c : cases) {
            TEST_ASSERT_EQUAL_INT(c.expected,
                                  effectivePowerDbm(*c.region, cap, kNoTxPowerOverrideDbm, false));
        }
    }
}

void test_effective_power_policy() {
    // Effective power is min(region legal max, board PA cap); an active override may rise only
    // up to the PA cap, never above it.
    TEST_ASSERT_EQUAL_INT(22, effectivePowerDbm(kRegionEu868, 22, kNoTxPowerOverrideDbm, false));
    TEST_ASSERT_EQUAL_INT(15, effectivePowerDbm(kRegionEu868, 15, kNoTxPowerOverrideDbm, false));
    TEST_ASSERT_EQUAL_INT(10, effectivePowerDbm(kRegionEu433, 22, kNoTxPowerOverrideDbm, false));
    // Under an override the power request may exceed the legal region max; only the PA cap
    // is absolute (contract §2.2: power = min(override request, hardware max)).
    TEST_ASSERT_EQUAL_INT(22, effectivePowerDbm(kRegionEu433, 22, 30, true));
    TEST_ASSERT_EQUAL_INT(22, effectivePowerDbm(kRegionUs915, 22, 30, true));
    TEST_ASSERT_EQUAL_INT(22, effectivePowerDbm(kRegionEu868, 22, 30, true));
    TEST_ASSERT_EQUAL_INT(15, effectivePowerDbm(kRegionEu868, 15, 30, true));
    // Without an active override, requests above the legal max are clamped to the legal max.
    TEST_ASSERT_EQUAL_INT(22, effectivePowerDbm(kRegionEu868, 22, 30, false));
    TEST_ASSERT_EQUAL_INT(15, effectivePowerDbm(kRegionEu868, 15, 30, false));
    // Reductions are always honoured, and -9 dBm is the radio floor.
    TEST_ASSERT_EQUAL_INT(10, effectivePowerDbm(kRegionEu868, 22, 10, false));
    TEST_ASSERT_EQUAL_INT(-9, effectivePowerDbm(kRegionEu868, 22, -9, false));
    // Override legality is judged from duty/frequency requests (contract §2.1).
    TEST_ASSERT_FALSE(overrideExceedsLegal(kRegionEu868, -1, 0));
    TEST_ASSERT_FALSE(overrideExceedsLegal(kRegionEu868, 10, kRegionEu868.freq_khz));
    TEST_ASSERT_FALSE(overrideExceedsLegal(kRegionEu868, 5, 0));   // reducing duty is legal
    TEST_ASSERT_TRUE(overrideExceedsLegal(kRegionEu868, 50, 0));
    TEST_ASSERT_TRUE(overrideExceedsLegal(kRegionEu868, -1, 869000));
    TEST_ASSERT_FALSE(overrideExceedsLegal(kRegionUs915, 100, 0));  // fair-use: no legal %
}

void test_duty_override_policy() {
    TEST_ASSERT_EQUAL_INT(10, (int)effectiveDutyPct(kRegionEu868, -1));
    TEST_ASSERT_EQUAL_INT(-1, (int)effectiveDutyPct(kRegionUs915, -1));
    TEST_ASSERT_EQUAL_INT(0, (int)effectiveDutyPct(kRegionEu868, 0));
    TEST_ASSERT_EQUAL_INT(100, (int)effectiveDutyPct(kRegionEu868, 100));
    TEST_ASSERT_EQUAL_INT(100, (int)effectiveDutyPct(kRegionEu868, 150));  // clamped
    AirtimeBudget legal;
    legal.configure(kRegionEu868);
    TEST_ASSERT_TRUE(legal.enabled());
    TEST_ASSERT_FALSE(legal.blocksAllTx());
    TEST_ASSERT_EQUAL_UINT32(360000, legal.capacityMs());
    AirtimeBudget full;
    full.configure(kRegionEu868, 100);
    TEST_ASSERT_EQUAL_INT(100, (int)full.dutyPct());
    TEST_ASSERT_EQUAL_UINT32(3600000, full.capacityMs());  // 100 % of an hour
    full.reset(0);
    TEST_ASSERT_TRUE(full.tryConsume(3600000, 0));
    TEST_ASSERT_FALSE(full.tryConsume(1, 0));
    AirtimeBudget silent;
    silent.configure(kRegionEu868, 0);
    TEST_ASSERT_TRUE(silent.enabled());
    TEST_ASSERT_TRUE(silent.blocksAllTx());
    TEST_ASSERT_EQUAL_UINT32(0, silent.capacityMs());
    TEST_ASSERT_FALSE(silent.tryConsume(1, 0));
    AirtimeBudget reduced;
    reduced.configure(kRegionEu868, 5);
    TEST_ASSERT_EQUAL_UINT32(180000, reduced.capacityMs());
    // Fair-use regions stay unlimited unless an override imposes a limit.
    AirtimeBudget fair;
    fair.configure(kRegionUs915);
    TEST_ASSERT_FALSE(fair.enabled());
    TEST_ASSERT_TRUE(fair.tryConsume(1000000, 0));
    fair.configure(kRegionUs915, 0);
    TEST_ASSERT_TRUE(fair.enabled());
    TEST_ASSERT_TRUE(fair.blocksAllTx());
}

void test_frequency_override_and_config() {
    TEST_ASSERT_TRUE(validateFrequencyKhz(150000));
    TEST_ASSERT_TRUE(validateFrequencyKhz(433920));
    TEST_ASSERT_TRUE(validateFrequencyKhz(869525));
    TEST_ASSERT_TRUE(validateFrequencyKhz(960000));
    TEST_ASSERT_FALSE(validateFrequencyKhz(149999));
    TEST_ASSERT_FALSE(validateFrequencyKhz(960001));
    TEST_ASSERT_FALSE(validateFrequencyKhz(0));  // 0 means "clear override", not a frequency
    TEST_ASSERT_EQUAL_UINT32(869525, effectiveFrequencyKhz(kRegionEu868, 0));
    TEST_ASSERT_EQUAL_UINT32(433920, effectiveFrequencyKhz(kRegionEu868, 433920));

    auto node = makeNode(0x73, false, false);
    TEST_ASSERT_TRUE(node->engine->applyConfig("radio.frequency", "433920"));
    TEST_ASSERT_EQUAL_UINT32(433920, node->engine->config().frequency_override_khz);
    TEST_ASSERT_EQUAL_UINT32(433920, node->engine->effectiveFrequencyKhzValue());
    TEST_ASSERT_FALSE(node->engine->applyConfig("radio.frequency", "149999"));
    TEST_ASSERT_FALSE(node->engine->applyConfig("radio.frequency", "960001"));
    TEST_ASSERT_TRUE(node->engine->applyConfig("radio.frequency", "0"));
    TEST_ASSERT_EQUAL_UINT32(0, node->engine->config().frequency_override_khz);
    // Duty override keys and their reflection in companion Config/Stats.
    TEST_ASSERT_FALSE(node->engine->applyConfig("radio.duty_override", "101"));
    TEST_ASSERT_FALSE(node->engine->applyConfig("radio.duty_override", "-2"));
    TEST_ASSERT_TRUE(node->engine->applyConfig("radio.duty_override", "100"));
    TEST_ASSERT_EQUAL_INT(100, (int)node->engine->config().duty_override_pct);
    TEST_ASSERT_TRUE(node->engine->overrideActive());
    TEST_ASSERT_EQUAL_STRING(kRegulatoryOverrideWarning, node->engine->overrideWarning());
    CompanionMsg config;
    node->engine->fillConfig(config);
    TEST_ASSERT_EQUAL_STRING("100", configValue(config, "radio.duty_override"));
    TEST_ASSERT_EQUAL_STRING("0", configValue(config, "radio.frequency"));
    CompanionMsg stats;
    node->engine->fillStats(stats);
    TEST_ASSERT_EQUAL_STRING("1", configValue(stats, "override_active"));
    TEST_ASSERT_EQUAL_STRING("100", configValue(stats, "duty_pct"));
    TEST_ASSERT_EQUAL_STRING("22", configValue(stats, "tx_power_dbm"));
    // Clearing the override returns to the legal duty and legal-max power.
    TEST_ASSERT_TRUE(node->engine->applyConfig("radio.duty_override", "-1"));
    TEST_ASSERT_FALSE(node->engine->overrideActive());
    TEST_ASSERT_NULL(node->engine->overrideWarning());
    CompanionMsg stats_after;
    node->engine->fillStats(stats_after);
    TEST_ASSERT_EQUAL_STRING("10", configValue(stats_after, "duty_pct"));
    TEST_ASSERT_EQUAL_STRING("0", configValue(stats_after, "override_active"));
}

void test_regulatory_override_persistence() {
    InMemoryStorage storage;
    {
        auto node = makeNodeWithConfig(0x81, defaultEngineConfig(0x81, false, false, false),
                                       &storage);
        TEST_ASSERT_TRUE(node->engine->applyConfig("radio.duty_override", "100"));
        TEST_ASSERT_TRUE(node->engine->applyConfig("radio.tx_power", "30"));
        TEST_ASSERT_TRUE(node->engine->applyConfig("radio.frequency", "869000"));
        TEST_ASSERT_TRUE(node->engine->overrideActive());
        TEST_ASSERT_EQUAL_INT(22, node->engine->config().tx_power_dbm);  // board cap
    }
    // A fresh boot with the same storage restores the override and policy.
    auto restored = makeNodeWithConfig(0x82, defaultEngineConfig(0x82, false, false, false),
                                       &storage);
    TEST_ASSERT_EQUAL_INT(100, (int)restored->engine->config().duty_override_pct);
    TEST_ASSERT_EQUAL_INT(30, (int)restored->engine->config().power_override_dbm);
    TEST_ASSERT_EQUAL_UINT32(869000, restored->engine->config().frequency_override_khz);
    TEST_ASSERT_TRUE(restored->engine->overrideActive());
    TEST_ASSERT_EQUAL_INT(100, (int)restored->engine->effectiveDutyPctValue());
    TEST_ASSERT_EQUAL_INT(22, restored->engine->config().tx_power_dbm);
    TEST_ASSERT_NOT_NULL(restored->engine->overrideWarning());
    // Disable in two steps (frequency, then duty); policy returns to the legal maxima.
    TEST_ASSERT_TRUE(restored->engine->applyConfig("radio.frequency", "0"));
    TEST_ASSERT_TRUE(restored->engine->applyConfig("radio.duty_override", "-1"));
    TEST_ASSERT_FALSE(restored->engine->overrideActive());
    TEST_ASSERT_NULL(restored->engine->overrideWarning());
    TEST_ASSERT_EQUAL_INT(10, (int)restored->engine->effectiveDutyPctValue());
    TEST_ASSERT_EQUAL_INT(22, restored->engine->config().tx_power_dbm);
}

void test_relay_config_persistence() {
    InMemoryStorage storage;
    {
        auto node = makeNodeWithConfig(0x83, defaultEngineConfig(0x83, true, false, false),
                                       &storage);
        TEST_ASSERT_TRUE(node->engine->config().relay_role);
        TEST_ASSERT_TRUE((node->engine->config().caps & kCapRelay) != 0);
        TEST_ASSERT_TRUE(node->engine->applyConfig("mesh.relay", "0"));
        TEST_ASSERT_FALSE(node->engine->config().relay_role);
    }
    // A fresh boot with the same storage restores mesh.relay=0 and clears the caps bit.
    auto restored = makeNodeWithConfig(0x83, defaultEngineConfig(0x83, true, false, false),
                                       &storage);
    TEST_ASSERT_FALSE(restored->engine->config().relay_role);
    TEST_ASSERT_EQUAL_UINT16(0, restored->engine->config().caps & kCapRelay);
    CompanionMsg config;
    restored->engine->fillConfig(config);
    TEST_ASSERT_EQUAL_STRING("0", configValue(config, "mesh.relay"));
    // Re-enabling at runtime restores the caps bit and is persisted again.
    TEST_ASSERT_TRUE(restored->engine->applyConfig("mesh.relay", "1"));
    TEST_ASSERT_TRUE(restored->engine->config().relay_role);
    TEST_ASSERT_TRUE((restored->engine->config().caps & kCapRelay) != 0);
}
