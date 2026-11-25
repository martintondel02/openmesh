// SPDX-License-Identifier: AGPL-3.0-or-later
// Golden-vector replay against the normative JSON in protocol/vectors/ (OMP/1 v1).
//
// The Rust core workstream generates these files; protocol/vectors/README.md is authoritative
// for the schema. If the files are absent the tests print a visible SKIP and pass, so the host
// suite still runs in a partial checkout. Once present, every vector is asserted: frame
// encode/decode/AAD, identity derivation, announce body/signature/hops, AppMessage/Ack CBOR
// round-trips, resource blob/manifest/chunk layout and companion framing (CRC + SLIP).
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>

#ifdef OM_USE_UNITY
#include <unity.h>
#else
#include "test_shim.h"
#endif

#include "json_lite.h"
#include "openmesh/om_companion.h"
#include "openmesh/om_identity.h"
#include "openmesh/om_messages.h"
#include "openmesh/om_sha256.h"
#include "openmesh/om_store.h"
#include "openmesh/om_types.h"
#include "tests.h"

namespace {

using namespace om;

int hexNib(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

bool hexToBytes(const std::string& hex, std::vector<uint8_t>& out) {
    out.clear();
    if (hex.size() % 2 != 0) return false;
    for (size_t i = 0; i < hex.size(); i += 2) {
        const int hi = hexNib(hex[i]);
        const int lo = hexNib(hex[i + 1]);
        if (hi < 0 || lo < 0) return false;
        out.push_back((uint8_t)((hi << 4) | lo));
    }
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

std::string fieldString(const json::Value& obj, const char* key) {
    const json::Value* v = obj.get(key);
    return v != nullptr && v->isString() ? v->asString() : std::string();
}

uint64_t fieldU64(const json::Value& obj, const char* key, uint64_t fallback = 0) {
    const json::Value* v = obj.get(key);
    return v != nullptr ? v->asU64() : fallback;
}

bool fieldBytes(const json::Value& obj, const char* key, std::vector<uint8_t>& out) {
    const json::Value* v = obj.get(key);
    if (v == nullptr || !v->isString()) return false;
    return hexToBytes(v->asString(), out);
}

std::vector<std::string> vectorsDirectories() {
    std::vector<std::string> dirs;
    if (const char* env = std::getenv("OM_VECTORS_DIR")) dirs.emplace_back(env);
    dirs.emplace_back("protocol/vectors");
    dirs.emplace_back("../protocol/vectors");
    dirs.emplace_back("../../protocol/vectors");
    dirs.emplace_back("../../../protocol/vectors");
    return dirs;
}

bool loadVectorFile(const char* name, json::Value& out, std::string& path) {
    for (const std::string& dir : vectorsDirectories()) {
        const std::string candidate = dir + "/" + name;
        std::ifstream in(candidate.c_str());
        if (!in.good()) continue;
        std::ostringstream buffer;
        buffer << in.rdbuf();
        std::string error;
        if (json::parse(buffer.str(), out, error)) {
            path = candidate;
            return true;
        }
        std::printf("  (vector parse error in %s: %s)\n", candidate.c_str(), error.c_str());
    }
    return false;
}

bool loadOrSkip(const char* name, json::Value& root, std::string& path) {
    if (loadVectorFile(name, root, path)) return true;
    std::printf(
        "  SKIP %-16s : protocol/vectors/%s not present; host unit tests still cover the codec.\n",
        name, name);
    return false;
}

// Builds a Frame header from an encode/decode vector object.
Frame frameFromJson(const json::Value& obj) {
    Frame frame;
    frame.type = (FrameType)(uint8_t)fieldU64(obj, "type");
    frame.flags = (uint8_t)fieldU64(obj, "flags");
    frame.ttl = (uint8_t)fieldU64(obj, "ttl");
    std::string src = fieldString(obj, "src_hex");
    std::string dst = fieldString(obj, "dst_hex");
    NodeId::fromHex(src.c_str(), frame.src);
    NodeId::fromHex(dst.c_str(), frame.dst);
    frame.seq = (uint32_t)fieldU64(obj, "seq");
    std::vector<uint8_t> body;
    if (fieldBytes(obj, "body_hex", body)) {
        if (body.size() > kMaxBody) body.resize(kMaxBody);
        if (!body.empty()) std::memcpy(frame.body, body.data(), body.size());
        frame.body_len = (uint16_t)body.size();
    }
    return frame;
}

void checkFrameAgainstJson(const Frame& frame, const json::Value& expect) {
    TEST_ASSERT_EQUAL_UINT8((uint8_t)fieldU64(expect, "type"), (uint8_t)frame.type);
    TEST_ASSERT_EQUAL_UINT8((uint8_t)fieldU64(expect, "flags"), frame.flags);
    TEST_ASSERT_EQUAL_UINT8((uint8_t)fieldU64(expect, "ttl"), frame.ttl);
    TEST_ASSERT_EQUAL_UINT32((uint32_t)fieldU64(expect, "seq"), frame.seq);
    std::vector<uint8_t> src;
    std::vector<uint8_t> dst;
    TEST_ASSERT_TRUE(fieldBytes(expect, "src_hex", src));
    TEST_ASSERT_TRUE(fieldBytes(expect, "dst_hex", dst));
    TEST_ASSERT_EQUAL_MEMORY(src.data(), frame.src.b, 8);
    TEST_ASSERT_EQUAL_MEMORY(dst.data(), frame.dst.b, 8);
    std::vector<uint8_t> expected_body;
    TEST_ASSERT_TRUE(fieldBytes(expect, "body_hex", expected_body));
    TEST_ASSERT_EQUAL_UINT32((unsigned)expected_body.size(), (unsigned)frame.body_len);
    if (!expected_body.empty()) {
        TEST_ASSERT_EQUAL_MEMORY(expected_body.data(), frame.body, expected_body.size());
    }
    std::vector<uint8_t> expected_aad;
    if (fieldBytes(expect, "aad_hex", expected_aad)) {
        uint8_t aad[kHeaderLen];
        frame.aad(aad);
        TEST_ASSERT_EQUAL_MEMORY(expected_aad.data(), aad, kHeaderLen);
    }
}

std::string kindToString(MessageKind kind) {
    return messageKindName(kind);
}

CompanionMsg companionFromJson(CompanionType type, const json::Value& payload) {
    CompanionMsg msg;
    msg.type = type;
    msg.entry_count = 0;
    switch (type) {
        case CompanionType::Hello:
            msg.proto = (uint8_t)fieldU64(payload, "proto");
            std::snprintf(msg.app_version, sizeof(msg.app_version), "%s",
                          fieldString(payload, "app_version").c_str());
            msg.wants_events = payload.get("wants_events") == nullptr
                                   ? true
                                   : payload.get("wants_events")->boolean;
            break;
        case CompanionType::HelloAck: {
            msg.proto = (uint8_t)fieldU64(payload, "proto");
            std::snprintf(msg.fw_version, sizeof(msg.fw_version), "%s",
                          fieldString(payload, "fw_version").c_str());
            std::vector<uint8_t> node_id;
            if (fieldBytes(payload, "node_id_hex", node_id) && node_id.size() == 8) {
                std::memcpy(msg.node_id.b, node_id.data(), 8);
            }
            msg.max_frame = (uint16_t)fieldU64(payload, "max_frame");
            msg.caps = (uint16_t)fieldU64(payload, "caps");
            std::snprintf(msg.region, sizeof(msg.region), "%s",
                          fieldString(payload, "region").c_str());
            std::snprintf(msg.profile, sizeof(msg.profile), "%s",
                          fieldString(payload, "profile").c_str());
            break;
        }
        case CompanionType::ConfigGet:
        case CompanionType::StatsGet:
        case CompanionType::Reset:
            break;
        case CompanionType::ConfigSet:
            std::snprintf(msg.key, sizeof(msg.key), "%s", fieldString(payload, "key").c_str());
            std::snprintf(msg.value, sizeof(msg.value), "%s",
                          fieldString(payload, "value").c_str());
            break;
        case CompanionType::Config: {
            const json::Value* entries = payload.get("entries");
            if (entries != nullptr && entries->isArray()) {
                for (const json::Value& entry : entries->array) {
                    if (!entry.isArray() || entry.size() != 2 || msg.entry_count >= kCompanionMaxEntries)
                        continue;
                    std::snprintf(msg.entries[msg.entry_count].key,
                                  sizeof(msg.entries[0].key), "%s",
                                  entry.array[0].asString().c_str());
                    std::snprintf(msg.entries[msg.entry_count].value,
                                  sizeof(msg.entries[0].value), "%s",
                                  entry.array[1].asString().c_str());
                    ++msg.entry_count;
                }
            }
            break;
        }
        case CompanionType::Tx: {
            std::vector<uint8_t> frame;
            if (fieldBytes(payload, "frame_hex", frame)) {
                std::memcpy(msg.frame, frame.data(), frame.size());
                msg.frame_len = (uint16_t)frame.size();
            }
            break;
        }
        case CompanionType::Rx: {
            std::vector<uint8_t> frame;
            if (fieldBytes(payload, "frame_hex", frame)) {
                std::memcpy(msg.frame, frame.data(), frame.size());
                msg.frame_len = (uint16_t)frame.size();
            }
            msg.rssi = (int16_t)fieldU64(payload, "rssi");
            msg.snr = (int16_t)fieldU64(payload, "snr");
            std::snprintf(msg.link, sizeof(msg.link), "%s", fieldString(payload, "link").c_str());
            msg.at_ms = fieldU64(payload, "at_ms");
            break;
        }
        case CompanionType::Stats: {
            const json::Value* entries = payload.get("entries");
            if (entries != nullptr && entries->isArray()) {
                for (const json::Value& entry : entries->array) {
                    if (!entry.isArray() || entry.size() != 2 || msg.entry_count >= kCompanionMaxEntries)
                        continue;
                    std::snprintf(msg.entries[msg.entry_count].key,
                                  sizeof(msg.entries[0].key), "%s",
                                  entry.array[0].asString().c_str());
                    std::snprintf(msg.entries[msg.entry_count].value,
                                  sizeof(msg.entries[0].value), "%llu",
                                  (unsigned long long)entry.array[1].asU64());
                    ++msg.entry_count;
                }
            }
            break;
        }
        case CompanionType::Ping:
        case CompanionType::Pong:
            msg.nonce = (uint32_t)fieldU64(payload, "nonce");
            break;
        case CompanionType::Event:
            std::snprintf(msg.kind, sizeof(msg.kind), "%s", fieldString(payload, "kind").c_str());
            std::snprintf(msg.detail, sizeof(msg.detail), "%s",
                          fieldString(payload, "detail").c_str());
            break;
    }
    return msg;
}

}  // namespace

// --- frame.json ------------------------------------------------------------------------------
void test_vectors_frame() {
    json::Value root;
    std::string path;
    if (!loadOrSkip("frame.json", root, path)) return;
    std::printf("  replaying %s\n", path.c_str());
    const json::Value* encode = root.get("encode");
    TEST_ASSERT_NOT_NULL(encode);
    for (const json::Value& c : encode->array) {
        Frame frame = frameFromJson(c);
        uint8_t out[kMtu];
        const size_t len = frame.encode(out, sizeof(out));
        std::vector<uint8_t> expected;
        TEST_ASSERT_TRUE(fieldBytes(c, "frame_hex", expected));
        TEST_ASSERT_EQUAL_UINT32((unsigned)expected.size(), (unsigned)len);
        if (len > 0) TEST_ASSERT_EQUAL_MEMORY(expected.data(), out, len);
        checkFrameAgainstJson(frame, c);
    }
    const json::Value* decode = root.get("decode");
    TEST_ASSERT_NOT_NULL(decode);
    for (const json::Value& c : decode->array) {
        std::vector<uint8_t> bytes;
        TEST_ASSERT_TRUE(fieldBytes(c, "frame_hex", bytes));
        Frame frame;
        TEST_ASSERT_TRUE(decodeFrame(bytes.data(), bytes.size(), frame).ok());
        const json::Value* expect = c.get("expect");
        TEST_ASSERT_NOT_NULL(expect);
        checkFrameAgainstJson(frame, *expect);
    }
    const json::Value* errors = root.get("decode_errors");
    TEST_ASSERT_NOT_NULL(errors);
    for (const json::Value& c : errors->array) {
        std::vector<uint8_t> bytes;
        TEST_ASSERT_TRUE(fieldBytes(c, "frame_hex", bytes));
        Frame frame;
        const DecodeResult result = decodeFrame(bytes.data(), bytes.size(), frame);
        const std::string expected_error = fieldString(c, "error");
        if (expected_error == "UnsupportedVersion") {
            TEST_ASSERT_TRUE(result.error == DecodeError::BadVersion);
        } else if (expected_error == "BufferTooSmall") {
            TEST_ASSERT_TRUE(result.error == DecodeError::TooShort);
        } else {
            TEST_ASSERT_FALSE(result.ok());
        }
    }
}

// --- identity.json ---------------------------------------------------------------------------
void test_vectors_identity() {
    json::Value root;
    std::string path;
    if (!loadOrSkip("identity.json", root, path)) return;
    std::printf("  replaying %s\n", path.c_str());
    const json::Value* nodes = root.get("nodes");
    TEST_ASSERT_NOT_NULL(nodes);
    for (const json::Value& c : nodes->array) {
        std::vector<uint8_t> seed;
        TEST_ASSERT_TRUE(fieldBytes(c, "seed_hex", seed));
        TEST_ASSERT_EQUAL_UINT32(32, (unsigned)seed.size());
        Identity id;
        TEST_ASSERT_TRUE(Identity::fromSeed(seed.data(), id));
        std::vector<uint8_t> expected;
        TEST_ASSERT_TRUE(fieldBytes(c, "node_id_hex", expected));
        TEST_ASSERT_EQUAL_MEMORY(expected.data(), id.nodeId().b, 8);
        TEST_ASSERT_TRUE(fieldBytes(c, "public_identity_hex", expected));
        TEST_ASSERT_EQUAL_UINT32(kPublicIdentityLen, (unsigned)expected.size());
        uint8_t actual[kPublicIdentityLen];
        id.publicIdentity().toBytes(actual);
        TEST_ASSERT_EQUAL_MEMORY(expected.data(), actual, kPublicIdentityLen);
        // Later nodes may omit the individual keys (they are contained in public_identity).
        if (c.get("sign_pk_hex") != nullptr) {
            TEST_ASSERT_TRUE(fieldBytes(c, "sign_pk_hex", expected));
            TEST_ASSERT_EQUAL_MEMORY(expected.data(), id.publicIdentity().sign_pk, 32);
        }
        if (c.get("dh_pk_hex") != nullptr) {
            TEST_ASSERT_TRUE(fieldBytes(c, "dh_pk_hex", expected));
            TEST_ASSERT_EQUAL_MEMORY(expected.data(), id.publicIdentity().dh_pk, 32);
        }
        // Node ids are unicast: address-class bit clear.
        TEST_ASSERT_EQUAL_UINT8(0, id.nodeId().b[0] & 0x80);
    }
    const json::Value* channels = root.get("channels");
    TEST_ASSERT_NOT_NULL(channels);
    for (const json::Value& c : channels->array) {
        std::vector<uint8_t> psk;
        TEST_ASSERT_TRUE(fieldBytes(c, "psk_hex", psk));
        TEST_ASSERT_EQUAL_UINT32(32, (unsigned)psk.size());
        ChannelKey key;
        TEST_ASSERT_TRUE(ChannelKey::derive(psk.data(), fieldString(c, "name").c_str(),
                                            fieldString(c, "name").size(), key));
        std::vector<uint8_t> expected;
        TEST_ASSERT_TRUE(fieldBytes(c, "channel_id_hex", expected));
        TEST_ASSERT_EQUAL_MEMORY(expected.data(), key.id.b, 8);
        TEST_ASSERT_TRUE((key.id.b[0] & 0x80) != 0);
    }
    const json::Value* signatures = root.get("signatures");
    TEST_ASSERT_NOT_NULL(signatures);
    for (const json::Value& c : signatures->array) {
        std::vector<uint8_t> seed;
        std::vector<uint8_t> message;
        std::vector<uint8_t> expected_signature;
        TEST_ASSERT_TRUE(fieldBytes(c, "seed_hex", seed));
        TEST_ASSERT_TRUE(fieldBytes(c, "message_hex", message));
        TEST_ASSERT_TRUE(fieldBytes(c, "signature_hex", expected_signature));
        Identity id;
        TEST_ASSERT_TRUE(Identity::fromSeed(seed.data(), id));
        uint8_t signature[kSignatureLen];
        TEST_ASSERT_TRUE(id.sign(message.data(), message.size(), signature));
        TEST_ASSERT_EQUAL_MEMORY(expected_signature.data(), signature, kSignatureLen);
        TEST_ASSERT_TRUE(id.verifyPeer(id.publicIdentity(), message.data(), message.size(),
                                       signature));
        std::vector<uint8_t> tampered;
        if (fieldBytes(c, "tampered_message_hex", tampered)) {
            TEST_ASSERT_FALSE(id.verifyPeer(id.publicIdentity(), tampered.data(),
                                            tampered.size(), signature));
        }
    }
    const json::Value* dm_keys = root.get("dm_keys");
    TEST_ASSERT_NOT_NULL(dm_keys);
    for (const json::Value& c : dm_keys->array) {
        std::vector<uint8_t> seed_a;
        std::vector<uint8_t> seed_b;
        TEST_ASSERT_TRUE(fieldBytes(c, "seed_a_hex", seed_a));
        TEST_ASSERT_TRUE(fieldBytes(c, "seed_b_hex", seed_b));
        Identity a;
        Identity b;
        TEST_ASSERT_TRUE(Identity::fromSeed(seed_a.data(), a));
        TEST_ASSERT_TRUE(Identity::fromSeed(seed_b.data(), b));
        uint8_t key_ab[32];
        uint8_t key_ba[32];
        TEST_ASSERT_TRUE(a.dmKey(b.publicIdentity(), key_ab));
        TEST_ASSERT_TRUE(b.dmKey(a.publicIdentity(), key_ba));
        TEST_ASSERT_EQUAL_MEMORY(key_ab, key_ba, 32);
        std::vector<uint8_t> expected;
        TEST_ASSERT_TRUE(fieldBytes(c, "dm_key_hex", expected));
        TEST_ASSERT_EQUAL_MEMORY(expected.data(), key_ab, 32);
    }
    const json::Value* identity_errors = root.get("identity_errors");
    TEST_ASSERT_NOT_NULL(identity_errors);
    for (const json::Value& c : identity_errors->array) {
        std::vector<uint8_t> bytes;
        TEST_ASSERT_TRUE(fieldBytes(c, "public_identity_hex", bytes));
        PublicIdentity ident;
        const std::string expected_error = fieldString(c, "error");
        if (expected_error == "Malformed") {
            TEST_ASSERT_FALSE(PublicIdentity::fromBytes(bytes.data(), bytes.size(), ident));
        } else {
            TEST_ASSERT_TRUE(PublicIdentity::fromBytes(bytes.data(), bytes.size(), ident));
            TEST_ASSERT_FALSE(ident.isSelfConsistent());
        }
    }
}

// --- announce.json ---------------------------------------------------------------------------
void test_vectors_announce() {
    json::Value root;
    std::string path;
    if (!loadOrSkip("announce.json", root, path)) return;
    std::printf("  replaying %s\n", path.c_str());
    const json::Value* cases = root.get("cases");
    TEST_ASSERT_NOT_NULL(cases);
    for (const json::Value& c : cases->array) {
        const json::Value* announce_json = c.get("announce");
        TEST_ASSERT_NOT_NULL(announce_json);
        Announce announce;
        announce.version = (uint8_t)fieldU64(*announce_json, "version", 1);
        NodeClass cls = NodeClass::Client;
        TEST_ASSERT_TRUE(parseNodeClass((const uint8_t*)fieldString(*announce_json, "class").data(),
                                        fieldString(*announce_json, "class").size(), cls));
        announce.cls = cls;
        announce.caps = (uint16_t)fieldU64(*announce_json, "caps");
        announce.interval_s = (uint16_t)fieldU64(*announce_json, "interval_s");
        announce.timestamp_ms = fieldU64(*announce_json, "timestamp_ms");
        std::string name = fieldString(*announce_json, "name");
        announce.setName(name.c_str(), name.size());
        const json::Value* ident_json = announce_json->get("ident");
        TEST_ASSERT_NOT_NULL(ident_json);
        std::vector<uint8_t> identity_bytes;
        TEST_ASSERT_TRUE(fieldBytes(*ident_json, "public_identity_hex", identity_bytes));
        TEST_ASSERT_TRUE(PublicIdentity::fromBytes(identity_bytes.data(), identity_bytes.size(),
                                                   announce.ident));

        uint8_t unsigned_body[kAnnounceBodyLen];
        const size_t unsigned_len = encodeAnnounceUnsigned(announce, unsigned_body,
                                                           sizeof(unsigned_body));
        std::vector<uint8_t> expected_unsigned;
        TEST_ASSERT_TRUE(fieldBytes(c, "body_without_signature_hex", expected_unsigned));
        TEST_ASSERT_EQUAL_UINT32((unsigned)expected_unsigned.size(), (unsigned)unsigned_len);
        if (unsigned_len > 0) {
            TEST_ASSERT_EQUAL_MEMORY(expected_unsigned.data(), unsigned_body, unsigned_len);
        }

        const uint32_t seq = (uint32_t)fieldU64(c, "seq");
        std::string src_hex = fieldString(c, "src_hex");
        NodeId src;
        TEST_ASSERT_TRUE(NodeId::fromHex(src_hex.c_str(), src));
        std::vector<uint8_t> signature;
        TEST_ASSERT_TRUE(fieldBytes(c, "signature_hex", signature));
        TEST_ASSERT_EQUAL_UINT32(kSignatureLen, (unsigned)signature.size());
        Frame frame;
        frame.type = FrameType::Announce;
        frame.ttl = kAnnounceTtl;
        frame.src = src;
        frame.dst = NodeId::broadcast();
        frame.seq = seq;
        std::vector<uint8_t> expected_body;
        TEST_ASSERT_TRUE(fieldBytes(c, "body_hex", expected_body));
        TEST_ASSERT_EQUAL_UINT32((unsigned)expected_body.size(), (unsigned)(unsigned_len + kSignatureLen));
        std::memcpy(frame.body, unsigned_body, unsigned_len);
        std::memcpy(frame.body + unsigned_len, signature.data(), kSignatureLen);
        frame.body_len = (uint16_t)expected_body.size();
        TEST_ASSERT_EQUAL_MEMORY(expected_body.data(), frame.body, frame.body_len);

        // Full TTL=8 frame must verify and match the vector frame bytes.
        std::vector<uint8_t> expected_frame;
        TEST_ASSERT_TRUE(fieldBytes(c, "frame_ttl8_hex", expected_frame));
        uint8_t encoded[kMtu];
        const size_t encoded_len = frame.encode(encoded, sizeof(encoded));
        TEST_ASSERT_EQUAL_UINT32((unsigned)expected_frame.size(), (unsigned)encoded_len);
        TEST_ASSERT_EQUAL_MEMORY(expected_frame.data(), encoded, encoded_len);
        Announce parsed;
        TEST_ASSERT_TRUE(verifyAnnounce(frame, parsed));
        TEST_ASSERT_EQUAL_STRING(name.c_str(), parsed.name);

        // A relay only decrements TTL; hops are derived, body untouched.
        const json::Value* relayed = c.get("relayed_ttl7");
        if (relayed != nullptr) {
            std::vector<uint8_t> relayed_frame;
            TEST_ASSERT_TRUE(fieldBytes(*relayed, "frame_hex", relayed_frame));
            Frame relayed_parsed;
            TEST_ASSERT_TRUE(decodeFrame(relayed_frame.data(), relayed_frame.size(),
                                         relayed_parsed).ok());
            TEST_ASSERT_TRUE(verifyAnnounce(relayed_parsed, parsed));
            TEST_ASSERT_EQUAL_UINT8((uint8_t)fieldU64(*relayed, "hops_derived"),
                                    announceHops(relayed_parsed.ttl));
            TEST_ASSERT_EQUAL_UINT32(frame.body_len, relayed_parsed.body_len);
            TEST_ASSERT_EQUAL_MEMORY(frame.body, relayed_parsed.body, frame.body_len);
        }
    }
    const json::Value* rejects = root.get("reject_cases");
    TEST_ASSERT_NOT_NULL(rejects);
    for (const json::Value& c : rejects->array) {
        std::vector<uint8_t> body;
        TEST_ASSERT_TRUE(fieldBytes(c, "body_hex", body));
        Frame frame;
        frame.type = FrameType::Announce;
        frame.ttl = kAnnounceTtl;
        TEST_ASSERT_TRUE(NodeId::fromHex(fieldString(c, "src_hex").c_str(), frame.src));
        frame.dst = NodeId::broadcast();
        frame.seq = (uint32_t)fieldU64(c, "seq");
        if (!body.empty()) std::memcpy(frame.body, body.data(), body.size());
        frame.body_len = (uint16_t)body.size();
        Announce parsed;
        TEST_ASSERT_FALSE(verifyAnnounce(frame, parsed));
    }
}


namespace {

// Wraps a standalone sealed payload (nonce_prefix || ct || tag) into a minimal OMP body so the
// production open path can be exercised without a test-only nonce-injection hook.
bool openSealedPayload(const Identity& me, const PublicIdentity& peer, bool is_channel,
                       const ChannelKey& channel, const uint8_t* aad, const uint8_t* sealed,
                       size_t sealed_len, uint8_t* out, size_t capacity, size_t& out_len) {
    // Rebuild the header from the vector AAD itself (version/type/flags/src/dst/seq); the TTL
    // nibble is cleared in the AAD so any TTL value reproduces the same bytes.
    Frame frame;
    frame.type = (FrameType)(aad[0] & 0x1F);
    frame.flags = (uint8_t)(aad[1] & 0x0F);
    frame.ttl = kDefaultTtl;
    std::memcpy(frame.src.b, aad + 2, 8);
    std::memcpy(frame.dst.b, aad + 10, 8);
    frame.seq = readU32Be(aad + 18);
    frame.body[0] = 0;
    writeU32Be(frame.body + 1, 0);
    if (sealed_len + 5 > kMaxBody) return false;
    std::memcpy(frame.body + 5, sealed, sealed_len);
    frame.body_len = (uint16_t)(5 + sealed_len);
    uint8_t frame_aad[kHeaderLen];
    frame.aad(frame_aad);
    if (std::memcmp(frame_aad, aad, kHeaderLen) != 0) return false;
    SealedBodyView view;
    if (!SealedBodyView::parse(frame.body, frame.body_len, view)) return false;
    if (is_channel) return channel.openBody(view, aad, out, capacity, out_len);
    return me.openDirectBody(peer, view, aad, out, capacity, out_len);
}

}  // namespace

// --- seal.json -------------------------------------------------------------------------------
void test_vectors_seal() {
    json::Value root;
    std::string path;
    if (!loadOrSkip("seal.json", root, path)) return;
    std::printf("  replaying %s\n", path.c_str());
    const json::Value* dm = root.get("dm");
    TEST_ASSERT_NOT_NULL(dm);
    // Keep the first DM case's key material for the empty-plaintext and tamper cases (same AAD).
    Identity dm_a;
    Identity dm_b;
    bool have_dm_keys = false;
    for (const json::Value& c : dm->array) {
        std::vector<uint8_t> seed_a;
        std::vector<uint8_t> seed_b;
        TEST_ASSERT_TRUE(fieldBytes(c, "seed_a_hex", seed_a));
        TEST_ASSERT_TRUE(fieldBytes(c, "seed_b_hex", seed_b));
        TEST_ASSERT_TRUE(Identity::fromSeed(seed_a.data(), dm_a));
        TEST_ASSERT_TRUE(Identity::fromSeed(seed_b.data(), dm_b));
        have_dm_keys = true;
        std::vector<uint8_t> sealed;
        std::vector<uint8_t> plaintext;
        std::vector<uint8_t> aad;
        TEST_ASSERT_TRUE(fieldBytes(c, "sealed_hex", sealed));
        TEST_ASSERT_TRUE(fieldBytes(c, "plaintext_hex", plaintext));
        TEST_ASSERT_TRUE(fieldBytes(c, "aad_hex", aad));
        TEST_ASSERT_EQUAL_UINT32(kHeaderLen, (uint32_t)aad.size());
        // The published key pairs must agree with the seeds and with each other's DM key.
        std::vector<uint8_t> identity_a;
        std::vector<uint8_t> identity_b;
        TEST_ASSERT_TRUE(fieldBytes(c, "public_identity_a_hex", identity_a));
        TEST_ASSERT_TRUE(fieldBytes(c, "public_identity_b_hex", identity_b));
        PublicIdentity pub_a;
        PublicIdentity pub_b;
        TEST_ASSERT_TRUE(PublicIdentity::fromBytes(identity_a.data(), identity_a.size(), pub_a));
        TEST_ASSERT_TRUE(PublicIdentity::fromBytes(identity_b.data(), identity_b.size(), pub_b));
        TEST_ASSERT_TRUE(pub_a == dm_a.publicIdentity());
        TEST_ASSERT_TRUE(pub_b == dm_b.publicIdentity());
        uint8_t key_ab[32];
        uint8_t key_ba[32];
        TEST_ASSERT_TRUE(dm_a.dmKey(pub_b, key_ab));
        TEST_ASSERT_TRUE(dm_b.dmKey(pub_a, key_ba));
        TEST_ASSERT_EQUAL_MEMORY(key_ab, key_ba, 32);
        // Bob opens the sealed payload sent by Alice (and vice versa).
        uint8_t opened[kMaxBody];
        size_t opened_len = 0;
        TEST_ASSERT_TRUE(openSealedPayload(dm_b, pub_a, false, ChannelKey{}, aad.data(),
                                            sealed.data(), sealed.size(), opened,
                                            sizeof(opened), opened_len));
        TEST_ASSERT_EQUAL_UINT32((unsigned)plaintext.size(), (unsigned)opened_len);
        if (!plaintext.empty()) {
            TEST_ASSERT_EQUAL_MEMORY(plaintext.data(), opened, opened_len);
        }
        TEST_ASSERT_TRUE(openSealedPayload(dm_a, pub_b, false, ChannelKey{}, aad.data(),
                                            sealed.data(), sealed.size(), opened,
                                            sizeof(opened), opened_len));
        (void)opened_len;
    }
    const json::Value* channels = root.get("channel");
    TEST_ASSERT_NOT_NULL(channels);
    for (const json::Value& c : channels->array) {
        std::vector<uint8_t> psk;
        std::vector<uint8_t> sealed;
        std::vector<uint8_t> plaintext;
        std::vector<uint8_t> aad;
        TEST_ASSERT_TRUE(fieldBytes(c, "psk_hex", psk));
        TEST_ASSERT_TRUE(fieldBytes(c, "sealed_hex", sealed));
        TEST_ASSERT_TRUE(fieldBytes(c, "plaintext_hex", plaintext));
        TEST_ASSERT_TRUE(fieldBytes(c, "aad_hex", aad));
        ChannelKey channel;
        TEST_ASSERT_TRUE(ChannelKey::derive(psk.data(),
                                            fieldString(c, "channel_name").c_str(),
                                            fieldString(c, "channel_name").size(), channel));
        std::vector<uint8_t> expected_id;
        TEST_ASSERT_TRUE(fieldBytes(c, "channel_id_hex", expected_id));
        TEST_ASSERT_EQUAL_MEMORY(expected_id.data(), channel.id.b, 8);
        uint8_t opened[kMaxBody];
        size_t opened_len = 0;
        TEST_ASSERT_TRUE(openSealedPayload(dm_a, dm_b.publicIdentity(), true, channel,
                                            aad.data(), sealed.data(), sealed.size(), opened,
                                            sizeof(opened), opened_len));
        TEST_ASSERT_EQUAL_UINT32((unsigned)plaintext.size(), (unsigned)opened_len);
        if (!plaintext.empty()) {
            TEST_ASSERT_EQUAL_MEMORY(plaintext.data(), opened, opened_len);
        }
    }
    // Empty plaintext uses the same DM key/AAD as the first dm case (no seeds repeated).
    const json::Value* empties = root.get("dm_empty_plaintext");
    if (empties != nullptr && have_dm_keys) {
        for (const json::Value& c : empties->array) {
            std::vector<uint8_t> sealed;
            std::vector<uint8_t> aad;
            TEST_ASSERT_TRUE(fieldBytes(c, "sealed_hex", sealed));
            TEST_ASSERT_TRUE(fieldBytes(c, "aad_hex", aad));
            uint8_t opened[kMaxBody];
            size_t opened_len = 0;
            const bool ok = openSealedPayload(dm_b, dm_a.publicIdentity(), false, ChannelKey{},
                                               aad.data(), sealed.data(), sealed.size(), opened,
                                               sizeof(opened), opened_len);
            TEST_ASSERT_TRUE(ok);
            TEST_ASSERT_EQUAL_UINT32(0, (unsigned)opened_len);
        }
    }
    // Complete body layouts: flags | msg_ref | [identity] | nonce_prefix | ct | tag.
    const json::Value* layouts = root.get("body_layouts");
    TEST_ASSERT_NOT_NULL(layouts);
    for (const json::Value& c : layouts->array) {
        std::vector<uint8_t> body;
        std::vector<uint8_t> sealed;
        TEST_ASSERT_TRUE(fieldBytes(c, "body_hex", body));
        TEST_ASSERT_TRUE(fieldBytes(c, "sealed_hex", sealed));
        SealedBodyView view;
        TEST_ASSERT_TRUE(SealedBodyView::parse(body.data(), body.size(), view));
        TEST_ASSERT_EQUAL_UINT8((uint8_t)fieldU64(c, "payload_flags"), view.payload_flags);
        TEST_ASSERT_EQUAL_UINT32((uint32_t)fieldU64(c, "msg_ref"), view.msg_ref);
        TEST_ASSERT_EQUAL_UINT32((unsigned)sealed.size(), (unsigned)view.sealed_len);
        TEST_ASSERT_EQUAL_MEMORY(sealed.data(), view.sealed, sealed.size());
        if ((view.payload_flags & kPayloadFlagIdentityIncluded) != 0) {
            TEST_ASSERT_NOT_NULL(view.identity);
            std::vector<uint8_t> expected_identity;
            TEST_ASSERT_TRUE(fieldBytes(c, "identity_hex", expected_identity));
            TEST_ASSERT_EQUAL_UINT32(kPublicIdentityLen, (unsigned)expected_identity.size());
            TEST_ASSERT_EQUAL_MEMORY(expected_identity.data(), view.identity, kPublicIdentityLen);
        } else {
            TEST_ASSERT_NULL(view.identity);
        }
    }
    // Tampered ciphertext must fail to open with the DM key (Decrypt).
    const json::Value* negatives = root.get("negative");
    TEST_ASSERT_NOT_NULL(negatives);
    for (const json::Value& c : negatives->array) {
        if (!have_dm_keys) continue;
        std::vector<uint8_t> sealed;
        std::vector<uint8_t> aad;
        TEST_ASSERT_TRUE(fieldBytes(c, "sealed_hex", sealed));
        TEST_ASSERT_TRUE(fieldBytes(c, "aad_hex", aad));
        uint8_t opened[kMaxBody];
        size_t opened_len = 0;
        TEST_ASSERT_FALSE(openSealedPayload(dm_b, dm_a.publicIdentity(), false, ChannelKey{},
                                             aad.data(), sealed.data(), sealed.size(), opened,
                                             sizeof(opened), opened_len));
    }
}

// --- app_message.json ------------------------------------------------------------------------
void test_vectors_app_message() {
    json::Value root;
    std::string path;
    if (!loadOrSkip("app_message.json", root, path)) return;
    std::printf("  replaying %s\n", path.c_str());
    const json::Value* cases = root.get("cases");
    TEST_ASSERT_NOT_NULL(cases);
    for (const json::Value& c : cases->array) {
        std::vector<uint8_t> cbor;
        TEST_ASSERT_TRUE(fieldBytes(c, "cbor_hex", cbor));
        AppMessage message;
        TEST_ASSERT_TRUE(decodeAppMessage(cbor.data(), cbor.size(), message));
        const json::Value* readable = c.get("message");
        TEST_ASSERT_NOT_NULL(readable);
        TEST_ASSERT_EQUAL_UINT32((uint32_t)fieldU64(*readable, "id"), message.id);
        TEST_ASSERT_EQUAL_STRING(fieldString(*readable, "kind").c_str(), kindToString(message.kind).c_str());
        TEST_ASSERT_EQUAL_UINT64(fieldU64(*readable, "sent_at_ms"), message.sent_at_ms);
        const json::Value* text = readable->get("text");
        if (text != nullptr) TEST_ASSERT_EQUAL_STRING(text->asString().c_str(), message.text.data);
        const json::Value* reply = readable->get("reply_to");
        if (reply != nullptr) {
            TEST_ASSERT_TRUE(message.has_reply_to);
            TEST_ASSERT_EQUAL_UINT32((uint32_t)reply->asU64(), message.reply_to);
        }
        // Canonical CBOR re-encode must reproduce the Rust bytes exactly. Attachment/thread
        // metadata is not modelled by the firmware AppMessage (the app owns it), so those
        // cases only assert the fields above.
        const bool fully_modelled = readable->get("attachment") == nullptr &&
                                    readable->get("thread") == nullptr &&
                                    readable->get("sender") == nullptr &&
                                    readable->get("signature") == nullptr;
        if (!fully_modelled) continue;
        uint8_t reencoded[kMaxBody];
        const size_t reencoded_len = encodeAppMessage(message, reencoded, sizeof(reencoded));
        TEST_ASSERT_EQUAL_UINT32((unsigned)cbor.size(), (unsigned)reencoded_len);
        TEST_ASSERT_EQUAL_MEMORY(cbor.data(), reencoded, cbor.size());
    }
    const json::Value* acks = root.get("ack");
    TEST_ASSERT_NOT_NULL(acks);
    for (const json::Value& c : acks->array) {
        std::vector<uint8_t> cbor;
        TEST_ASSERT_TRUE(fieldBytes(c, "cbor_hex", cbor));
        Ack ack;
        TEST_ASSERT_TRUE(decodeAck(cbor.data(), cbor.size(), ack));
        const json::Value* readable = c.get("ack");
        TEST_ASSERT_NOT_NULL(readable);
        TEST_ASSERT_EQUAL_STRING(fieldString(*readable, "kind").c_str(), ackKindName(ack.kind));
        TEST_ASSERT_EQUAL_UINT32((uint32_t)fieldU64(*readable, "msg_id"), ack.msg_id);
        TEST_ASSERT_EQUAL_UINT64(fieldU64(*readable, "timestamp_ms"), ack.timestamp_ms);
        uint8_t reencoded[256];
        const size_t reencoded_len = encodeAck(ack, reencoded, sizeof(reencoded));
        TEST_ASSERT_EQUAL_UINT32((unsigned)cbor.size(), (unsigned)reencoded_len);
        TEST_ASSERT_EQUAL_MEMORY(cbor.data(), reencoded, cbor.size());
    }
    const json::Value* errors = root.get("decode_errors");
    TEST_ASSERT_NOT_NULL(errors);
    for (const json::Value& c : errors->array) {
        std::vector<uint8_t> cbor;
        TEST_ASSERT_TRUE(fieldBytes(c, "cbor_hex", cbor));
        AppMessage message;
        TEST_ASSERT_FALSE(decodeAppMessage(cbor.data(), cbor.size(), message));
    }
}

// --- resource.json ---------------------------------------------------------------------------
void test_vectors_resource() {
    json::Value root;
    std::string path;
    if (!loadOrSkip("resource.json", root, path)) return;
    std::printf("  replaying %s\n", path.c_str());
    const json::Value* blob = root.get("blob");
    TEST_ASSERT_NOT_NULL(blob);
    std::vector<uint8_t> sealed_blob;
    std::vector<uint8_t> plaintext;
    TEST_ASSERT_TRUE(fieldBytes(*blob, "sealed_blob_hex", sealed_blob));
    TEST_ASSERT_TRUE(fieldBytes(*blob, "plaintext_hex", plaintext));
    const uint32_t res_id = (uint32_t)fieldU64(*blob, "res_id");
    const uint32_t msg_id = (uint32_t)fieldU64(*blob, "msg_id");
    const uint32_t total_len = (uint32_t)sealed_blob.size();
    const uint16_t chunk_bytes = (uint16_t)fieldU64(root, "chunk_bytes", 180);
    TEST_ASSERT_EQUAL_UINT32((uint32_t)fieldU64(*blob, "plaintext_len"), (uint32_t)plaintext.size());
    TEST_ASSERT_EQUAL_UINT32((uint32_t)fieldU64(*blob, "chunk_count"),
                             (total_len + chunk_bytes - 1) / chunk_bytes);
    // Blob layout: flags(0) || nonce_prefix(12) || ct || tag.
    TEST_ASSERT_EQUAL_UINT8(0, sealed_blob[0]);
    std::vector<uint8_t> nonce_prefix;
    TEST_ASSERT_TRUE(fieldBytes(*blob, "nonce_prefix_hex", nonce_prefix));
    TEST_ASSERT_EQUAL_MEMORY(nonce_prefix.data(), sealed_blob.data() + 1, kNoncePrefixLen);
    TEST_ASSERT_EQUAL_UINT32(1 + kNoncePrefixLen + plaintext.size() + kAeadTagLen,
                             (uint32_t)sealed_blob.size());
    // AAD is 34 bytes and binds the manifest parameters (but not the sha256: circular).
    uint8_t aad[kResourceAadLen];
    resourceAad(res_id, msg_id, total_len, chunk_bytes, aad);
    std::vector<uint8_t> expected_aad;
    TEST_ASSERT_TRUE(fieldBytes(*blob, "aad_hex", expected_aad));
    TEST_ASSERT_EQUAL_UINT32((uint32_t)expected_aad.size(), (uint32_t)kResourceAadLen);
    TEST_ASSERT_EQUAL_MEMORY(expected_aad.data(), aad, kResourceAadLen);
    // Manifest: CBOR round-trip + sha256 over the sealed blob.
    const json::Value* manifest_json = root.get("manifest");
    TEST_ASSERT_NOT_NULL(manifest_json);
    std::vector<uint8_t> manifest_cbor;
    TEST_ASSERT_TRUE(fieldBytes(*manifest_json, "cbor_hex", manifest_cbor));
    ResourceManifest manifest;
    TEST_ASSERT_TRUE(decodeResourceManifest(manifest_cbor.data(), manifest_cbor.size(), manifest));
    uint8_t reencoded_manifest[512];
    const size_t reencoded_len = encodeResourceManifest(manifest, reencoded_manifest,
                                                         sizeof(reencoded_manifest));
    TEST_ASSERT_EQUAL_UINT32((unsigned)manifest_cbor.size(), (unsigned)reencoded_len);
    TEST_ASSERT_EQUAL_MEMORY(manifest_cbor.data(), reencoded_manifest, manifest_cbor.size());
    uint8_t digest[32];
    om_sha256(sealed_blob.data(), sealed_blob.size(), digest);
    TEST_ASSERT_EQUAL_MEMORY(manifest.sha256, digest, 32);
    const json::Value* manifest_readable = manifest_json->get("json");
    if (manifest_readable != nullptr) {
        TEST_ASSERT_EQUAL_UINT32((uint32_t)fieldU64(*manifest_readable, "total_len"),
                                 manifest.total_len);
        TEST_ASSERT_EQUAL_UINT32((uint32_t)fieldU64(*manifest_readable, "chunk_count"),
                                 manifest.chunk_count);
    }
    // Chunk bodies reassemble the sealed blob: flags(0) | msg_ref | res_id | index | slice.
    const json::Value* chunks = root.get("chunks");
    TEST_ASSERT_NOT_NULL(chunks);
    std::vector<uint8_t> reassembled;
    for (const json::Value& c : chunks->array) {
        std::vector<uint8_t> body;
        std::vector<uint8_t> slice;
        TEST_ASSERT_TRUE(fieldBytes(c, "body_hex", body));
        TEST_ASSERT_TRUE(fieldBytes(c, "slice_hex", slice));
        const uint16_t index = (uint16_t)fieldU64(c, "index");
        TEST_ASSERT_EQUAL_UINT8(0, body[0]);
        TEST_ASSERT_EQUAL_UINT32(msg_id, readU32Be(body.data() + 1));
        TEST_ASSERT_EQUAL_UINT32(res_id, readU32Be(body.data() + 5));
        TEST_ASSERT_EQUAL_UINT16(index, readU16Be(body.data() + 9));
        TEST_ASSERT_EQUAL_UINT32((unsigned)slice.size(), (unsigned)(body.size() - 11));
        TEST_ASSERT_EQUAL_MEMORY(slice.data(), body.data() + 11, slice.size());
        reassembled.insert(reassembled.end(), slice.begin(), slice.end());
    }
    TEST_ASSERT_EQUAL_UINT32((unsigned)sealed_blob.size(), (unsigned)reassembled.size());
    TEST_ASSERT_EQUAL_MEMORY(sealed_blob.data(), reassembled.data(), sealed_blob.size());
    // Channel blob: same AAD layout with a channel key/address.
    const json::Value* channel_blob = root.get("channel_blob");
    if (channel_blob != nullptr) {
        std::vector<uint8_t> psk;
        TEST_ASSERT_TRUE(fieldBytes(*channel_blob, "psk_hex", psk));
        ChannelKey key;
        TEST_ASSERT_TRUE(ChannelKey::derive(psk.data(),
                                            fieldString(*channel_blob, "channel_name").c_str(),
                                            fieldString(*channel_blob, "channel_name").size(),
                                            key));
        std::vector<uint8_t> expected_channel_id;
        TEST_ASSERT_TRUE(fieldBytes(*channel_blob, "channel_id_hex", expected_channel_id));
        TEST_ASSERT_EQUAL_MEMORY(expected_channel_id.data(), key.id.b, 8);
        std::vector<uint8_t> channel_aad;
        TEST_ASSERT_TRUE(fieldBytes(*channel_blob, "aad_hex", channel_aad));
        TEST_ASSERT_EQUAL_UINT32(34, (uint32_t)channel_aad.size());
        TEST_ASSERT_EQUAL_MEMORY("openmesh/v1/resource", channel_aad.data(), 20);
    }
    // ACK bitmap semantics: bit i reports chunk next_expected + 1 + i.
    const json::Value* bitmaps = root.get("ack_bitmaps");
    TEST_ASSERT_NOT_NULL(bitmaps);
    for (const json::Value& c : bitmaps->array) {
        const uint16_t next_expected = (uint16_t)fieldU64(c, "next_expected");
        const uint32_t total = (uint32_t)fieldU64(c, "total_chunks");
        const uint32_t bitmap = (uint32_t)fieldU64(c, "bitmap");
        std::vector<uint32_t> received;
        const json::Value* recv = c.get("received");
        if (recv != nullptr) {
            for (const json::Value& item : recv->array) received.push_back((uint32_t)item.asU64());
        }
        for (uint32_t bit = 0; bit < 32; ++bit) {
            const uint32_t chunk = (uint32_t)next_expected + 1 + bit;
            bool expected_bit = false;

            if (chunk < total) {
                for (uint32_t r : received) {
                    if (r == chunk) {
                        expected_bit = true;
                        break;
                    }
                }
            }
            TEST_ASSERT_TRUE(((bitmap >> bit) & 1u) == (expected_bit ? 1u : 0u));
        }
        // "received" entries at or before next_expected must be present (already contiguous).
        for (uint32_t r : received) {
            if (r < next_expected) continue;  // historical chunks are not reported
            if (r == next_expected) continue;
            TEST_ASSERT_TRUE((bitmap >> (r - next_expected - 1)) & 1u);
        }
    }
}

// --- companion.json --------------------------------------------------------------------------
void test_vectors_companion() {
    json::Value root;
    std::string path;
    if (!loadOrSkip("companion.json", root, path)) return;
    std::printf("  replaying %s\n", path.c_str());
    // CRC-16/CCITT-FALSE known answers.
    const json::Value* crcs = root.get("crc16_ccitt_false");
    if (crcs != nullptr) {
        for (const json::Value& c : crcs->array) {
            const std::string input = fieldString(c, "input_ascii");
            const uint16_t expected = (uint16_t)fieldU64(c, "crc16");
            TEST_ASSERT_EQUAL_UINT16(expected,
                                     crc16CcittFalse((const uint8_t*)input.data(), input.size()));
        }
    }
    const json::Value* cases = root.get("cases");
    TEST_ASSERT_NOT_NULL(cases);
    for (const json::Value& c : cases->array) {
        const CompanionType type = (CompanionType)(uint8_t)fieldU64(c, "type");
        const json::Value* payload = c.get("payload");
        TEST_ASSERT_NOT_NULL(payload);
        CompanionMsg msg = companionFromJson(type, *payload);
        // 1) payload CBOR byte-for-byte.
        std::vector<uint8_t> expected_payload;
        TEST_ASSERT_TRUE(fieldBytes(c, "payload_cbor_hex", expected_payload));
        uint8_t payload_buffer[2048];
        const size_t payload_len = encodeCompanionPayload(msg, payload_buffer,
                                                          sizeof(payload_buffer));
        TEST_ASSERT_EQUAL_UINT32((unsigned)expected_payload.size(), (unsigned)payload_len);
        TEST_ASSERT_EQUAL_MEMORY(expected_payload.data(), payload_buffer, payload_len);
        // 2) decoded payload matches the readable JSON.
        CompanionMsg decoded_payload;
        TEST_ASSERT_TRUE(decodeCompanionPayload(type, expected_payload.data(),
                                                expected_payload.size(), decoded_payload));
        TEST_ASSERT_EQUAL_UINT8((uint8_t)type, (uint8_t)decoded_payload.type);
        // 3) whole chunk (magic/type/len/payload/crc).
        std::vector<uint8_t> expected_frame;
        TEST_ASSERT_TRUE(fieldBytes(c, "frame_hex", expected_frame));
        uint8_t frame_buffer[4096];
        const size_t frame_len = encodeCompanionChunk(msg, frame_buffer, sizeof(frame_buffer));
        TEST_ASSERT_EQUAL_UINT32((unsigned)expected_frame.size(), (unsigned)frame_len);
        TEST_ASSERT_EQUAL_MEMORY(expected_frame.data(), frame_buffer, frame_len);
        // 4) decode the chunk and compare the interesting fields.
        CompanionMsg decoded;
        TEST_ASSERT_TRUE(decodeCompanionChunk(expected_frame.data(), expected_frame.size(),
                                             decoded));
        switch (type) {
            case CompanionType::Hello:
                TEST_ASSERT_EQUAL_UINT8(msg.proto, decoded.proto);
                TEST_ASSERT_EQUAL_STRING(msg.app_version, decoded.app_version);
                TEST_ASSERT_EQUAL_INT((int)msg.wants_events, (int)decoded.wants_events);
                break;
            case CompanionType::HelloAck:
                TEST_ASSERT_TRUE(msg.node_id == decoded.node_id);
                TEST_ASSERT_EQUAL_UINT16(msg.max_frame, decoded.max_frame);
                TEST_ASSERT_EQUAL_UINT16(msg.caps, decoded.caps);
                TEST_ASSERT_EQUAL_STRING(msg.region, decoded.region);
                TEST_ASSERT_EQUAL_STRING(msg.profile, decoded.profile);
                break;
            case CompanionType::ConfigSet:
                TEST_ASSERT_EQUAL_STRING(msg.key, decoded.key);
                TEST_ASSERT_EQUAL_STRING(msg.value, decoded.value);
                break;
            case CompanionType::Config:
            case CompanionType::Stats:
                TEST_ASSERT_EQUAL_UINT8(msg.entry_count, decoded.entry_count);
                for (uint8_t i = 0; i < msg.entry_count; ++i) {
                    TEST_ASSERT_EQUAL_STRING(msg.entries[i].key, decoded.entries[i].key);
                    TEST_ASSERT_EQUAL_STRING(msg.entries[i].value, decoded.entries[i].value);
                }
                break;
            case CompanionType::Tx:
            case CompanionType::Rx:
                TEST_ASSERT_EQUAL_UINT16(msg.frame_len, decoded.frame_len);
                TEST_ASSERT_EQUAL_MEMORY(msg.frame, decoded.frame, msg.frame_len);
                break;
            case CompanionType::Ping:
            case CompanionType::Pong:
                TEST_ASSERT_EQUAL_UINT32(msg.nonce, decoded.nonce);
                break;
            case CompanionType::Event:
                TEST_ASSERT_EQUAL_STRING(msg.kind, decoded.kind);
                TEST_ASSERT_EQUAL_STRING(msg.detail, decoded.detail);
                break;
            default:
                break;
        }
        // 5) SLIP encoding round-trip.
        std::vector<uint8_t> expected_slip;
        TEST_ASSERT_TRUE(fieldBytes(c, "slip_hex", expected_slip));
        uint8_t slip_buffer[8192];
        const size_t slip_len = slipEncode(expected_frame.data(), expected_frame.size(),
                                           slip_buffer, sizeof(slip_buffer));
        TEST_ASSERT_EQUAL_UINT32((unsigned)expected_slip.size(), (unsigned)slip_len);
        TEST_ASSERT_EQUAL_MEMORY(expected_slip.data(), slip_buffer, slip_len);
        SlipDecoder decoder;
        bool got = false;
        for (size_t i = 0; i < slip_len; ++i) got = decoder.feed(slip_buffer[i]) || got;
        TEST_ASSERT_TRUE(got);
        TEST_ASSERT_EQUAL_UINT32(1, decoder.framesDecoded());
        TEST_ASSERT_EQUAL_MEMORY(expected_frame.data(), decoder.chunk(), decoder.chunkLen());
    }
    const json::Value* errors = root.get("error_cases");
    TEST_ASSERT_NOT_NULL(errors);
    for (const json::Value& c : errors->array) {
        std::vector<uint8_t> frame;
        TEST_ASSERT_TRUE(fieldBytes(c, "frame_hex", frame));
        CompanionMsg decoded;
        TEST_ASSERT_FALSE(decodeCompanionChunk(frame.data(), frame.size(), decoded));
    }
}
