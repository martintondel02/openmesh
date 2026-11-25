// SPDX-License-Identifier: AGPL-3.0-or-later
#include "openmesh/om_messages.h"

#include <cstring>

#include "openmesh/om_cbor.h"

namespace om {

namespace {

constexpr char kAnnounceDomain[] = "openmesh/v1/announce";
constexpr size_t kAnnounceDomainLen = sizeof(kAnnounceDomain) - 1;  // 20
constexpr char kGroupDomain[] = "openmesh/v1/group-msg";
constexpr size_t kGroupDomainLen = sizeof(kGroupDomain) - 1;  // 22

bool textEquals(const uint8_t* text, size_t len, const char* expect) {
    const size_t n = std::strlen(expect);
    return len == n && std::memcmp(text, expect, n) == 0;
}

bool keyIs(const uint8_t* key, size_t key_len, const char* expected) {
    return textEquals(key, key_len, expected);
}

}  // namespace

// --- Enums ----------------------------------------------------------------------------------
const char* nodeClassName(NodeClass c) {
    switch (c) {
        case NodeClass::Client: return "client";
        case NodeClass::Repeater: return "repeater";
        case NodeClass::Gateway: return "gateway";
        case NodeClass::RoomServer: return "room_server";
        case NodeClass::Tracker: return "tracker";
    }
    return "client";
}

bool parseNodeClass(const uint8_t* text, size_t len, NodeClass& out) {
    if (textEquals(text, len, "client")) { out = NodeClass::Client; return true; }
    if (textEquals(text, len, "repeater")) { out = NodeClass::Repeater; return true; }
    if (textEquals(text, len, "gateway")) { out = NodeClass::Gateway; return true; }
    if (textEquals(text, len, "room_server")) { out = NodeClass::RoomServer; return true; }
    if (textEquals(text, len, "tracker")) { out = NodeClass::Tracker; return true; }
    return false;
}

const char* messageKindName(MessageKind k) {
    switch (k) {
        case MessageKind::Text: return "text";
        case MessageKind::Position: return "position";
        case MessageKind::Attachment: return "attachment";
        case MessageKind::GroupInvite: return "group_invite";
        case MessageKind::Telemetry: return "telemetry";
        case MessageKind::Control: return "control";
    }
    return "text";
}

const char* ackKindName(AckKind k) {
    switch (k) {
        case AckKind::Delivered: return "delivered";
        case AckKind::Read: return "read";
        case AckKind::ResourceWindow: return "resource_window";
        case AckKind::ResourceComplete: return "resource_complete";
    }
    return "delivered";
}

bool parseAckKind(const uint8_t* text, size_t len, AckKind& out) {
    if (textEquals(text, len, "delivered")) { out = AckKind::Delivered; return true; }
    if (textEquals(text, len, "read")) { out = AckKind::Read; return true; }
    if (textEquals(text, len, "resource_window")) { out = AckKind::ResourceWindow; return true; }
    if (textEquals(text, len, "resource_complete")) {
        out = AckKind::ResourceComplete;
        return true;
    }
    return false;
}

const char* resourceKindName(ResourceKind k) {
    switch (k) {
        case ResourceKind::Attachment: return "attachment";
        case ResourceKind::LongText: return "long_text";
        case ResourceKind::Avatar: return "avatar";
        case ResourceKind::ChannelHistory: return "channel_history";
    }
    return "attachment";
}

// --- Announce (binary layout, PROTOCOL.md §6) ----------------------------------------------
void Announce::setName(const char* n, size_t len) {
    if (len > kAnnounceMaxNameLen) len = kAnnounceMaxNameLen;
    std::memcpy(name, n, len);
    name_len = (uint8_t)len;
}

size_t encodeAnnounceUnsigned(const Announce& a, uint8_t* out, size_t capacity) {
    const size_t total = kAnnounceFixedLen + a.name_len;
    if (capacity < total) return 0;
    uint8_t ident_bytes[kPublicIdentityLen];
    a.ident.toBytes(ident_bytes);
    out[0] = a.version;
    out[1] = (uint8_t)a.cls;
    writeU16Be(out + 2, a.caps);
    out[4] = 0;  // reserved, MUST be 0 (hop count is derived from TTL)
    out[5] = a.name_len;
    writeU16Be(out + 6, a.interval_s);
    writeU64Be(out + 8, a.timestamp_ms);
    std::memcpy(out + 16, ident_bytes, kPublicIdentityLen);
    std::memcpy(out + kAnnounceFixedLen, a.name, a.name_len);
    return total;
}

size_t encodeAnnounceBody(const Announce& a, const uint8_t signature[kSignatureLen],
                          uint8_t* out, size_t capacity) {
    const size_t unsigned_len = encodeAnnounceUnsigned(a, out, capacity);
    if (unsigned_len == 0) return 0;
    if (capacity < unsigned_len + kSignatureLen) return 0;
    std::memcpy(out + unsigned_len, signature, kSignatureLen);
    return unsigned_len + kSignatureLen;
}

bool decodeAnnounceBody(const uint8_t* body, size_t len, Announce& out,
                        const uint8_t*& signature) {
    if (len < kAnnounceBodyLen || len > kMtu) return false;
    out = Announce{};
    out.version = body[0];
    out.cls = (NodeClass)body[1];
    out.caps = readU16Be(body + 2);
    if (body[4] != 0) return false;  // reserved byte must be zero
    out.name_len = body[5];
    if (out.name_len > kAnnounceMaxNameLen) return false;
    if (len != kAnnounceFixedLen + (size_t)out.name_len + kSignatureLen) return false;
    out.interval_s = readU16Be(body + 6);
    out.timestamp_ms = readU64Be(body + 8);
    if (!PublicIdentity::fromBytes(body + 16, kPublicIdentityLen, out.ident)) return false;
    std::memcpy(out.name, body + kAnnounceFixedLen, out.name_len);
    out.name[out.name_len] = '\0';
    signature = body + kAnnounceFixedLen + out.name_len;
    return true;
}

size_t announceSignaturePayload(const NodeId& src, uint32_t seq, const uint8_t* body_unsigned,
                                size_t body_len, uint8_t* out, size_t capacity) {
    const size_t need = kAnnounceDomainLen + 8 + 4 + body_len;
    if (capacity < need) return 0;
    size_t off = 0;
    std::memcpy(out + off, kAnnounceDomain, kAnnounceDomainLen);
    off += kAnnounceDomainLen;
    std::memcpy(out + off, src.b, 8);
    off += 8;
    writeU32Be(out + off, seq);
    off += 4;
    std::memcpy(out + off, body_unsigned, body_len);
    off += body_len;
    return off;
}

// --- AppMessage ----------------------------------------------------------------------------
namespace {

bool encodePosition(const Position& p, CborWriter& w) {
    size_t count = 3;  // lat_e7, lon_e7, timestamp_ms
    if (p.has_alt) ++count;
    if (p.has_accuracy) ++count;
    if (p.has_speed) ++count;
    if (p.has_heading) ++count;
    if (!w.mapHeader(count)) return false;
    if (!w.key("lat_e7")) return false;
    if (!w.int64Value(p.lat_e7)) return false;
    if (!w.key("lon_e7")) return false;
    if (!w.int64Value(p.lon_e7)) return false;
    if (p.has_alt) {
        if (!w.key("alt_m")) return false;
        if (!w.int64Value(p.alt_m)) return false;
    }
    if (p.has_accuracy) {
        if (!w.key("accuracy_m")) return false;
        if (!w.uint(p.accuracy_m)) return false;
    }
    if (p.has_speed) {
        if (!w.key("speed_cm_s")) return false;
        if (!w.uint(p.speed_cm_s)) return false;
    }
    if (p.has_heading) {
        if (!w.key("heading_deg")) return false;
        if (!w.uint(p.heading_deg)) return false;
    }
    if (!w.key("timestamp_ms")) return false;
    if (!w.uint(p.timestamp_ms)) return false;
    return true;
}

bool encodeTelemetry(const Telemetry& t, CborWriter& w) {
    size_t count = 1;  // timestamp_ms
    if (t.has_battery_mv) ++count;
    if (t.has_battery_pct) ++count;
    if (t.has_uptime_s) ++count;
    if (t.has_airtime_used_ms) ++count;
    if (t.has_rx_frames) ++count;
    if (t.has_tx_frames) ++count;
    if (t.has_temperature_c) ++count;
    if (t.has_solar_mv) ++count;
    if (!w.mapHeader(count)) return false;
    if (!w.key("timestamp_ms")) return false;
    if (!w.uint(t.timestamp_ms)) return false;
    if (t.has_battery_mv) {
        if (!w.key("battery_mv")) return false;
        if (!w.uint(t.battery_mv)) return false;
    }
    if (t.has_battery_pct) {
        if (!w.key("battery_pct")) return false;
        if (!w.uint(t.battery_pct)) return false;
    }
    if (t.has_uptime_s) {
        if (!w.key("uptime_s")) return false;
        if (!w.uint(t.uptime_s)) return false;
    }
    if (t.has_airtime_used_ms) {
        if (!w.key("airtime_used_ms")) return false;
        if (!w.uint(t.airtime_used_ms)) return false;
    }
    if (t.has_rx_frames) {
        if (!w.key("rx_frames")) return false;
        if (!w.uint(t.rx_frames)) return false;
    }
    if (t.has_tx_frames) {
        if (!w.key("tx_frames")) return false;
        if (!w.uint(t.tx_frames)) return false;
    }
    if (t.has_temperature_c) {
        if (!w.key("temperature_c")) return false;
        if (!w.int64Value(t.temperature_c)) return false;
    }
    if (t.has_solar_mv) {
        if (!w.key("solar_mv")) return false;
        if (!w.uint(t.solar_mv)) return false;
    }
    return true;
}

bool decodePosition(CborReader& r, Position& p) {
    size_t pairs = 0;
    if (!r.mapHeader(pairs)) return false;
    p = Position{};
    for (size_t i = 0; i < pairs && r.ok(); ++i) {
        const uint8_t* key = nullptr;
        size_t key_len = 0;
        if (!r.readKeySpan(key, key_len)) return false;
        if (keyIs(key, key_len, "lat_e7")) {
            int64_t v = 0;
            if (!r.readInt(v)) return false;
            p.lat_e7 = (int32_t)v;
        } else if (keyIs(key, key_len, "lon_e7")) {
            int64_t v = 0;
            if (!r.readInt(v)) return false;
            p.lon_e7 = (int32_t)v;
        } else if (keyIs(key, key_len, "alt_m")) {
            int64_t v = 0;
            if (!r.readInt(v)) return false;
            p.alt_m = (int16_t)v;
            p.has_alt = true;
        } else if (keyIs(key, key_len, "accuracy_m")) {
            if (!r.readU8(p.accuracy_m)) return false;
            p.has_accuracy = true;
        } else if (keyIs(key, key_len, "speed_cm_s")) {
            if (!r.readU16(p.speed_cm_s)) return false;
            p.has_speed = true;
        } else if (keyIs(key, key_len, "heading_deg")) {
            if (!r.readU16(p.heading_deg)) return false;
            p.has_heading = true;
        } else if (keyIs(key, key_len, "timestamp_ms")) {
            if (!r.readUint(p.timestamp_ms)) return false;
        } else if (!r.skipValue()) {
            return false;
        }
    }
    return r.ok();
}

bool decodeTelemetry(CborReader& r, Telemetry& t) {
    size_t pairs = 0;
    if (!r.mapHeader(pairs)) return false;
    t = Telemetry{};
    for (size_t i = 0; i < pairs && r.ok(); ++i) {
        const uint8_t* key = nullptr;
        size_t key_len = 0;
        if (!r.readKeySpan(key, key_len)) return false;
        if (keyIs(key, key_len, "timestamp_ms")) {
            if (!r.readUint(t.timestamp_ms)) return false;
        } else if (keyIs(key, key_len, "battery_mv")) {
            if (!r.readU16(t.battery_mv)) return false;
            t.has_battery_mv = true;
        } else if (keyIs(key, key_len, "battery_pct")) {
            if (!r.readU8(t.battery_pct)) return false;
            t.has_battery_pct = true;
        } else if (keyIs(key, key_len, "uptime_s")) {
            if (!r.readU32(t.uptime_s)) return false;
            t.has_uptime_s = true;
        } else if (keyIs(key, key_len, "airtime_used_ms")) {
            if (!r.readU32(t.airtime_used_ms)) return false;
            t.has_airtime_used_ms = true;
        } else if (keyIs(key, key_len, "rx_frames")) {
            if (!r.readU32(t.rx_frames)) return false;
            t.has_rx_frames = true;
        } else if (keyIs(key, key_len, "tx_frames")) {
            if (!r.readU32(t.tx_frames)) return false;
            t.has_tx_frames = true;
        } else if (keyIs(key, key_len, "temperature_c")) {
            int64_t v = 0;
            if (!r.readInt(v)) return false;
            t.temperature_c = (int8_t)v;
            t.has_temperature_c = true;
        } else if (keyIs(key, key_len, "solar_mv")) {
            if (!r.readU16(t.solar_mv)) return false;
            t.has_solar_mv = true;
        } else if (!r.skipValue()) {
            return false;
        }
    }
    return r.ok();
}

}  // namespace

size_t encodeAppMessage(const AppMessage& m, uint8_t* out, size_t capacity) {
    CborWriter w(out, capacity);
    size_t count = 3;  // id, kind, sent_at_ms
    if (m.has_text) ++count;
    if (m.has_reply_to) ++count;
    if (m.has_position) ++count;
    if (m.has_telemetry) ++count;
    w.mapHeader(count);
    w.key("id");
    w.uint(m.id);
    w.key("kind");
    w.text(messageKindName(m.kind));
    w.key("sent_at_ms");
    w.uint(m.sent_at_ms);
    if (m.has_text) {
        w.key("text");
        w.text(m.text.data, m.text.len);
    }
    if (m.has_reply_to) {
        w.key("reply_to");
        w.uint(m.reply_to);
    }
    if (m.has_position) {
        w.key("position");
        if (!encodePosition(m.position, w)) return 0;
    }
    if (m.has_telemetry) {
        // Provisional extension: services.rs MessageKind::Telemetry has no AppMessage field
        // yet; the key is tolerated by serde on the Rust side. Documented in README.
        w.key("telemetry");
        if (!encodeTelemetry(m.telemetry, w)) return 0;
    }
    return w.ok() ? w.size() : 0;
}

bool decodeAppMessage(const uint8_t* cbor, size_t len, AppMessage& out) {
    CborReader r(cbor, len);
    size_t pairs = 0;
    if (!r.mapHeader(pairs)) return false;
    out = AppMessage{};
    bool have_id = false;
    bool have_kind = false;
    bool have_sent_at = false;
    for (size_t i = 0; i < pairs && r.ok(); ++i) {
        const uint8_t* key = nullptr;
        size_t key_len = 0;
        if (!r.readKeySpan(key, key_len)) return false;
        if (keyIs(key, key_len, "id")) {
            if (!r.readU32(out.id)) return false;
            have_id = true;
        } else if (keyIs(key, key_len, "kind")) {
            const uint8_t* p = nullptr;
            size_t n = 0;
            if (!r.readText(p, n)) return false;
            if (textEquals(p, n, "text")) out.kind = MessageKind::Text;
            else if (textEquals(p, n, "position")) out.kind = MessageKind::Position;
            else if (textEquals(p, n, "attachment")) out.kind = MessageKind::Attachment;
            else if (textEquals(p, n, "group_invite")) out.kind = MessageKind::GroupInvite;
            else if (textEquals(p, n, "telemetry")) out.kind = MessageKind::Telemetry;
            else if (textEquals(p, n, "control")) out.kind = MessageKind::Control;
            else return false;
            have_kind = true;
        } else if (keyIs(key, key_len, "sent_at_ms")) {
            if (!r.readUint(out.sent_at_ms)) return false;
            have_sent_at = true;
        } else if (keyIs(key, key_len, "text")) {
            const uint8_t* p = nullptr;
            size_t n = 0;
            if (!r.readText(p, n)) return false;
            if (n > sizeof(out.text.data)) n = sizeof(out.text.data);
            std::memcpy(out.text.data, p, n);
            out.text.len = n;
            out.has_text = true;
        } else if (keyIs(key, key_len, "reply_to")) {
            if (!r.readU32(out.reply_to)) return false;
            out.has_reply_to = true;
        } else if (keyIs(key, key_len, "position")) {
            if (!decodePosition(r, out.position)) return false;
            out.has_position = true;
        } else if (keyIs(key, key_len, "telemetry")) {
            if (!decodeTelemetry(r, out.telemetry)) return false;
            out.has_telemetry = true;
        } else if (!r.skipValue()) {
            return false;
        }
    }
    return r.ok() && have_id && have_kind && have_sent_at;
}

// --- Ack -----------------------------------------------------------------------------------
size_t encodeAck(const Ack& a, uint8_t* out, size_t capacity) {
    CborWriter w(out, capacity);
    size_t count = 3;  // kind, msg_id, timestamp_ms
    if (a.has_res_id) ++count;
    if (a.has_next_expected) ++count;
    if (a.has_bitmap) ++count;
    w.mapHeader(count);
    w.key("kind");
    w.text(ackKindName(a.kind));
    w.key("msg_id");
    w.uint(a.msg_id);
    if (a.has_res_id) {
        w.key("res_id");
        w.uint(a.res_id);
    }
    if (a.has_next_expected) {
        w.key("next_expected");
        w.uint(a.next_expected);
    }
    if (a.has_bitmap) {
        w.key("bitmap");
        w.uint(a.bitmap);
    }
    w.key("timestamp_ms");
    w.uint(a.timestamp_ms);
    return w.ok() ? w.size() : 0;
}

bool decodeAck(const uint8_t* cbor, size_t len, Ack& out) {
    CborReader r(cbor, len);
    size_t pairs = 0;
    if (!r.mapHeader(pairs)) return false;
    out = Ack{};
    bool have_kind = false;
    bool have_msg_id = false;
    bool have_timestamp = false;
    for (size_t i = 0; i < pairs && r.ok(); ++i) {
        const uint8_t* key = nullptr;
        size_t key_len = 0;
        if (!r.readKeySpan(key, key_len)) return false;
        if (keyIs(key, key_len, "kind")) {
            const uint8_t* p = nullptr;
            size_t n = 0;
            if (!r.readText(p, n)) return false;
            if (!parseAckKind(p, n, out.kind)) return false;
            have_kind = true;
        } else if (keyIs(key, key_len, "msg_id")) {
            if (!r.readU32(out.msg_id)) return false;
            have_msg_id = true;
        } else if (keyIs(key, key_len, "res_id")) {
            if (!r.readU32(out.res_id)) return false;
            out.has_res_id = true;
        } else if (keyIs(key, key_len, "next_expected")) {
            if (!r.readU16(out.next_expected)) return false;
            out.has_next_expected = true;
        } else if (keyIs(key, key_len, "bitmap")) {
            if (!r.readU32(out.bitmap)) return false;
            out.has_bitmap = true;
        } else if (keyIs(key, key_len, "timestamp_ms")) {
            if (!r.readUint(out.timestamp_ms)) return false;
            have_timestamp = true;
        } else if (!r.skipValue()) {
            return false;
        }
    }
    return r.ok() && have_kind && have_msg_id && have_timestamp;
}

// --- ResourceManifest ----------------------------------------------------------------------
size_t encodeResourceManifest(const ResourceManifest& manifest, uint8_t* out, size_t capacity) {
    CborWriter w(out, capacity);
    w.mapHeader(8);
    w.key("res_id");
    w.uint(manifest.res_id);
    w.key("msg_id");
    w.uint(manifest.msg_id);
    w.key("kind");
    w.text(resourceKindName(manifest.kind));
    w.key("mime");
    w.text(manifest.mime, manifest.mime_len);
    w.key("total_len");
    w.uint(manifest.total_len);
    w.key("chunk_bytes");
    w.uint(manifest.chunk_bytes);
    w.key("chunk_count");
    w.uint(manifest.chunk_count);
    w.key("sha256");
    w.bytes(manifest.sha256, sizeof(manifest.sha256));
    return w.ok() ? w.size() : 0;
}

bool decodeResourceManifest(const uint8_t* cbor, size_t len, ResourceManifest& out) {
    CborReader r(cbor, len);
    size_t pairs = 0;
    if (!r.mapHeader(pairs)) return false;
    out = ResourceManifest{};
    bool have_res_id = false;
    bool have_msg_id = false;
    bool have_kind = false;
    bool have_mime = false;
    bool have_total = false;
    bool have_chunk_bytes = false;
    bool have_chunk_count = false;
    bool have_sha = false;
    for (size_t i = 0; i < pairs && r.ok(); ++i) {
        const uint8_t* key = nullptr;
        size_t key_len = 0;
        if (!r.readKeySpan(key, key_len)) return false;
        if (keyIs(key, key_len, "res_id")) {
            if (!r.readU32(out.res_id)) return false;
            have_res_id = true;
        } else if (keyIs(key, key_len, "msg_id")) {
            if (!r.readU32(out.msg_id)) return false;
            have_msg_id = true;
        } else if (keyIs(key, key_len, "kind")) {
            const uint8_t* p = nullptr;
            size_t n = 0;
            if (!r.readText(p, n)) return false;
            if (textEquals(p, n, "attachment")) out.kind = ResourceKind::Attachment;
            else if (textEquals(p, n, "long_text")) out.kind = ResourceKind::LongText;
            else if (textEquals(p, n, "avatar")) out.kind = ResourceKind::Avatar;
            else if (textEquals(p, n, "channel_history")) out.kind = ResourceKind::ChannelHistory;
            else return false;
            have_kind = true;
        } else if (keyIs(key, key_len, "mime")) {
            const uint8_t* p = nullptr;
            size_t n = 0;
            if (!r.readText(p, n)) return false;
            if (n > sizeof(out.mime)) n = sizeof(out.mime);
            std::memcpy(out.mime, p, n);
            out.mime_len = (uint8_t)n;
            have_mime = true;
        } else if (keyIs(key, key_len, "total_len")) {
            if (!r.readU32(out.total_len)) return false;
            have_total = true;
        } else if (keyIs(key, key_len, "chunk_bytes")) {
            if (!r.readU16(out.chunk_bytes)) return false;
            have_chunk_bytes = true;
        } else if (keyIs(key, key_len, "chunk_count")) {
            if (!r.readU16(out.chunk_count)) return false;
            have_chunk_count = true;
        } else if (keyIs(key, key_len, "sha256")) {
            const uint8_t* p = nullptr;
            size_t n = 0;
            if (!r.readBytes(p, n)) return false;
            if (n != sizeof(out.sha256)) return false;
            std::memcpy(out.sha256, p, n);
            have_sha = true;
        } else if (!r.skipValue()) {
            return false;
        }
    }
    return r.ok() && have_res_id && have_msg_id && have_kind && have_mime && have_total &&
           have_chunk_bytes && have_chunk_count && have_sha;
}

// --- Store offers/wants --------------------------------------------------------------------
namespace {
size_t encodeIdsMap(const uint32_t* ids, uint8_t count, bool include_count, uint8_t* out,
                    size_t capacity) {
    CborWriter w(out, capacity);
    w.mapHeader(include_count ? 2 : 1);
    w.key("ids");
    w.arrayHeader(count);
    for (uint8_t i = 0; i < count; ++i) w.uint(ids[i]);
    if (include_count) {
        w.key("count");
        w.uint(count);
    }
    return w.ok() ? w.size() : 0;
}

bool decodeIdsMap(const uint8_t* cbor, size_t len, uint32_t* ids, uint8_t& count) {
    CborReader r(cbor, len);
    size_t pairs = 0;
    if (!r.mapHeader(pairs)) return false;
    count = 0;
    for (size_t i = 0; i < pairs && r.ok(); ++i) {
        const uint8_t* key = nullptr;
        size_t key_len = 0;
        if (!r.readKeySpan(key, key_len)) return false;
        if (keyIs(key, key_len, "ids")) {
            size_t items = 0;
            if (!r.arrayHeader(items)) return false;
            if (items > 16) items = 16;  // protocol caps offers at 16 ids per frame
            for (size_t k = 0; k < items; ++k) {
                uint32_t id = 0;
                if (!r.readU32(id)) return false;
                ids[count++] = id;
            }
        } else if (keyIs(key, key_len, "count")) {
            uint32_t ignored = 0;
            if (!r.readU32(ignored)) return false;
        } else if (!r.skipValue()) {
            return false;
        }
    }
    return r.ok();
}
}  // namespace

size_t encodeStoreOffer(const StoreOffer& offer, uint8_t* out, size_t capacity) {
    return encodeIdsMap(offer.ids, offer.count, true, out, capacity);
}

bool decodeStoreOffer(const uint8_t* cbor, size_t len, StoreOffer& out) {
    out = StoreOffer{};
    return decodeIdsMap(cbor, len, out.ids, out.count);
}

size_t encodeStoreWant(const StoreWant& want, uint8_t* out, size_t capacity) {
    return encodeIdsMap(want.ids, want.count, false, out, capacity);
}

bool decodeStoreWant(const uint8_t* cbor, size_t len, StoreWant& out) {
    out = StoreWant{};
    return decodeIdsMap(cbor, len, out.ids, out.count);
}

// --- GroupMessage (PROTOCOL.md §7.3) -------------------------------------------------------
size_t encodeGroupMessage(const NodeId& sender, const uint8_t signature[kSignatureLen],
                          const AppMessage& message, uint8_t* out, size_t capacity) {
    uint8_t scratch[kMaxBody];
    const size_t msg_len = encodeAppMessage(message, scratch, sizeof(scratch));
    if (msg_len == 0) return 0;
    CborWriter w(out, capacity);
    w.mapHeader(3);
    w.key("sender");
    w.bytes(sender.b, 8);
    w.key("signature");
    w.bytes(signature, kSignatureLen);
    w.key("message");
    w.rawItem(scratch, msg_len);
    return w.ok() ? w.size() : 0;
}

bool decodeGroupMessage(const uint8_t* cbor, size_t len, GroupMessageView& out) {
    CborReader r(cbor, len);
    size_t pairs = 0;
    if (!r.mapHeader(pairs)) return false;
    out = GroupMessageView{};
    bool have_sender = false;
    bool have_message = false;
    for (size_t i = 0; i < pairs && r.ok(); ++i) {
        const uint8_t* key = nullptr;
        size_t key_len = 0;
        if (!r.readKeySpan(key, key_len)) return false;
        if (keyIs(key, key_len, "sender")) {
            const uint8_t* p = nullptr;
            size_t n = 0;
            if (!r.readBytes(p, n)) return false;
            if (n != 8) return false;
            std::memcpy(out.sender.b, p, 8);
            have_sender = true;
        } else if (keyIs(key, key_len, "signature")) {
            const uint8_t* p = nullptr;
            size_t n = 0;
            if (!r.readBytes(p, n)) return false;
            out.signature = p;
            out.signature_len = n;
        } else if (keyIs(key, key_len, "message")) {
            size_t off = 0;
            size_t n = 0;
            if (!r.skipValueSpan(off, n)) return false;
            out.message_cbor = cbor + off;
            out.message_cbor_len = n;
            have_message = true;
        } else if (!r.skipValue()) {
            return false;
        }
    }
    return r.ok() && have_sender && have_message && out.signature != nullptr &&
           out.signature_len == kSignatureLen;
}

size_t groupSignaturePayload(const NodeId& sender, const NodeId& channel,
                             const uint8_t* message_cbor, size_t message_cbor_len, uint8_t* out,
                             size_t capacity) {
    const size_t need = kGroupDomainLen + 8 + 8 + message_cbor_len;
    if (capacity < need) return 0;
    size_t off = 0;
    std::memcpy(out + off, kGroupDomain, kGroupDomainLen);
    off += kGroupDomainLen;
    std::memcpy(out + off, sender.b, 8);
    off += 8;
    std::memcpy(out + off, channel.b, 8);
    off += 8;
    std::memcpy(out + off, message_cbor, message_cbor_len);
    return off + message_cbor_len;
}

// --- Resource blob AAD (PROTOCOL.md §9.1) ---------------------------------------------------
void resourceAad(uint32_t res_id, uint32_t msg_id, uint32_t total_len, uint16_t chunk_bytes,
                 uint8_t out[kResourceAadLen]) {
    static const char kDomain[] = "openmesh/v1/resource";
    std::memcpy(out, kDomain, sizeof(kDomain) - 1);
    writeU32Be(out + 20, res_id);
    writeU32Be(out + 24, msg_id);
    writeU32Be(out + 28, total_len);
    writeU16Be(out + 32, chunk_bytes);
}

}  // namespace om
