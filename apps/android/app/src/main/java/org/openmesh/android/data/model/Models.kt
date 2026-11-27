package org.openmesh.android.data.model

import org.openmesh.android.data.transport.LinkError
import org.openmesh.android.data.transport.RadioHealth
import org.openmesh.android.data.transport.RadioPhase

/**
 * Domain models for the Android client.
 *
 * These are deliberately close to the bridge WebSocket event payloads
 * (docs/architecture/WEBSOCKET-API.md) and the companion API's CBOR messages
 * (docs/architecture/COMPANION-API.md), so the same models can be populated either by the
 * bridge client or by [org.openmesh.android.data.repository.MeshRepository] once the Rust core
 * is wired in through `openmesh-ffi`.
 */

enum class NodeClass {
    CLIENT, REPEATER, GATEWAY, TRACKER, ROOM_SERVER;

    val label: String
        get() = when (this) {
            CLIENT -> "Client"
            REPEATER -> "Repeater"
            GATEWAY -> "Gateway"
            TRACKER -> "Tracker"
            ROOM_SERVER -> "Room server"
        }

    companion object {
        fun fromWire(value: String): NodeClass = when (value.lowercase()) {
            "repeater" -> REPEATER
            "gateway" -> GATEWAY
            "tracker" -> TRACKER
            "room_server", "room-server" -> ROOM_SERVER
            else -> CLIENT
        }
    }
}

enum class MessageKind { TEXT, POSITION, ATTACHMENT, TELEMETRY }

enum class DeliveryState { QUEUED, SENT, DELIVERED, READ, FAILED, STORED_AT_RELAY }

data class Position(
    val lat: Double,
    val lon: Double,
    val altM: Int? = null,
    val accuracyM: Int? = null,
    val speedCmS: Int? = null,
    val headingDeg: Int? = null,
    val timestampMs: Long = 0L,
)

data class Attachment(
    val id: String,
    val mime: String,
    val sizeBytes: Int,
    val sha256: String? = null,
    val durationMs: Int? = null,
    val name: String? = null,
    /** 0..1 while the resource is transferring; 1 when complete. */
    val progress: Float = 1f,
)

data class Message(
    val id: Long,
    val peerId: String,
    val fromId: String,
    val toId: String,
    val kind: MessageKind,
    val text: String? = null,
    val sentAtMs: Long,
    val receivedAtMs: Long? = null,
    val outgoing: Boolean = false,
    val delivery: DeliveryState = DeliveryState.QUEUED,
    val read: Boolean = false,
    val attachment: Attachment? = null,
    val position: Position? = null,
    /** Envelope arrived replayed from a relay's store-and-forward queue. */
    val viaStore: Boolean = false,
    /** Message traversed the M4 Reticulum bridge (gateway sees plaintext; shown to users). */
    val bridged: Boolean = false,
) {
    val isText: Boolean get() = kind == MessageKind.TEXT && !text.isNullOrBlank()
}

data class NodeInfo(
    val nodeId: String,
    val name: String,
    val nodeClass: NodeClass,
    val caps: Set<String> = emptySet(),
    val hops: Int = 0,
    val lastSeenMs: Long = 0L,
    val rssi: Int? = null,
    val snr: Float? = null,
    val batteryPct: Int? = null,
    val position: Position? = null,
    val isContact: Boolean = false,
)

data class Contact(
    val nodeId: String,
    val name: String,
    val identityBase64: String,
    val lastSeenMs: Long = 0L,
    val verified: Boolean = false,
    val bridged: Boolean = false,
)

data class Channel(
    val id: String,
    val name: String,
    val unreadCount: Int = 0,
    val pskSet: Boolean = true,
)

data class Conversation(
    val peerId: String,
    val peerName: String,
    val lastMessage: Message? = null,
    val unreadCount: Int = 0,
    val online: Boolean = false,
    val contact: Boolean = false,
)

data class RadioStatus(
    val connected: Boolean = false,
    val transportName: String = "simulator",
    val firmwareVersion: String? = null,
    val nodeId: String? = null,
    val region: String = "EU868",
    val profile: String = "MEDIUM_FAST",
    val txQueue: Int = 0,
    val airtimePct: Float = 0f,
    val rssi: Int? = null,
    val snr: Float? = null,
    // ---- Radio-link UX (v0.8): typed phase, persistent failure and liveness. ----
    /** Current link phase; [connected] is derived as `phase == READY` for real links. */
    val phase: RadioPhase = RadioPhase.NONE,
    /** Plain-language progress line published by the transport ("Looking for OpenMesh radios…"). */
    val phaseMessage: String = "",
    val deviceName: String? = null,
    val deviceAddress: String? = null,
    /** Candidate radios seen during the last scan (0 when not scanning). */
    val devicesFound: Int = 0,
    /** Persistent, user-readable failure reason; stays visible until the next connect attempt. */
    val lastError: String? = null,
    /** 1–2 beginner-language bullets on what to try next. */
    val lastErrorHint: String? = null,
    /** Typed failure reason behind [lastError] (drives the step rail and help links). */
    val lastErrorKind: LinkError? = null,
    /** System.currentTimeMillis() when [phase] started (elapsed counter in the radio screen). */
    val phaseSinceMs: Long = 0L,
    /** Liveness of a READY link; STALE means the radio stopped answering keep-alives. */
    val health: RadioHealth = RadioHealth.UNKNOWN,
    /** Last inbound companion message timestamp (hello/rx/stats/pong/config). */
    val lastReplyMs: Long = 0L,
    /** Messages the app is holding locally until the radio link is ready. */
    val queuedMessages: Int = 0,
)

/** Settings persisted locally; mirrors the subset of companion config keys we expose. */
data class AppSettings(
    val nodeName: String = "openmesh-android",
    val role: String = "client",
    val profile: String = "MEDIUM_FAST",
    val region: String = "EU868",
    val announceIntervalSec: Int = 1800,
    val positionSharing: Boolean = false,
    val storeForward: Boolean = true,
    val autoConnect: Boolean = true,
    val preferUsb: Boolean = true,
    val showAirtimeEstimates: Boolean = true,
    val demoMode: Boolean = false,
    val uiMode: String = "beginner",
    val notificationsEnabled: Boolean = true,
    val setupCompleted: Boolean = false,
    val radioKind: String = "none",
    // Regulatory override (v0.5.0): off by default, legal maxima otherwise.
    val overrideEnabled: Boolean = false,
    val overrideAcknowledged: Boolean = false,
    val overrideDutyPct: Int? = null,
    val overridePowerDbm: Int? = null,
    val overrideFreqKhz: Long? = null,
    val overrideReason: String? = null,
    // Media tiers (v0.6.0): defaults come from content/presets/media.json.
    val defaultImageTier: String = "medium",
    val defaultVoiceTier: String = "tiny",
)
