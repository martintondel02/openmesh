// SPDX-License-Identifier: AGPL-3.0-or-later
// OMP/1 node engine implementation.
#include "openmesh/om_engine.h"

#include <cstdlib>
#include <cstring>

#include "openmesh/om_sha256.h"

#ifndef OM_FW_VERSION
#define OM_FW_VERSION "0.1.0"
#endif

namespace om {

namespace {

constexpr char kSeqFileName[] = "om_seq.bin";
constexpr char kStoreFileName[] = "om_store.bin";

void copyName(char* dst, size_t cap, const char* src, size_t src_len) {
    if (src_len >= cap) src_len = cap - 1;
    if (src_len > 0) std::memcpy(dst, src, src_len);
    dst[src_len] = '\0';
}

uint64_t wallOrBoot(uint64_t wall_base_unix, uint64_t wall_base_mono, bool valid, uint64_t now) {
    if (!valid) return now;
    if (now < wall_base_mono) return wall_base_unix;
    return wall_base_unix + (now - wall_base_mono);
}

void u64ToText(uint64_t v, char* out, size_t cap) {
    char tmp[24];
    size_t pos = sizeof(tmp);
    do {
        tmp[--pos] = (char)('0' + (v % 10));
        v /= 10;
    } while (v > 0 && pos > 0);
    size_t n = sizeof(tmp) - pos;
    if (n >= cap) n = cap - 1;
    std::memcpy(out, tmp + pos, n);
    out[n] = '\0';
}

}  // namespace

// --- Outbox / EventQueue --------------------------------------------------------------------
bool Outbox::push(const Frame& frame, bool broadcast, const NodeId& peer) {
    if (count_ >= kCapacity) return false;
    items_[count_].frame = frame;
    items_[count_].broadcast = broadcast;
    items_[count_].peer = peer;
    ++count_;
    return true;
}

void Outbox::popFront() {
    if (count_ == 0) return;
    for (size_t i = 1; i < count_; ++i) items_[i - 1] = items_[i];
    --count_;
}

bool EventQueue::push(const EngineEvent& e) {
    if (count_ >= kCapacity) return false;
    items_[(head_ + count_) % kCapacity] = e;
    ++count_;
    return true;
}

void EventQueue::popFront() {
    if (count_ == 0) return;
    head_ = (head_ + 1) % kCapacity;
    --count_;
}

// --- Engine ---------------------------------------------------------------------------------
bool Engine::begin(const Identity& identity, const EngineConfig& config, RngFn rng,
                   void* rng_ctx, StorageBackend* storage) {
    identity_ = identity;
    config_ = config;
    rng_ = rng;
    rng_ctx_ = rng_ctx;
    storage_ = storage;
    if (!identity_.valid) return false;
    if (config_.name_len >= sizeof(config_.name)) config_.name_len = sizeof(config_.name) - 1;
    config_.name[config_.name_len] = '\0';

    dedup_.clear();
    store_.clear();
    outbox_.clear();
    events_.clear();
    stats_ = EngineStats{};
    std::memset(peers_, 0, sizeof(peers_));
    peer_count_ = 0;
    std::memset(offers_, 0, sizeof(offers_));
    offer_count_ = 0;
    std::memset(app_dedup_, 0, sizeof(app_dedup_));
    app_dedup_head_ = 0;
    seq_ = 0;
    wall_valid_ = false;
    power_ = PowerPolicy{};
    battery_ = BatteryReading{};

    if (config_.relay_role) config_.caps |= kCapRelay;
    if (config_.store_role) config_.caps |= kCapStore;
    recomputeRegulatoryPolicy();
    loadPersistedConfig();
    loadStore(0);
    return true;
}

void Engine::start(uint64_t now_ms) {
    boot_ms_ = now_ms;
    started_ = true;
    originate_.reset(now_ms);
    budget_.reset(now_ms);
    scheduleNextAnnounce(now_ms, true);
    next_telemetry_ms_ = now_ms + config_.telemetry_interval_ms;
    last_offer_scan_ms_ = now_ms;
}

void Engine::stop() { started_ = false; }

uint32_t Engine::jitter(uint32_t interval_ms) {
    if (rng_ == nullptr) return 0;
    const uint32_t span = interval_ms / 10;  // +-10 %
    if (span == 0) return 0;
    uint8_t bytes[4];
    rng_(rng_ctx_, bytes, sizeof(bytes));
    const uint32_t r = readU32Be(bytes) % (2 * span + 1);
    return r - span;
}

void Engine::scheduleNextAnnounce(uint64_t now_ms, bool announce_now) {
    if (announce_now) {
        next_announce_ms_ = now_ms;
        return;
    }
    const uint32_t base = (uint32_t)config_.announce_interval_ms;
    int64_t delta = (int64_t)base + (int64_t)jitter(base);
    if (delta < (int64_t)kAnnounceMinIntervalMs) delta = (int64_t)kAnnounceMinIntervalMs;
    next_announce_ms_ = now_ms + (uint64_t)delta;
}

bool Engine::transmitBudgetOk(size_t frame_len, uint64_t now_ms) {
    const uint32_t airtime = loraAirtimeMs(*config_.profile, frame_len);
    if (!budget_.tryConsume(airtime, now_ms)) {
        ++stats_.budget_drops;
        return false;
    }
    stats_.airtime_ms += airtime;
    return true;
}

bool Engine::queueTx(const Frame& frame, bool broadcast, const NodeId& peer, uint64_t now_ms) {
    if (!started_ && frame.src == identity_.nodeId()) {
        // Origination before start() is a bug; drop defensively.
        ++stats_.dropped;
        return false;
    }
    if (power_.critical && frame.type != FrameType::Announce) {
        // Critical battery: only announces go out, everything else is deferred/dropped.
        ++stats_.dropped;
        return false;
    }
    uint8_t encoded[kMtu];
    const size_t len = frame.encode(encoded, sizeof(encoded));
    if (len == 0) {
        ++stats_.dropped;
        return false;
    }
    if (!transmitBudgetOk(len, now_ms)) return false;
    if (!outbox_.push(frame, broadcast, peer)) {
        ++stats_.dropped;
        return false;
    }
    // Mark as seen so an echo relayed back by another node is not processed again.
    dedup_.checkAndInsert(frame.src, frame.seq, now_ms);
    ++stats_.tx_frames;
    return true;
}

void Engine::pushError(const char* detail) {
    EngineEvent e;
    e.type = EventType::Error;
    copyName(e.detail, sizeof(e.detail), detail, std::strlen(detail));
    events_.push(e);
}

void Engine::emitFrameEvent(const Frame& frame, Link link, int16_t rssi, int16_t snr) {
    EngineEvent e;
    e.type = EventType::RxFrame;
    e.link = link;
    e.rssi = rssi;
    e.snr = snr;
    e.peer = frame.src;
    const size_t n = frame.encode(e.frame, sizeof(e.frame));
    e.frame_len = (uint16_t)n;
    events_.push(e);
}

bool Engine::onBytes(const uint8_t* data, size_t len, Link link, int16_t rssi, int16_t snr,
                     uint64_t now_ms) {
    Frame frame;
    const DecodeResult r = decodeFrame(data, len, frame);
    if (!r.ok()) {
        if (r.shouldIgnore()) {
            // Reserved frame type: ignore silently (PROTOCOL.md §16).
            return true;
        }
        ++stats_.malformed;
        return false;
    }
    onFrame(frame, link, rssi, snr, now_ms);
    return true;
}

void Engine::onFrame(const Frame& frame, Link link, int16_t rssi, int16_t snr, uint64_t now_ms) {
    ++stats_.rx_frames;
    emitFrameEvent(frame, link, rssi, snr);

    // PROTOCOL.md §11: dedup first, then learn and dispatch.
    if (!dedup_.checkAndInsert(frame.src, frame.seq, now_ms)) {
        ++stats_.duplicates;
        return;
    }
    const bool for_me = (frame.dst == identity_.nodeId());
    const bool for_my_channel = (channelIndex(frame.dst) >= 0);

    // A TTL of zero means the frame cannot be forwarded; it is only useful to us.
    if (frame.ttl == 0 && !for_me && !for_my_channel && !frame.isBroadcast()) {
        // Nothing to do, but it was a valid frame and has been reported to the companion.
        return;
    }

    learnPeer(frame.src, nullptr, nullptr, 0, NodeClass::Client, 0, 0, rssi, snr, now_ms);

    switch (frame.type) {
        case FrameType::Announce:
            handleAnnounce(frame, link, rssi, snr, now_ms);
            break;
        case FrameType::Data:
            if (for_me) deliverDirect(frame, link, rssi, snr, now_ms);
            else if (for_my_channel) deliverChannel(frame, link, rssi, snr, now_ms);
            if (isStorable(frame.type) && !for_me) maybeStore(frame, now_ms);
            maybeRelay(frame, now_ms);
            break;
        case FrameType::Ack:
            if (for_me) deliverDirect(frame, link, rssi, snr, now_ms);
            maybeRelay(frame, now_ms);
            break;
        case FrameType::Resource:
            if (isStorable(frame.type) && !for_me && !for_my_channel) maybeStore(frame, now_ms);
            maybeRelay(frame, now_ms);
            break;
        case FrameType::StoreOffer:
            if (for_me) handleStoreOffer(frame, now_ms);
            maybeRelay(frame, now_ms);
            break;
        case FrameType::StoreWant:
            if (for_me) handleStoreWant(frame, now_ms);
            maybeRelay(frame, now_ms);
            break;
        case FrameType::Probe:
        case FrameType::ProbeResponse:
        case FrameType::RouteRequest:
        case FrameType::RouteResponse:
            maybeRelay(frame, now_ms);
            break;
        case FrameType::Admin:
            // Reserved for future signed admin frames; never relayed by v1.
            break;
        default:
            break;
    }
}

// --- Peer bookkeeping -----------------------------------------------------------------------
void Engine::learnPeer(const NodeId& id, const PublicIdentity* ident, const char* name,
                       size_t name_len, NodeClass cls, uint16_t caps, uint8_t hops,
                       int16_t rssi, int16_t snr, uint64_t now_ms) {
    PeerInfo* peer = nullptr;
    for (size_t i = 0; i < peer_count_; ++i) {
        if (peers_[i].used && peers_[i].ident.node_id == id) {
            peer = &peers_[i];
            break;
        }
    }
    if (peer == nullptr) {
        if (peer_count_ < OM_PEER_CAPACITY) {
            peer = &peers_[peer_count_++];
        } else {
            // Evict the least recently heard peer.
            size_t oldest = 0;
            for (size_t i = 1; i < OM_PEER_CAPACITY; ++i) {
                if (peers_[i].last_heard_ms < peers_[oldest].last_heard_ms) oldest = i;
            }
            peer = &peers_[oldest];
        }
        *peer = PeerInfo{};
        peer->used = true;
        peer->ident.node_id = id;
    }
    if (ident != nullptr) peer->ident = *ident;
    if (name != nullptr) {
        copyName(peer->name, sizeof(peer->name), name, name_len);
        peer->name_len = (uint8_t)std::strlen(peer->name);
        peer->cls = cls;
        peer->caps = caps;
    }
    peer->hops = hops;
    peer->last_heard_ms = now_ms;
    peer->rssi = rssi;
    peer->snr = snr;
}

size_t Engine::peerCount() const {
    size_t n = 0;
    for (size_t i = 0; i < peer_count_; ++i)
        if (peers_[i].used) ++n;
    return n;
}

const PeerInfo* Engine::peerAt(size_t index) const {
    size_t n = 0;
    for (size_t i = 0; i < peer_count_; ++i) {
        if (!peers_[i].used) continue;
        if (n == index) return &peers_[i];
        ++n;
    }
    return nullptr;
}

const PeerInfo* Engine::findPeer(const NodeId& id) const {
    for (size_t i = 0; i < peer_count_; ++i) {
        if (peers_[i].used && peers_[i].ident.node_id == id) return &peers_[i];
    }
    return nullptr;
}

// --- Announces ------------------------------------------------------------------------------
void Engine::handleAnnounce(const Frame& frame, Link link, int16_t rssi, int16_t snr,
                            uint64_t now_ms) {
    (void)link;
    Announce announce;
    if (!verifyAnnounce(frame, announce)) {
        // Bad signature / inconsistent identity: drop and do not relay (PROTOCOL.md §6).
        ++stats_.malformed;
        return;
    }
    ++stats_.announce_rx;
    const uint8_t hops = announceHops(frame.ttl);
    learnPeer(frame.src, &announce.ident, announce.name, announce.name_len, announce.cls,
              announce.caps, hops, rssi, snr, now_ms);
    EngineEvent e;
    e.type = EventType::Announce;
    e.peer = frame.src;
    e.announce = announce;
    events_.push(e);
    if (config_.store_role) maybeOfferTo(frame.src, true, now_ms);
    maybeRelay(frame, now_ms);
}

// --- Delivery -------------------------------------------------------------------------------
void Engine::deliverDirect(const Frame& frame, Link link, int16_t rssi, int16_t snr,
                           uint64_t now_ms) {
    if (!frame.hasFlag(kFlagEncrypted)) {
        // Clear bodies are only valid for announce/probe (PROTOCOL.md §4.1).
        ++stats_.malformed;
        return;
    }
    SealedBodyView body;
    if (!SealedBodyView::parse(frame.body, frame.body_len, body)) {
        ++stats_.malformed;
        return;
    }
    PublicIdentity peer_ident;
    bool have_peer = false;
    if (body.identity != nullptr) {
        if (!PublicIdentity::fromBytes(body.identity, kPublicIdentityLen, peer_ident)) {
            ++stats_.malformed;
            return;
        }
        if (!peer_ident.isSelfConsistent() || !(peer_ident.node_id == frame.src)) {
            ++stats_.malformed;
            return;
        }
        have_peer = true;
        learnPeer(frame.src, &peer_ident, nullptr, 0, NodeClass::Client, 0, 0, rssi, snr, now_ms);
    } else {
        const PeerInfo* info = findPeer(frame.src);
        if (info != nullptr && info->ident.isSelfConsistent()) {
            peer_ident = info->ident;
            have_peer = true;
        }
    }
    if (!have_peer) {
        // Unknown sender and no identity in the body: cannot decrypt (PROTOCOL.md §7.1).
        ++stats_.decrypt_errors;
        return;
    }
    uint8_t aad[kHeaderLen];
    frame.aad(aad);
    uint8_t plaintext[kMaxBody];
    size_t plaintext_len = 0;
    if (!identity_.openDirectBody(peer_ident, body, aad, plaintext, sizeof(plaintext),
                                  plaintext_len)) {
        ++stats_.decrypt_errors;
        return;
    }

    const bool is_ack = (body.payload_flags & kPayloadFlagIsAck) != 0;
    if (frame.type == FrameType::Ack || is_ack) {
        Ack ack;
        if (!decodeAck(plaintext, plaintext_len, ack)) {
            ++stats_.malformed;
            return;
        }
        ++stats_.acks_rx;
        (void)ack;
        maybeOfferTo(frame.src, false, now_ms);
        return;
    }

    if (body.payload_flags & kPayloadFlagIsManifest) {
        ResourceManifest manifest;
        if (!decodeResourceManifest(plaintext, plaintext_len, manifest)) {
            ++stats_.malformed;
            return;
        }
        // Resource payloads are reassembled by the app; the radio forwards the manifest as an
        // Rx event (the companion link always carries the raw frame too).
        maybeOfferTo(frame.src, false, now_ms);
        return;
    }

    AppMessage message;
    if (!decodeAppMessage(plaintext, plaintext_len, message)) {
        ++stats_.malformed;
        return;
    }
    // Application-layer replay suppression by (src, msg_ref) (PROTOCOL.md §7.1).
    bool first_delivery = true;
    for (size_t i = 0; i < OM_APP_DEDUP_CAPACITY; ++i) {
        if (app_dedup_[i].msg_ref == body.msg_ref && app_dedup_[i].src == frame.src) {
            first_delivery = false;
            break;
        }
    }
    app_dedup_[app_dedup_head_].src = frame.src;
    app_dedup_[app_dedup_head_].msg_ref = body.msg_ref;
    app_dedup_head_ = (app_dedup_head_ + 1) % OM_APP_DEDUP_CAPACITY;
    if (first_delivery) {
        EngineEvent e;
        e.type = EventType::AppMessage;
        e.link = link;
        e.rssi = rssi;
        e.snr = snr;
        e.peer = frame.src;
        e.message = message;
        events_.push(e);
    }
    if (frame.hasFlag(kFlagAckRequest)) {
        Ack ack;
        ack.kind = AckKind::Delivered;
        ack.msg_id = message.id;
        ack.timestamp_ms = wallNow(now_ms);
        Frame ack_frame;
        buildAck(frame.src, ack, now_ms, ack_frame);
    }
    maybeOfferTo(frame.src, false, now_ms);
}

void Engine::deliverChannel(const Frame& frame, Link link, int16_t rssi, int16_t snr,
                            uint64_t now_ms) {
    (void)link;
    (void)rssi;
    (void)snr;
    const int idx = channelIndex(frame.dst);
    if (idx < 0 || !channelIsValid(idx)) return;
    if (!frame.hasFlag(kFlagEncrypted)) {
        ++stats_.malformed;
        return;
    }
    SealedBodyView body;
    if (!SealedBodyView::parse(frame.body, frame.body_len, body)) {
        ++stats_.malformed;
        return;
    }
    ChannelKey channel;
    if (!ChannelKey::derive(channels_[idx].psk, channels_[idx].name, channels_[idx].name_len,
                            channel)) {
        return;
    }
    uint8_t aad[kHeaderLen];
    frame.aad(aad);
    uint8_t plaintext[kMaxBody];
    size_t plaintext_len = 0;
    if (!channel.openBody(body, aad, plaintext, sizeof(plaintext), plaintext_len)) {
        ++stats_.decrypt_errors;
        return;
    }
    if (body.payload_flags & kPayloadFlagIsAck) return;  // ACKs are direct-only
    GroupMessageView group;
    if (!decodeGroupMessage(plaintext, plaintext_len, group)) {
        ++stats_.malformed;
        return;
    }
    // Resolve the sender's authentication key from the peer book or the body identity.
    const PeerInfo* peer = findPeer(group.sender);
    PublicIdentity sender_ident;
    bool have_sender = false;
    if (peer != nullptr && peer->ident.isSelfConsistent()) {
        sender_ident = peer->ident;
        have_sender = true;
    } else if (body.identity != nullptr &&
               PublicIdentity::fromBytes(body.identity, kPublicIdentityLen, sender_ident) &&
               sender_ident.isSelfConsistent() && (sender_ident.node_id == group.sender)) {
        have_sender = true;
        learnPeer(group.sender, &sender_ident, nullptr, 0, NodeClass::Client, 0, 0, 0, 0, now_ms);
    }
    if (!have_sender) {
        ++stats_.decrypt_errors;  // unauthenticated group message: drop (PROTOCOL.md §7.3)
        return;
    }
    if (!verifyGroupMessage(group.signature, group.sender, frame.dst, group.message_cbor,
                            group.message_cbor_len, sender_ident.sign_pk)) {
        ++stats_.malformed;
        return;
    }
    AppMessage message;
    if (!decodeAppMessage(group.message_cbor, group.message_cbor_len, message)) {
        ++stats_.malformed;
        return;
    }
    EngineEvent e;
    e.type = EventType::AppMessage;
    e.link = Link::Lora;
    e.peer = group.sender;
    e.message = message;
    events_.push(e);
    maybeOfferTo(group.sender, false, now_ms);
}

// --- Flooding / store -----------------------------------------------------------------------
void Engine::maybeRelay(const Frame& frame, uint64_t now_ms) {
    if (!config_.relay_role) return;
    if (frame.ttl == 0) return;
    if (!isFloodable(frame.type)) return;
    Frame relay;
    if (!frame.relayCopy(relay)) return;
    if (queueTx(relay, true, NodeId::zero(), now_ms)) ++stats_.relayed;
}

void Engine::maybeStore(const Frame& frame, uint64_t now_ms) {
    if (!config_.store_role) return;
    if (!isStorable(frame.type)) return;
    if (!isUnicastAddress(frame.dst)) return;  // channel traffic is never stored (§10)
    if (frame.body_len < 1 + kMsgRefLen) return;
    const uint32_t msg_ref = readU32Be(frame.body + 1);
    if (store_.retain(frame, identity_.nodeId(), msg_ref, now_ms)) {
        ++stats_.store_retained;
    }
}

void Engine::maybeOfferTo(const NodeId& peer, bool fresh_announce, uint64_t now_ms) {
    if (!config_.store_role) return;
    if (!isUnicastAddress(peer)) return;
    uint32_t ids[16];
    const size_t count = store_.idsFor(peer, ids, 16);
    if (count == 0) return;
    OfferState* state = nullptr;
    for (size_t i = 0; i < offer_count_; ++i) {
        if (offers_[i].peer == peer) {
            state = &offers_[i];
            break;
        }
    }
    if (state == nullptr) {
        if (offer_count_ >= OM_PEER_CAPACITY) return;
        state = &offers_[offer_count_++];
        state->peer = peer;
        state->last_offer_ms = 0;
    }
    if (!fresh_announce && (now_ms - state->last_offer_ms) < kStoreOfferIntervalMs) return;
    Frame offer;
    if (buildStoreOffer(peer, now_ms, offer)) state->last_offer_ms = now_ms;
}

void Engine::handleStoreOffer(const Frame& frame, uint64_t now_ms) {
    StoreOffer offer;
    if (!decodeStoreOffer(frame.body, frame.body_len, offer)) {
        ++stats_.malformed;
        return;
    }
    StoreWant want;
    for (uint8_t i = 0; i < offer.count; ++i) {
        bool already = false;
        for (size_t k = 0; k < OM_APP_DEDUP_CAPACITY; ++k) {
            if (app_dedup_[k].msg_ref == offer.ids[i] && app_dedup_[k].src == frame.src) {
                already = true;
                break;
            }
        }
        if (!already && want.count < 16) want.ids[want.count++] = offer.ids[i];
    }
    if (want.count == 0) return;
    Frame want_frame;
    buildStoreWant(frame.src, want, now_ms, want_frame);
}

void Engine::handleStoreWant(const Frame& frame, uint64_t now_ms) {
    StoreWant want;
    if (!decodeStoreWant(frame.body, frame.body_len, want)) {
        ++stats_.malformed;
        return;
    }
    for (uint8_t i = 0; i < want.count; ++i) {
        StoredEnvelope env;
        if (!store_.take(frame.src, want.ids[i], env)) continue;
        Frame replay;
        if (decodeFrame(env.frame, env.frame_len, replay).ok()) {
            replay.ttl = config_.default_ttl;  // original bytes with a fresh TTL (AAD ignores TTL)
            if (queueTx(replay, true, NodeId::zero(), now_ms)) {
                ++stats_.store_delivered;
                EngineEvent e;
                e.type = EventType::StoreDelivered;
                e.peer = frame.src;
                events_.push(e);
            }
        }
    }
}

// --- Origination ----------------------------------------------------------------------------
bool Engine::buildAnnounce(uint64_t now_ms, Frame& out) {
    if (!started_) return false;
    Announce announce;
    announce.cls = config_.cls;
    announce.caps = config_.caps;
    announce.setName(config_.name, config_.name_len);
    announce.timestamp_ms = wallNow(now_ms);
    announce.interval_s = (uint16_t)(config_.announce_interval_ms / 1000u);
    announce.ident = identity_.publicIdentity();
    uint8_t unsigned_body[kAnnounceBodyLen];
    const size_t unsigned_len = encodeAnnounceUnsigned(announce, unsigned_body,
                                                        sizeof(unsigned_body));
    if (unsigned_len == 0) return false;
    out = Frame{};
    out.type = FrameType::Announce;
    out.flags = 0;
    out.ttl = kAnnounceTtl;
    out.src = identity_.nodeId();
    out.dst = NodeId::broadcast();
    out.seq = ++seq_;
    uint8_t payload[kAnnounceBodyLen + 64];
    const size_t payload_len = announceSignaturePayload(out.src, out.seq, unsigned_body,
                                                         unsigned_len, payload, sizeof(payload));
    if (payload_len == 0) return false;
    uint8_t signature[kSignatureLen];
    if (!identity_.sign(payload, payload_len, signature)) return false;
    const size_t body_len = encodeAnnounceBody(announce, signature, out.body, sizeof(out.body));
    if (body_len == 0) return false;
    out.body_len = (uint16_t)body_len;
    if (!queueTx(out, true, NodeId::zero(), now_ms)) return false;
    ++stats_.announce_tx;
    return true;
}

bool Engine::sealAndQueue(Frame& frame, const uint8_t* plaintext, size_t plaintext_len,
                          bool include_identity, const PublicIdentity* direct_peer,
                          int channel_index, uint8_t payload_flags, uint32_t msg_ref,
                          uint64_t now_ms) {
    uint8_t aad[kHeaderLen];
    frame.aad(aad);
    size_t body_len = 0;
    if (channel_index >= 0) {
        if (channel_index >= (int)OM_CHANNEL_CAPACITY || !channels_[channel_index].used)
            return false;
        ChannelKey channel;
        if (!ChannelKey::derive(channels_[channel_index].psk, channels_[channel_index].name,
                                channels_[channel_index].name_len, channel))
            return false;
        body_len = channel.sealBody(rng_, rng_ctx_, msg_ref, payload_flags, aad, plaintext,
                                    plaintext_len, frame.body, sizeof(frame.body));
    } else {
        if (direct_peer == nullptr) return false;
        body_len = identity_.sealDirectBody(rng_, rng_ctx_, *direct_peer, msg_ref, payload_flags,
                                            include_identity, aad, plaintext, plaintext_len,
                                            frame.body, sizeof(frame.body));
    }
    if (body_len == 0) return false;
    frame.body_len = (uint16_t)body_len;
    return queueTx(frame, true, NodeId::zero(), now_ms);
}

bool Engine::buildChannelAppMessage(const AppMessage& message, int channel_index,
                                    uint64_t now_ms, Frame& out) {
    if (!started_) return false;
    if (channel_index < 0 || channel_index >= (int)OM_CHANNEL_CAPACITY) return false;
    if (!channels_[channel_index].used) return false;
    uint8_t message_cbor[kMaxBody];
    const size_t message_len = encodeAppMessage(message, message_cbor, sizeof(message_cbor));
    if (message_len == 0) return false;
    uint8_t payload[kMaxBody];
    const size_t payload_len = groupSignaturePayload(
        identity_.nodeId(), channels_[channel_index].id, message_cbor, message_len, payload,
        sizeof(payload));
    if (payload_len == 0) return false;
    uint8_t signature[kSignatureLen];
    if (!identity_.sign(payload, payload_len, signature)) return false;
    uint8_t plaintext[kMaxBody];
    const size_t plaintext_len = encodeGroupMessage(identity_.nodeId(), signature, message,
                                                     plaintext, sizeof(plaintext));
    if (plaintext_len == 0) return false;
    // GroupMessage overhead (signature = 64 bytes) means a position usually does not fit in a
    // single channel frame; the caller falls back to a direct message (see buildPosition).
    if (1 + kMsgRefLen + kSealOverhead + plaintext_len > kMaxBody) return false;
    out = Frame{};
    out.type = FrameType::Data;
    out.flags = kFlagEncrypted;
    out.ttl = config_.default_ttl;
    out.src = identity_.nodeId();
    out.dst = channels_[channel_index].id;
    out.seq = ++seq_;
    return sealAndQueue(out, plaintext, plaintext_len, false, nullptr, channel_index, 0,
                        message.id, now_ms);
}

bool Engine::sendAppDirect(const NodeId& peer_id, const AppMessage& message,
                           uint8_t payload_flags, uint64_t now_ms, Frame& out) {
    const PeerInfo* peer = findPeer(peer_id);
    if (peer == nullptr || !peer->ident.isSelfConsistent()) return false;
    uint8_t cbor[kMaxBody];
    const size_t cbor_len = encodeAppMessage(message, cbor, sizeof(cbor));
    if (cbor_len == 0) return false;
    out = Frame{};
    out.type = FrameType::Data;
    out.flags = kFlagEncrypted | kFlagAckRequest;
    out.ttl = config_.default_ttl;
    out.src = identity_.nodeId();
    out.dst = peer_id;
    out.seq = ++seq_;
    return sealAndQueue(out, cbor, cbor_len, false, &peer->ident, -1, payload_flags, message.id,
                        now_ms);
}

bool Engine::buildPosition(const Position& position, uint32_t msg_id, int8_t channel_index,
                           uint64_t now_ms, Frame& out) {
    AppMessage message;
    message.id = msg_id;
    message.kind = MessageKind::Position;
    message.sent_at_ms = position.timestamp_ms;
    message.has_position = true;
    message.position = position;
    if (channel_index >= 0) {
        if (buildChannelAppMessage(message, channel_index, now_ms, out)) return true;
    }
    // Fallback for trackers: a direct message to the best known infrastructure peer (a gateway
    // or repeater) fits easily, whereas a signed GroupMessage often exceeds the MTU.
    const NodeId* best = nullptr;
    NodeId best_id{};
    int best_rank = -1;
    for (size_t i = 0; i < peer_count_; ++i) {
        if (!peers_[i].used || !peers_[i].ident.isSelfConsistent()) continue;
        int rank = 0;
        if (peers_[i].cls == NodeClass::Gateway) rank = 3;
        else if (peers_[i].cls == NodeClass::Repeater) rank = 2;
        else if (peers_[i].cls == NodeClass::Client) rank = 1;
        if (rank > best_rank) {
            best_rank = rank;
            best_id = peers_[i].ident.node_id;
            best = &best_id;
        }
    }
    if (best == nullptr) return false;
    return sendAppDirect(*best, message, 0, now_ms, out);
}

bool Engine::buildTelemetry(const Telemetry& telemetry, uint64_t now_ms, Frame& out) {
    if (!started_) return false;
    AppMessage message;
    message.id = nextRandomId();
    message.kind = MessageKind::Telemetry;
    message.sent_at_ms = telemetry.timestamp_ms;
    message.has_telemetry = true;
    message.telemetry = telemetry;
    for (size_t i = 0; i < OM_CHANNEL_CAPACITY; ++i) {
        if (channels_[i].used && buildChannelAppMessage(message, (int)i, now_ms, out)) return true;
    }
    // Direct fallback to a known infrastructure peer.
    for (size_t i = 0; i < peer_count_; ++i) {
        if (!peers_[i].used || !peers_[i].ident.isSelfConsistent()) continue;
        if (peers_[i].cls != NodeClass::Gateway && peers_[i].cls != NodeClass::Repeater) continue;
        if (sendAppDirect(peers_[i].ident.node_id, message, 0, now_ms, out)) return true;
    }
    return false;
}

bool Engine::buildAck(const NodeId& to, const Ack& ack, uint64_t now_ms, Frame& out) {
    const PeerInfo* peer = findPeer(to);
    if (peer == nullptr || !peer->ident.isSelfConsistent()) return false;
    uint8_t plaintext[128];
    const size_t plaintext_len = encodeAck(ack, plaintext, sizeof(plaintext));
    if (plaintext_len == 0) return false;
    out = Frame{};
    out.type = FrameType::Ack;
    out.flags = kFlagEncrypted | kFlagPriority;  // ACKs never request ACKs
    out.ttl = config_.default_ttl;
    out.src = identity_.nodeId();
    out.dst = to;
    out.seq = ++seq_;
    uint8_t aad[kHeaderLen];
    out.aad(aad);
    const size_t body_len = identity_.sealDirectBody(rng_, rng_ctx_, peer->ident, ack.msg_id,
                                                      kPayloadFlagIsAck, false, aad, plaintext,
                                                      plaintext_len, out.body, sizeof(out.body));
    if (body_len == 0) return false;
    out.body_len = (uint16_t)body_len;
    if (!queueTx(out, true, NodeId::zero(), now_ms)) return false;
    ++stats_.acks_sent;
    return true;
}

bool Engine::buildStoreOffer(const NodeId& to, uint64_t now_ms, Frame& out) {
    StoreOffer offer;
    offer.count = (uint8_t)store_.idsFor(to, offer.ids, 16);
    if (offer.count == 0) return false;
    uint8_t body[kMaxBody];
    const size_t body_len = encodeStoreOffer(offer, body, sizeof(body));
    if (body_len == 0) return false;
    out = Frame{};
    out.type = FrameType::StoreOffer;
    out.flags = kFlagPriority;
    out.ttl = config_.default_ttl;
    out.src = identity_.nodeId();
    out.dst = to;
    out.seq = ++seq_;
    std::memcpy(out.body, body, body_len);
    out.body_len = (uint16_t)body_len;
    return queueTx(out, true, NodeId::zero(), now_ms);
}

bool Engine::buildStoreWant(const NodeId& to, const StoreWant& want, uint64_t now_ms,
                            Frame& out) {
    uint8_t body[kMaxBody];
    const size_t body_len = encodeStoreWant(want, body, sizeof(body));
    if (body_len == 0) return false;
    out = Frame{};
    out.type = FrameType::StoreWant;
    out.flags = kFlagPriority;
    out.ttl = config_.default_ttl;
    out.src = identity_.nodeId();
    out.dst = to;
    out.seq = ++seq_;
    std::memcpy(out.body, body, body_len);
    out.body_len = (uint16_t)body_len;
    return queueTx(out, true, NodeId::zero(), now_ms);
}

bool Engine::originateFromApp(const uint8_t* frame_bytes, size_t len, uint64_t now_ms) {
    Frame frame;
    if (!decodeFrame(frame_bytes, len, frame).ok()) return false;
    if (!outbox_.push(frame, true, NodeId::zero())) {
        ++stats_.dropped;
        return false;
    }
    dedup_.checkAndInsert(frame.src, frame.seq, now_ms);
    ++stats_.tx_frames;
    return true;
}

// --- Periodic -------------------------------------------------------------------------------
void Engine::tick(uint64_t now_ms) {
    if (!started_) return;
    last_tick_ms_ = now_ms;
    const size_t before = store_.size();
    store_.prune(now_ms);
    if (store_.size() < before) stats_.store_expired += (uint32_t)(before - store_.size());
    if (now_ms >= next_announce_ms_) {
        Frame announce;
        buildAnnounce(now_ms, announce);
        scheduleNextAnnounce(now_ms, false);
    }
    if (now_ms >= next_telemetry_ms_) {
        Telemetry telemetry;
        telemetry.timestamp_ms = wallNow(now_ms);
        telemetry.has_uptime_s = true;
        telemetry.uptime_s = (uint32_t)((now_ms - boot_ms_) / 1000u);
        if (battery_.valid) {
            telemetry.has_battery_mv = true;
            telemetry.battery_mv = battery_.battery_mv;
            telemetry.has_battery_pct = true;
            telemetry.battery_pct = battery_.percent;
            if (battery_.solar_mv > 0) {
                telemetry.has_solar_mv = true;
                telemetry.solar_mv = battery_.solar_mv;
            }
        }
        telemetry.has_airtime_used_ms = true;
        telemetry.airtime_used_ms = stats_.airtime_ms;
        telemetry.has_rx_frames = true;
        telemetry.rx_frames = stats_.rx_frames;
        telemetry.has_tx_frames = true;
        telemetry.tx_frames = stats_.tx_frames;
        Frame frame;
        buildTelemetry(telemetry, now_ms, frame);
        next_telemetry_ms_ = now_ms + config_.telemetry_interval_ms;
    }
    // Store offers: scan peers at most every 30 s to keep the loop cheap.
    if (config_.store_role && now_ms - last_offer_scan_ms_ >= 30000u) {
        last_offer_scan_ms_ = now_ms;
        for (size_t i = 0; i < peer_count_; ++i) {
            if (!peers_[i].used) continue;
            if (now_ms - peers_[i].last_heard_ms > kPeerExpiryMs) {
                peers_[i].used = false;
                continue;
            }
            maybeOfferTo(peers_[i].ident.node_id, false, now_ms);
        }
    }
}

void Engine::setWallTime(uint64_t unix_ms, uint64_t now_ms) {
    wall_base_unix_ms_ = unix_ms;
    wall_base_mono_ms_ = now_ms;
    wall_valid_ = true;
}

uint64_t Engine::wallNow(uint64_t now_ms) const {
    return wallOrBoot(wall_base_unix_ms_, wall_base_mono_ms_, wall_valid_, now_ms);
}

void Engine::setBattery(const BatteryReading& reading, uint64_t now_ms) {
    battery_ = reading;
    const bool has_solar = (config_.caps & kCapSolar) != 0;
    const bool is_repeater = config_.cls == NodeClass::Repeater ||
                             config_.cls == NodeClass::Gateway;
    const PowerPolicy previous = power_;
    power_ = evaluatePower(reading, has_solar, is_repeater);
    if (power_.critical && !previous.critical) {
        EngineEvent e;
        e.type = EventType::LowBattery;
        e.battery = reading;
        copyName(e.detail, sizeof(e.detail), "critical", 8);
        events_.push(e);
    } else if (power_.low_power && !previous.low_power) {
        EngineEvent e;
        e.type = EventType::LowBattery;
        e.battery = reading;
        copyName(e.detail, sizeof(e.detail), "low", 3);
        events_.push(e);
    }
    (void)now_ms;
}

void Engine::setGpsFix(const GpsFix& fix, uint64_t now_ms) {
    if (!fix.valid || !fix.has_position) return;
    if (!config_.position_sharing || !power_.position_sharing) return;
    const uint32_t interval = power_.low_power ? power_.tracker_sleep_ms : 300000u;
    if (last_gps_position_ms_ != 0 && now_ms - last_gps_position_ms_ < interval) return;
    // Find the first configured channel; positions are normally shared to a channel.
    int channel_index = -1;
    for (size_t i = 0; i < OM_CHANNEL_CAPACITY; ++i) {
        if (channels_[i].used) {
            channel_index = (int)i;
            break;
        }
    }
    if (channel_index < 0) return;
    Position position;
    position.lat_e7 = fix.lat_e7;
    position.lon_e7 = fix.lon_e7;
    position.has_alt = fix.has_alt;
    position.alt_m = fix.alt_m;
    position.has_accuracy = fix.has_accuracy;
    position.accuracy_m = fix.accuracy_m;
    position.has_speed = fix.has_speed;
    position.speed_cm_s = fix.speed_cm_s;
    position.has_heading = fix.has_heading;
    position.heading_deg = fix.heading_deg;
    position.timestamp_ms = wallNow(now_ms);
    Frame frame;
    if (buildPosition(position, nextRandomId(), (int8_t)channel_index, now_ms, frame)) {
        last_gps_position_ms_ = now_ms;
    }
}

uint32_t Engine::nextRandomId() {
    if (rng_ == nullptr) return ++seq_;
    uint8_t bytes[4];
    rng_(rng_ctx_, bytes, sizeof(bytes));
    uint32_t v = readU32Be(bytes);
    return v == 0 ? 1 : v;
}

// --- Persistence ----------------------------------------------------------------------------
bool Engine::saveStore(uint64_t now_ms) {
    if (storage_ == nullptr) return false;
    uint8_t seq_bytes[8];
    seq_bytes[0] = 'O';
    seq_bytes[1] = 'M';
    seq_bytes[2] = 'Q';
    seq_bytes[3] = 1;
    writeU32Be(seq_bytes + 4, seq_);
    if (!storage_->save(kSeqFileName, seq_bytes, sizeof(seq_bytes))) return false;
    const size_t len = store_.serialize(save_scratch_, sizeof(save_scratch_));
    if (len == 0) return false;
    if (!storage_->save(kStoreFileName, save_scratch_, len)) return false;
    store_.clearDirty();
    last_store_save_ms_ = now_ms;
    return true;
}

void Engine::loadStore(uint64_t now_ms) {
    if (storage_ == nullptr) return;
    uint8_t seq_bytes[8];
    size_t len = 0;
    if (storage_->load(kSeqFileName, seq_bytes, sizeof(seq_bytes), len) && len == 8 &&
        seq_bytes[0] == 'O' && seq_bytes[1] == 'M' && seq_bytes[2] == 'Q') {
        seq_ = readU32Be(seq_bytes + 4);
    }
    // The store file is ~13 KiB; the board storage adapter loads it into a static buffer and
    // calls loadStoreBuffer() after Engine::begin() (see firmware/src/storage_backend.cpp).
    (void)now_ms;
}

void Engine::loadStoreBuffer(const uint8_t* data, size_t len, uint64_t now_ms) {
    store_.deserialize(data, len, now_ms);
}

// --- Companion helpers ----------------------------------------------------------------------
void Engine::fillHelloAck(CompanionMsg& msg) const {
    msg.type = CompanionType::HelloAck;
    msg.proto = kProtoVersion;
    copyName(msg.fw_version, sizeof(msg.fw_version), OM_FW_VERSION, std::strlen(OM_FW_VERSION));
    msg.node_id = identity_.nodeId();
    msg.max_frame = (uint16_t)kMtu;
    msg.caps = config_.caps;
    copyName(msg.region, sizeof(msg.region), config_.region->name,
             std::strlen(config_.region->name));
    copyName(msg.profile, sizeof(msg.profile), config_.profile->name,
             std::strlen(config_.profile->name));
}

void Engine::fillConfig(CompanionMsg& msg) const {
    msg.type = CompanionType::Config;
    msg.entry_count = 0;
    auto add = [&](const char* k, const char* v) {
        if (msg.entry_count >= kCompanionMaxEntries) return;
        copyName(msg.entries[msg.entry_count].key, sizeof(msg.entries[0].key), k,
                 std::strlen(k));
        copyName(msg.entries[msg.entry_count].value, sizeof(msg.entries[0].value), v,
                 std::strlen(v));
        ++msg.entry_count;
    };
    add("node.name", config_.name);
    add("node.class", nodeClassName(config_.cls));
    add("radio.region", config_.region->name);
    add("radio.profile", config_.profile->name);
    char tmp[16];
    u64ToText((uint64_t)(int64_t)config_.tx_power_dbm, tmp, sizeof(tmp));
    add("radio.tx_power", tmp);
    u64ToText(config_.default_ttl, tmp, sizeof(tmp));
    add("mesh.ttl", tmp);
    add("mesh.store", config_.store_role ? "1" : "0");
    u64ToText(config_.announce_interval_ms / 1000u, tmp, sizeof(tmp));
    add("mesh.announce_interval", tmp);
    char signed_tmp[16];
    u64ToText((uint64_t)(int64_t)config_.duty_override_pct, signed_tmp, sizeof(signed_tmp));
    add("radio.duty_override", signed_tmp);
    u64ToText(config_.frequency_override_khz, tmp, sizeof(tmp));
    add("radio.frequency", tmp);
}

void Engine::fillStats(CompanionMsg& msg) const {
    msg.type = CompanionType::Stats;
    msg.entry_count = 0;
    auto add = [&](const char* k, uint64_t v) {
        if (msg.entry_count >= kCompanionMaxEntries) return;
        copyName(msg.entries[msg.entry_count].key, sizeof(msg.entries[0].key), k,
                 std::strlen(k));
        u64ToText(v, msg.entries[msg.entry_count].value, sizeof(msg.entries[0].value));
        ++msg.entry_count;
    };
    add("rx_frames", stats_.rx_frames);
    add("tx_frames", stats_.tx_frames);
    add("relayed", stats_.relayed);
    add("dedup_drops", stats_.duplicates);
    add("decrypt_errors", stats_.decrypt_errors);
    add("store_count", store_.size());
    add("airtime_ms", stats_.airtime_ms);
    add("uptime_s", (uint64_t)((last_tick_ms_ > boot_ms_) ? (last_tick_ms_ - boot_ms_) / 1000 : 0));
    add("override_active", override_active_ ? 1 : 0);
    add("duty_pct", (uint64_t)(int64_t)effectiveDutyPctValue());
    add("tx_power_dbm", (uint64_t)(int64_t)config_.tx_power_dbm);
    add("frequency_khz", (uint64_t)effectiveFrequencyKhzValue());
}

// --- Config ---------------------------------------------------------------------------------
bool Engine::applyConfig(const char* key, const char* value) {
    if (key == nullptr || value == nullptr) return false;
    auto eq = [](const char* a, const char* b) { return std::strcmp(a, b) == 0; };
    bool accepted = false;
    bool policy_changed = false;

    if (eq(key, "node.name")) {
        copyName(config_.name, sizeof(config_.name), value, std::strlen(value));
        config_.name_len = (uint8_t)std::strlen(config_.name);
        accepted = true;
    } else if (eq(key, "node.class")) {
        NodeClass cls;
        if (!parseNodeClass((const uint8_t*)value, std::strlen(value), cls)) return false;
        config_.cls = cls;
        accepted = true;
    } else if (eq(key, "radio.region")) {
        const Region* region = regionByName(value);
        if (region == nullptr) return false;
        config_.region = region;
        // A profile that is legal in the old region may exceed the new region's max bandwidth
        // (e.g. SHORT_TURBO 500 kHz -> EU868 250 kHz). Fall back to a safe preset with a visible
        // Config reply rather than silently running an illegal bandwidth.
        if (!profileFitsRegion(*config_.profile, *config_.region)) {
            config_.profile = (config_.region == &kRegionEu433) ? &kLongFast : &kMediumFast;
        }
        policy_changed = true;
        accepted = true;
    } else if (eq(key, "radio.profile")) {
        const LoraProfile* profile = profileByName(value);
        if (profile == nullptr) return false;
        // Reject instead of clamping: a 500 kHz preset must never run in a 250 kHz region.
        if (!profileFitsRegion(*profile, *config_.region)) return false;
        config_.profile = profile;
        accepted = true;
    } else if (eq(key, "radio.tx_power")) {
        const long v = std::strtol(value, nullptr, 10);
        // Store the request; effectivePowerDbm() applies the legal-region ceiling unless the
        // override is active, and always the board PA cap.
        config_.power_override_dbm = (int8_t)(v < -9 ? -9 : (v > 30 ? 30 : v));
        policy_changed = true;
        accepted = true;
    } else if (eq(key, "radio.duty_override")) {
        const long v = std::strtol(value, nullptr, 10);
        if (v < -1 || v > 100) return false;
        config_.duty_override_pct = (int16_t)v;
        policy_changed = true;
        accepted = true;
    } else if (eq(key, "radio.frequency")) {
        const long v = std::strtol(value, nullptr, 10);
        if (v < 0 || (uint32_t)v > 1000000000L) return false;
        if (v == 0) {
            config_.frequency_override_khz = 0;  // clear override, back to region centre
        } else {
            if (!validateFrequencyKhz((uint32_t)v)) return false;  // out of radio range
            config_.frequency_override_khz = (uint32_t)v;
        }
        policy_changed = true;
        accepted = true;
    } else if (eq(key, "mesh.ttl")) {
        const long v = std::strtol(value, nullptr, 10);
        if (v < 1 || v > 15) return false;
        config_.default_ttl = (uint8_t)v;
        accepted = true;
    } else if (eq(key, "mesh.store")) {
        const bool on = std::strcmp(value, "1") == 0;
        config_.store_role = on;
        if (on) config_.caps |= kCapStore;
        else config_.caps &= (uint16_t)~kCapStore;
        accepted = true;
    } else if (eq(key, "mesh.store_capacity")) {
        accepted = true;  // compile-time capacity (OM_STORE_CAPACITY); accepted but not resized
    } else if (eq(key, "mesh.announce_interval")) {
        const long v = std::strtol(value, nullptr, 10);
        if (v < 300) return false;  // 5 min floor
        config_.announce_interval_ms = (uint32_t)v * 1000u;
        accepted = true;
    } else if (eq(key, "mesh.position_sharing")) {
        config_.position_sharing = std::strcmp(value, "1") == 0;
        accepted = true;
    } else if (eq(key, "gps.enabled")) {
        const bool on = std::strcmp(value, "1") == 0;
        if (on) config_.caps |= kCapGps;
        else config_.caps &= (uint16_t)~kCapGps;
        accepted = true;
    }
    if (!accepted) return false;
    if (policy_changed) recomputeRegulatoryPolicy();
    persistConfig();
    return true;
}

// --- Regulatory policy ----------------------------------------------------------------------
void Engine::recomputeRegulatoryPolicy() {
    const bool active = overrideExceedsLegal(*config_.region, config_.duty_override_pct,
                                             config_.frequency_override_khz);
    override_active_ = active;
    config_.tx_power_dbm = effectivePowerDbm(*config_.region, config_.hardware_max_power_dbm,
                                             config_.power_override_dbm, active);
    budget_.configure(*config_.region, config_.duty_override_pct, true);
    if (active && !override_warned_) {
        // Short machine-readable marker; the full warning text is exposed via overrideWarning()
        // and logged by the board HAL/serial console.
        override_warned_ = true;
        EngineEvent e;
        e.type = EventType::Error;
        copyName(e.detail, sizeof(e.detail), "regulatory_override_active", 26);
        events_.push(e);
    } else if (!active) {
        override_warned_ = false;
    }
}

bool Engine::persistConfig() {
    if (storage_ == nullptr) return false;
    uint8_t buf[96];
    std::memset(buf, 0, sizeof(buf));
    size_t off = 0;
    buf[off++] = 'O';
    buf[off++] = 'M';
    buf[off++] = 'C';
    buf[off++] = 2;
    auto putName = [&](const char* name, size_t max_len) {
        const size_t n = name != nullptr ? std::strlen(name) : 0;
        const size_t take = n < max_len ? n : max_len - 1;
        std::memcpy(buf + off, name != nullptr ? name : "", take);
        off += max_len;
    };
    putName(config_.region->name, 16);
    putName(config_.profile->name, 16);
    putName(config_.name, 33);
    buf[off++] = config_.name_len;
    buf[off++] = (uint8_t)config_.cls;
    buf[off++] = config_.store_role ? 1 : 0;
    buf[off++] = config_.position_sharing ? 1 : 0;
    buf[off++] = (config_.caps & kCapGps) != 0 ? 1 : 0;
    buf[off++] = config_.default_ttl;
    writeU32Be(buf + off, config_.announce_interval_ms);
    off += 4;
    writeU16Be(buf + off, (uint16_t)(int16_t)config_.duty_override_pct);
    off += 2;
    buf[off++] = (uint8_t)(int8_t)config_.power_override_dbm;
    writeU32Be(buf + off, config_.frequency_override_khz);
    off += 4;
    writeU32Be(buf + off, crc32(buf, off));
    off += 4;
    return storage_->save(configFileName(), buf, off);
}

void Engine::loadPersistedConfig() {
    if (storage_ == nullptr) return;
    uint8_t buf[96];
    size_t len = 0;
    if (!storage_->load(configFileName(), buf, sizeof(buf), len) || len < 80) return;
    if (buf[0] != 'O' || buf[1] != 'M' || buf[2] != 'C' || buf[3] != 2) return;
    const uint32_t expected_crc = readU32Be(buf + len - 4);
    if (crc32(buf, len - 4) != expected_crc) return;
    size_t off = 4;
    const Region* region = regionByName((const char*)(buf + off));
    off += 16;
    const LoraProfile* profile = profileByName((const char*)(buf + off));
    off += 16;
    char name[33];
    std::memcpy(name, buf + off, 32);
    name[32] = '\0';
    off += 33;
    const uint8_t name_len = buf[off++];
    const NodeClass cls = (NodeClass)buf[off++];
    config_.store_role = buf[off++] != 0;
    config_.position_sharing = buf[off++] != 0;
    const bool gps = buf[off++] != 0;
    config_.default_ttl = buf[off++];
    config_.announce_interval_ms = readU32Be(buf + off);
    off += 4;
    config_.duty_override_pct = (int16_t)readU16Be(buf + off);
    off += 2;
    config_.power_override_dbm = (int8_t)buf[off++];
    config_.frequency_override_khz = readU32Be(buf + off);
    off += 4;
    if (region != nullptr) config_.region = region;
    if (profile != nullptr && profileFitsRegion(*profile, *config_.region)) {
        config_.profile = profile;
    } else if (!profileFitsRegion(*config_.profile, *config_.region)) {
        config_.profile = (config_.region == &kRegionEu433) ? &kLongFast : &kMediumFast;
    }
    if (name_len < sizeof(config_.name)) {
        std::memcpy(config_.name, name, name_len);
        config_.name[name_len] = '\0';
        config_.name_len = name_len;
    }
    if ((uint8_t)cls <= (uint8_t)NodeClass::Tracker) config_.cls = cls;
    if (gps) config_.caps |= kCapGps;
    else config_.caps &= (uint16_t)~kCapGps;
    if (config_.store_role) config_.caps |= kCapStore;
    else config_.caps &= (uint16_t)~kCapStore;
    recomputeRegulatoryPolicy();
}

void Engine::setChannel(const char* name, const uint8_t psk[32]) {
    if (name == nullptr) return;
    const size_t name_len = std::strlen(name);
    for (size_t i = 0; i < OM_CHANNEL_CAPACITY; ++i) {
        if (channels_[i].used && std::strncmp(channels_[i].name, name, sizeof(channels_[i].name)) == 0) {
            // Update the PSK in place.
            std::memcpy(channels_[i].psk, psk, 32);
            ChannelKey key;
            ChannelKey::derive(channels_[i].psk, channels_[i].name, channels_[i].name_len, key);
            channels_[i].id = key.id;
            return;
        }
    }
    for (size_t i = 0; i < OM_CHANNEL_CAPACITY; ++i) {
        if (!channels_[i].used) {
            channels_[i].used = true;
            copyName(channels_[i].name, sizeof(channels_[i].name), name, name_len);
            channels_[i].name_len = (uint8_t)std::strlen(channels_[i].name);
            std::memcpy(channels_[i].psk, psk, 32);
            ChannelKey key;
            ChannelKey::derive(channels_[i].psk, channels_[i].name, channels_[i].name_len, key);
            channels_[i].id = key.id;
            config_.accept_channels = true;
            return;
        }
    }
}

void Engine::clearChannels() {
    for (size_t i = 0; i < OM_CHANNEL_CAPACITY; ++i) channels_[i] = ChannelConfig{};
}

int Engine::channelIndex(const NodeId& id) const {
    for (size_t i = 0; i < OM_CHANNEL_CAPACITY; ++i) {
        if (channels_[i].used && channels_[i].id == id) return (int)i;
    }
    return -1;
}

bool Engine::channelIsValid(int index) const {
    return index >= 0 && index < (int)OM_CHANNEL_CAPACITY && channels_[index].used;
}

}  // namespace om
