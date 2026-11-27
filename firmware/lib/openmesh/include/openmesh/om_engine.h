// SPDX-License-Identifier: AGPL-3.0-or-later
// OMP/1 node engine: announces, managed flooding, dedup, store-and-forward, acks, telemetry.
//
// Portable C++17; all IO is injected (RNG callback, storage backend, monotonic clock passed to
// every entry point). The Arduino/RadioLib glue lives in firmware/src/, not here. This mirrors
// the Rust engine's externally observable behaviour (crates/openmesh-core/src/engine.rs).
#ifndef OPENMESH_OM_ENGINE_H
#define OPENMESH_OM_ENGINE_H

#include <cstddef>
#include <cstdint>

#include "openmesh/om_companion.h"
#include "openmesh/om_dedup.h"
#include "openmesh/om_gps.h"
#include "openmesh/om_identity.h"
#include "openmesh/om_messages.h"
#include "openmesh/om_power.h"
#include "openmesh/om_region.h"
#include "openmesh/om_store.h"
#include "openmesh/om_types.h"

#ifndef OM_PEER_CAPACITY
#define OM_PEER_CAPACITY 64
#endif
#ifndef OM_CHANNEL_CAPACITY
#define OM_CHANNEL_CAPACITY 4
#endif
#ifndef OM_EVENT_QUEUE
#define OM_EVENT_QUEUE 6
#endif
#ifndef OM_OUTBOX_CAPACITY
#define OM_OUTBOX_CAPACITY 6
#endif
#ifndef OM_APP_DEDUP_CAPACITY
#define OM_APP_DEDUP_CAPACITY 32
#endif

namespace om {

// Storage backend injected by the board HAL (LittleFS on ESP32, InternalFileSystem on nRF52,
// in-memory on host).
class StorageBackend {
public:
    virtual ~StorageBackend() = default;
    virtual bool load(const char* name, uint8_t* out, size_t capacity, size_t& len) = 0;
    virtual bool save(const char* name, const uint8_t* data, size_t len) = 0;
    virtual bool remove(const char* name) = 0;
};

struct ChannelConfig {
    char name[16]{};
    uint8_t name_len = 0;
    uint8_t psk[32]{};
    NodeId id{};
    bool used = false;
};

struct EngineConfig {
    NodeClass cls = NodeClass::Client;
    char name[33]{};
    uint8_t name_len = 0;
    uint16_t caps = 0;
    uint8_t default_ttl = kDefaultTtl;
    // Effective TX power in dBm, already clamped to min(legal region max, board PA cap).
    int8_t tx_power_dbm = 14;
    // Board PA ceiling (from firmware/boards/<board>.cpp). Absolute under all overrides.
    int8_t hardware_max_power_dbm = 22;
    // Regulatory override (docs/design/regulatory-and-regions.md §2). Defaults are the legal
    // maxima; these fields are only set through the companion ConfigSet API / tests.
    int16_t duty_override_pct = -1;      // -1 = legal region duty, 0..100 override
    int8_t power_override_dbm = kNoTxPowerOverrideDbm;  // -9..30 request; sentinel = legal max
    uint32_t frequency_override_khz = 0; // 0 = region centre frequency
    const LoraProfile* profile = &kMediumFast;
    const Region* region = &kRegionEu868;
    bool relay_role = true;
    bool store_role = false;
    bool accept_channels = false;
    bool position_sharing = true;
    uint32_t announce_interval_ms = (uint32_t)kAnnounceIntervalClientMs;
    uint32_t telemetry_interval_ms = 900000;  // 15 min for infrastructure nodes
};

struct PeerInfo {
    bool used = false;
    PublicIdentity ident{};
    char name[33]{};
    uint8_t name_len = 0;
    NodeClass cls = NodeClass::Client;
    uint16_t caps = 0;
    uint8_t hops = 0;
    uint64_t last_heard_ms = 0;
    int16_t rssi = 0;
    int16_t snr = 0;
};

enum class Link : uint8_t { Lora = 0, Usb = 1, Ble = 2 };

struct OutFrame {
    Frame frame;
    bool broadcast = true;
    NodeId peer{};  // unicast peer when broadcast == false
    // Best-effort managed-flooding copy. App-forwarded and engine-originated frames are not
    // "relay" copies and take priority over relay work when the queue is under pressure.
    bool relay = false;
    // Frame supplied by the companion app (transmitted unchanged). Used for TX-path diagnostics.
    bool app = false;
};

class Outbox {
public:
    static constexpr size_t kCapacity = OM_OUTBOX_CAPACITY;
    bool push(const Frame& frame, bool broadcast = true, const NodeId& peer = NodeId::zero(),
              bool relay = false, bool app = false);
    size_t size() const { return count_; }
    const OutFrame& at(size_t i) const { return items_[i]; }
    void popFront();
    void removeAt(size_t index);
    void clear() { count_ = 0; }
    bool full() const { return count_ >= kCapacity; }

private:
    OutFrame items_[kCapacity]{};
    size_t count_ = 0;
};

enum class EventType : uint8_t {
    RxFrame = 0,
    AppMessage,
    Announce,
    StoreDelivered,
    LowBattery,
    GpsFix,
    Error,
};

struct EngineEvent {
    EventType type = EventType::RxFrame;
    Link link = Link::Lora;
    int16_t rssi = 0;
    int16_t snr = 0;
    uint16_t frame_len = 0;
    uint8_t frame[kMtu]{};
    NodeId peer{};
    AppMessage message;
    Announce announce;
    BatteryReading battery;
    char detail[40]{};
};

class EventQueue {
public:
    static constexpr size_t kCapacity = OM_EVENT_QUEUE;
    bool push(const EngineEvent& e);
    size_t size() const { return count_; }
    const EngineEvent& at(size_t i) const { return (items_[(head_ + i) % kCapacity]); }
    void popFront();
    void clear() { count_ = 0; head_ = 0; }

private:
    EngineEvent items_[kCapacity]{};
    size_t head_ = 0;
    size_t count_ = 0;
};

struct EngineStats {
    uint32_t rx_frames = 0;
    uint32_t tx_frames = 0;
    uint32_t relayed = 0;
    uint32_t dropped = 0;
    // RxFrame engine events silently discarded because the event queue was full (burst overload).
    uint32_t events_dropped = 0;
    uint32_t malformed = 0;
    uint32_t duplicates = 0;
    uint32_t decrypt_errors = 0;
    uint32_t announce_rx = 0;
    uint32_t announce_tx = 0;
    uint32_t acks_sent = 0;
    uint32_t acks_rx = 0;
    uint32_t store_retained = 0;
    uint32_t store_delivered = 0;
    uint32_t store_expired = 0;
    uint32_t airtime_ms = 0;
    uint32_t budget_drops = 0;
    // Companion/app-forwarded traffic (frames supplied by the app, transmitted unchanged).
    uint32_t app_tx_frames = 0;
    // Subset of app_tx_frames that carries an acknowledgement (direct Ack or PAYLOAD_IS_ACK data,
    // e.g. resource-window ACKs). TX outcomes are tracked by the firmware main loop.
    uint32_t app_ack_tx_frames = 0;
    uint32_t app_tx_airtime_ms = 0;  // estimated airtime of app frames queued for transmission
    // Relay copies not queued (deferred while companion traffic is active / queue is backed up)
    // or evicted from a full outbox to make room for an app-forwarded frame.
    uint32_t relay_deferred = 0;
    uint32_t relay_evicted = 0;
};

class Engine {
public:
    Engine() = default;

    // Copies the identity (secrets stay in RAM), sets configuration and RNG. `storage` may be
    // null for RAM-only operation. Call `start()` once after configuration.
    bool begin(const Identity& identity, const EngineConfig& config, RngFn rng, void* rng_ctx,
               StorageBackend* storage);
    void start(uint64_t now_ms);
    void stop();

    // --- Ingest -----------------------------------------------------------------------------
    // A validated frame received on a link. Handles dedup, delivery, relaying and storing.
    void onFrame(const Frame& frame, Link link, int16_t rssi, int16_t snr, uint64_t now_ms);
    // Raw bytes from a transport (LoRa packet or companion Tx). Returns false when malformed.
    bool onBytes(const uint8_t* data, size_t len, Link link, int16_t rssi, int16_t snr,
                 uint64_t now_ms);

    // --- Periodic work ---------------------------------------------------------------------
    // Announce scheduler, telemetry heartbeat, store offers and expiry. Call every loop.
    void tick(uint64_t now_ms);
    // Wall-clock (Unix ms) for announce/telemetry timestamps. The HAL provides it from the
    // RTC/GPS; monotonic time is used for all timeouts. Defaults to boot-relative time.
    void setWallTime(uint64_t unix_ms, uint64_t now_ms);
    uint64_t wallNow(uint64_t now_ms) const;
    // Battery/solar reading from the HAL; updates the power policy.
    void setBattery(const BatteryReading& reading, uint64_t now_ms);
    // GPS fix from the HAL; may originate a position message when due.
    void setGpsFix(const GpsFix& fix, uint64_t now_ms);
    // Notes that the companion app link is currently active (frames were just forwarded to the app
    // over BLE/USB, or the app supplied a frame). While active, relay copies are best-effort: they
    // are deferred during resource (media) transfers or when the TX queue is backed up, so
    // app-forwarded frames and the phone's window ACKs are not starved. Relaying resumes when the
    // companion has been idle for the activity window.
    void noteCompanionActivity(uint64_t now_ms);
    // Sticky "a companion app is attached right now" flag, refreshed by the firmware loop from the
    // transport state. Unlike noteCompanionActivity() this does not expire between radio frames:
    // the bridge paces resource batches ~12 s apart, which is longer than a traffic-recency window,
    // so relay deferral must key off the attached link, not off the last frame.
    void setCompanionLinkActive(bool active);
    // Persist the store queue when dirty (rate-limited by the caller). Returns true when saved.
    bool saveStore(uint64_t now_ms);
    // Regulatory/config state persistence (region, profile, overrides, mesh settings). Called
    // automatically on accepted ConfigSet changes when a StorageBackend is attached; loaded by
    // begin() so overrides survive a reboot.
    bool persistConfig();
    void loadPersistedConfig();
    void loadStore(uint64_t now_ms);
    // Board storage adapters load the serialised store file and call this (avoids a 13 KiB
    // stack buffer in the engine).
    void loadStoreBuffer(const uint8_t* data, size_t len, uint64_t now_ms);

    // --- Origination -----------------------------------------------------------------------
    bool buildAnnounce(uint64_t now_ms, Frame& out);
    bool buildPosition(const Position& position, uint32_t msg_id, int8_t channel_index,
                       uint64_t now_ms, Frame& out);
    // Sends an arbitrary AppMessage as a signed GroupMessage on a channel (used by positions,
    // telemetry and tests). Fails when the sealed frame would exceed the MTU.
    bool buildChannelAppMessage(const AppMessage& message, int channel_index, uint64_t now_ms,
                                Frame& out);
    bool buildTelemetry(const Telemetry& telemetry, uint64_t now_ms, Frame& out);
    // Builds and queues a delivery ACK for `to` (requires the peer's announce key).
    bool buildAck(const NodeId& to, const Ack& ack, uint64_t now_ms, Frame& out);
    bool buildStoreOffer(const NodeId& to, uint64_t now_ms, Frame& out);
    bool buildStoreWant(const NodeId& to, const StoreWant& want, uint64_t now_ms, Frame& out);
    // Frame supplied by a companion app: transmitted unchanged (TTL untouched, PROTOCOL §13).
    bool originateFromApp(const uint8_t* frame_bytes, size_t len, uint64_t now_ms);

    // --- Configuration / introspection -----------------------------------------------------
    bool applyConfig(const char* key, const char* value);
    void setChannel(const char* name, const uint8_t psk[32]);
    void clearChannels();
    const EngineConfig& config() const { return config_; }
    EngineConfig& mutableConfig() { return config_; }
    const Identity& identity() const { return identity_; }
    EngineStats& mutableStats() { return stats_; }
    const EngineStats& stats() const { return stats_; }
    Outbox& outbox() { return outbox_; }
    EventQueue& events() { return events_; }
    const StoreQueue& store() const { return store_; }
    StoreQueue& mutableStore() { return store_; }
    // --- Regulatory policy ------------------------------------------------------------------
    bool overrideActive() const { return override_active_; }
    const char* overrideWarning() const {
        return override_active_ ? kRegulatoryOverrideWarning : nullptr;
    }
    int16_t effectiveDutyPctValue() const {
        return effectiveDutyPct(*config_.region, config_.duty_override_pct);
    }
    int8_t effectivePowerDbmValue() const { return config_.tx_power_dbm; }
    uint32_t effectiveFrequencyKhzValue() const {
        return effectiveFrequencyKhz(*config_.region, config_.frequency_override_khz);
    }

    size_t peerCount() const;
    const PeerInfo* peerAt(size_t index) const;
    const PeerInfo* findPeer(const NodeId& id) const;
    // Fills HelloAck / Config / Stats companion messages.
    void fillHelloAck(CompanionMsg& msg) const;
    void fillConfig(CompanionMsg& msg) const;
    void fillStats(CompanionMsg& msg) const;

private:
    // Frame processing stages.
    void handleAnnounce(const Frame& frame, Link link, int16_t rssi, int16_t snr,
                        uint64_t now_ms);
    void deliverDirect(const Frame& frame, Link link, int16_t rssi, int16_t snr,
                       uint64_t now_ms);
    void deliverChannel(const Frame& frame, Link link, int16_t rssi, int16_t snr,
                        uint64_t now_ms);
    void handleStoreOffer(const Frame& frame, uint64_t now_ms);
    void handleStoreWant(const Frame& frame, uint64_t now_ms);
    void maybeRelay(const Frame& frame, uint64_t now_ms);
    void maybeStore(const Frame& frame, uint64_t now_ms);
    void maybeOfferTo(const NodeId& peer, bool fresh_announce, uint64_t now_ms);
    void learnPeer(const NodeId& id, const PublicIdentity* ident, const char* name,
                   size_t name_len, NodeClass cls, uint16_t caps, uint8_t hops, int16_t rssi,
                   int16_t snr, uint64_t now_ms);
    bool queueTx(const Frame& frame, bool broadcast, const NodeId& peer, uint64_t now_ms,
                 bool is_relay = false);
    bool companionActive(uint64_t now_ms) const { return now_ms < companion_active_until_ms_; }
    bool resourceActive(uint64_t now_ms) const { return now_ms < resource_active_until_ms_; }
    bool sealAndQueue(Frame& frame, const uint8_t* plaintext, size_t plaintext_len,
                      bool include_identity, const PublicIdentity* direct_peer,
                      int channel_index, uint8_t payload_flags, uint32_t msg_ref,
                      uint64_t now_ms);
    bool transmitBudgetOk(size_t frame_len, uint64_t now_ms);
    void emitFrameEvent(const Frame& frame, Link link, int16_t rssi, int16_t snr);
    void scheduleNextAnnounce(uint64_t now_ms, bool announce_now);
    void pushError(const char* detail);
    int channelIndex(const NodeId& id) const;
    bool channelIsValid(int index) const;
    uint32_t jitter(uint32_t interval_ms);
    uint32_t nextRandomId();
    // Recomputes budget configuration, effective power clamp and the override-active flag after
    // a config change (region, profile, overrides, board cap).
    void recomputeRegulatoryPolicy();
    const char* configFileName() const { return "om_cfg.bin"; }
    // Direct-message fallback: positions/telemetry to a known infrastructure peer when a
    // channel GroupMessage would not fit in one frame.
    bool sendAppDirect(const NodeId& peer_id, const AppMessage& message, uint8_t payload_flags,
                       uint64_t now_ms, Frame& out);

    Identity identity_{};
    EngineConfig config_{};
    // Activity windows for companion-aware relay prioritisation (see noteCompanionActivity).
    uint64_t companion_active_until_ms_ = 0;
    uint64_t resource_active_until_ms_ = 0;
    bool companion_link_active_ = false;
    RngFn rng_ = nullptr;
    void* rng_ctx_ = nullptr;
    StorageBackend* storage_ = nullptr;

    DedupCache dedup_{};
    StoreQueue store_{};
    ChannelConfig channels_[OM_CHANNEL_CAPACITY]{};
    PeerInfo peers_[OM_PEER_CAPACITY]{};
    size_t peer_count_ = 0;

    struct AppDedupEntry {
        NodeId src{};
        uint32_t msg_ref = 0;
    };
    AppDedupEntry app_dedup_[OM_APP_DEDUP_CAPACITY]{};
    size_t app_dedup_head_ = 0;

    // Per-peer store-offer pacing.
    struct OfferState {
        NodeId peer{};
        uint64_t last_offer_ms = 0;
    };
    OfferState offers_[OM_PEER_CAPACITY]{};
    size_t offer_count_ = 0;

    AirtimeBudget budget_{};
    OriginateLimiter originate_{};
    PowerPolicy power_{};
    BatteryReading battery_{};

    Outbox outbox_{};
    EventQueue events_{};
    EngineStats stats_{};

    uint32_t seq_ = 0;
    bool override_active_ = false;
    bool override_warned_ = false;
    uint64_t boot_ms_ = 0;
    uint64_t wall_base_unix_ms_ = 0;
    uint64_t wall_base_mono_ms_ = 0;
    bool wall_valid_ = false;
    uint64_t next_announce_ms_ = 0;
    uint64_t next_telemetry_ms_ = 0;
    uint64_t last_store_save_ms_ = 0;
    uint64_t last_offer_scan_ms_ = 0;
    uint64_t last_tick_ms_ = 0;
    uint64_t last_gps_position_ms_ = 0;
    bool started_ = false;
    uint8_t save_scratch_[StoreQueue::kSerializedSize]{};
};

}  // namespace om

#endif  // OPENMESH_OM_ENGINE_H
