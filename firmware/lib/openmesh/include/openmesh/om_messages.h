// SPDX-License-Identifier: AGPL-3.0-or-later
// CBOR message schemas carried inside OMP/1 frames.
//
// Normative reference: crates/openmesh-core/src/services.rs + docs/architecture/PROTOCOL.md
// §6-§10. Field names are the CBOR map keys; `None` fields are omitted. The firmware only
// constructs the messages it originates itself (announce, position, telemetry, ack,
// store offer/want) and decodes enough of the rest to relay and surface them.
#ifndef OPENMESH_OM_MESSAGES_H
#define OPENMESH_OM_MESSAGES_H

#include <cstddef>
#include <cstdint>

#include "openmesh/om_types.h"

namespace om {

// --- Enumerations (serde `snake_case` strings on the wire) ---------------------------------
enum class NodeClass : uint8_t { Client, Repeater, Gateway, RoomServer, Tracker };
enum class MessageKind : uint8_t {
    Text,
    Position,
    Attachment,
    GroupInvite,
    Telemetry,
    Control,
};
enum class AckKind : uint8_t {
    Delivered,
    Read,
    ResourceWindow,
    ResourceComplete,
};
enum class ResourceKind : uint8_t {
    Attachment,
    LongText,
    Avatar,
    ChannelHistory,
};

const char* nodeClassName(NodeClass c);
bool parseNodeClass(const uint8_t* text, size_t len, NodeClass& out);
const char* messageKindName(MessageKind k);
const char* ackKindName(AckKind k);
bool parseAckKind(const uint8_t* text, size_t len, AckKind& out);
const char* resourceKindName(ResourceKind k);

// Capability bits (PROTOCOL.md §6.1).
constexpr uint16_t kCapRelay = 1u << 0;
constexpr uint16_t kCapStore = 1u << 1;
constexpr uint16_t kCapGps = 1u << 2;
constexpr uint16_t kCapWifi = 1u << 3;
constexpr uint16_t kCapEthernet = 1u << 4;
constexpr uint16_t kCapBle = 1u << 5;
constexpr uint16_t kCapUsb = 1u << 6;
constexpr uint16_t kCapScreen = 1u << 7;
constexpr uint16_t kCapAudioOut = 1u << 8;
constexpr uint16_t kCapSolar = 1u << 9;

// --- Announce (PROTOCOL.md §6, fixed binary layout — not CBOR) ------------------------------
// offset  size  field
// 0       1     version (1)
// 1       1     class (0=client,1=repeater,2=gateway,3=room_server,4=tracker)
// 2       2     caps (u16 BE)
// 4       1     reserved (MUST be 0)
// 5       1     name_len (0..32)
// 6       2     interval_s (u16 BE)
// 8       8     timestamp_ms (u64 BE)
// 16      8     node_id
// 24      32    sign_pk
// 56      32    dh_pk
// 88      N     name (UTF-8, name_len bytes)
// 88+N    64    signature
constexpr size_t kAnnounceFixedLen = 88;
constexpr size_t kAnnounceBodyLen = kAnnounceFixedLen + kSignatureLen;  // 152
constexpr size_t kAnnounceMaxNameLen = 32;

struct Announce {
    uint8_t version = kProtoVersion;
    NodeClass cls = NodeClass::Client;
    uint16_t caps = 0;
    char name[kAnnounceMaxNameLen + 1]{};
    uint8_t name_len = 0;
    uint16_t interval_s = 0;
    uint64_t timestamp_ms = 0;
    PublicIdentity ident{};

    void setName(const char* n, size_t len);
};

// Encodes the signed body without the trailing signature. Returns body length, or 0 on error.
size_t encodeAnnounceUnsigned(const Announce& a, uint8_t* out, size_t capacity);
// Appends the 64-byte signature; returns total body length.
size_t encodeAnnounceBody(const Announce& a, const uint8_t signature[kSignatureLen],
                          uint8_t* out, size_t capacity);
// Parses the binary body. On success `signature` points inside `body` and `out` is filled.
bool decodeAnnounceBody(const uint8_t* body, size_t len, Announce& out,
                        const uint8_t*& signature);
// Signature payload: "openmesh/v1/announce" || src(8) || seq(4 BE) || body_without_signature
size_t announceSignaturePayload(const NodeId& src, uint32_t seq, const uint8_t* body_unsigned,
                                size_t body_len, uint8_t* out, size_t capacity);
// Hops are derived from TTL because relays cannot modify signed bytes.
inline uint8_t announceHops(uint8_t received_ttl) {
    return received_ttl >= kAnnounceTtl ? 0 : (uint8_t)(kAnnounceTtl - received_ttl);
}

// --- Position (services.rs) -----------------------------------------------------------------
struct Position {
    int32_t lat_e7 = 0;
    int32_t lon_e7 = 0;
    bool has_alt = false;
    int16_t alt_m = 0;
    bool has_accuracy = false;
    uint8_t accuracy_m = 0;
    bool has_speed = false;
    uint16_t speed_cm_s = 0;
    bool has_heading = false;
    uint16_t heading_deg = 0;
    uint64_t timestamp_ms = 0;
};

// --- Telemetry (services.rs) ----------------------------------------------------------------
struct Telemetry {
    uint64_t timestamp_ms = 0;
    bool has_battery_mv = false;
    uint16_t battery_mv = 0;
    bool has_battery_pct = false;
    uint8_t battery_pct = 0;
    bool has_uptime_s = false;
    uint32_t uptime_s = 0;
    bool has_airtime_used_ms = false;
    uint32_t airtime_used_ms = 0;
    bool has_rx_frames = false;
    uint32_t rx_frames = 0;
    bool has_tx_frames = false;
    uint32_t tx_frames = 0;
    bool has_temperature_c = false;
    int8_t temperature_c = 0;
    bool has_solar_mv = false;
    uint16_t solar_mv = 0;
};

// --- AppMessage (services.rs) ---------------------------------------------------------------
struct TextRef {
    char data[192]{};
    size_t len = 0;
};

struct AppMessage {
    uint32_t id = 0;
    MessageKind kind = MessageKind::Text;
    uint64_t sent_at_ms = 0;
    bool has_text = false;
    TextRef text;
    bool has_position = false;
    Position position;
    bool has_telemetry = false;  // provisional extension key, see firmware/README.md
    Telemetry telemetry;
    uint32_t reply_to = 0;
    bool has_reply_to = false;
};

size_t encodeAppMessage(const AppMessage& m, uint8_t* out, size_t capacity);
bool decodeAppMessage(const uint8_t* cbor, size_t len, AppMessage& out);

// --- Ack (PROTOCOL.md §8) -------------------------------------------------------------------
struct Ack {
    AckKind kind = AckKind::Delivered;
    uint32_t msg_id = 0;
    bool has_res_id = false;
    uint32_t res_id = 0;
    bool has_next_expected = false;
    uint16_t next_expected = 0;
    bool has_bitmap = false;
    uint32_t bitmap = 0;
    uint64_t timestamp_ms = 0;
};

size_t encodeAck(const Ack& a, uint8_t* out, size_t capacity);
bool decodeAck(const uint8_t* cbor, size_t len, Ack& out);

// --- Resource manifest (PROTOCOL.md §9.1) ---------------------------------------------------
struct ResourceManifest {
    uint32_t res_id = 0;
    uint32_t msg_id = 0;
    ResourceKind kind = ResourceKind::Attachment;
    char mime[48]{};
    uint8_t mime_len = 0;
    uint32_t total_len = 0;
    uint16_t chunk_bytes = 0;
    uint16_t chunk_count = 0;
    uint8_t sha256[32]{};
};

bool decodeResourceManifest(const uint8_t* cbor, size_t len, ResourceManifest& out);
size_t encodeResourceManifest(const ResourceManifest& manifest, uint8_t* out, size_t capacity);

// --- Store-and-forward offers (PROTOCOL.md §10) --------------------------------------------
struct StoreOffer {
    uint32_t ids[16]{};
    uint8_t count = 0;
};

struct StoreWant {
    uint32_t ids[16]{};
    uint8_t count = 0;
};

// Clear-text CBOR bodies: relays that do not share a key with the addressee must be able to
// construct and parse them, so these are deliberately unsealed (spec note in README).
size_t encodeStoreOffer(const StoreOffer& offer, uint8_t* out, size_t capacity);
bool decodeStoreOffer(const uint8_t* cbor, size_t len, StoreOffer& out);
size_t encodeStoreWant(const StoreWant& want, uint8_t* out, size_t capacity);
bool decodeStoreWant(const uint8_t* cbor, size_t len, StoreWant& out);

// --- GroupMessage (PROTOCOL.md §7.3) --------------------------------------------------------
struct GroupMessageView {
    NodeId sender{};                        // bytes(8): sender node id
    const uint8_t* message_cbor = nullptr;  // span inside the decrypted plaintext
    size_t message_cbor_len = 0;
    const uint8_t* signature = nullptr;     // bytes(64)
    size_t signature_len = 0;
};

bool decodeGroupMessage(const uint8_t* cbor, size_t len, GroupMessageView& out);
// Encodes {sender: bytes(8), signature: bytes(64), message: AppMessage}.
size_t encodeGroupMessage(const NodeId& sender, const uint8_t signature[kSignatureLen],
                          const AppMessage& message, uint8_t* out, size_t capacity);

// Signature payload: "openmesh/v1/group-msg" || sender.node_id(8) || channel_id(8) || cbor(msg)
size_t groupSignaturePayload(const NodeId& sender, const NodeId& channel,
                             const uint8_t* message_cbor, size_t message_cbor_len, uint8_t* out,
                             size_t capacity);

// --- Resource blob (PROTOCOL.md §9.1) -------------------------------------------------------
// AAD = "openmesh/v1/resource" || res_id(4 BE) || msg_id(4 BE) || total_len(4 BE) ||
//       chunk_bytes(2 BE). The 20-byte domain makes this 34 bytes total (PROTOCOL.md §9.1
//       says "25 bytes", which assumes a 16-byte domain string — noted in the firmware README).
constexpr size_t kResourceAadLen = 20 + 4 + 4 + 4 + 2;
void resourceAad(uint32_t res_id, uint32_t msg_id, uint32_t total_len, uint16_t chunk_bytes,
                 uint8_t out[kResourceAadLen]);

}  // namespace om

#endif  // OPENMESH_OM_MESSAGES_H
