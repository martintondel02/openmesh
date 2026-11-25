// LOCAL-ONLY STUB of the UniFFI-generated bindings (`uniffi.openmesh_ffi`).
//
// This file is compiled only when `app/src/main/java/uniffi/openmesh_ffi/*.kt` is absent, so the
// Compose app can be built and unit-tested before the Rust FFI crate + NDK toolchain have
// produced the real bindings. `apps/android/scripts/build-ffi.sh` (run locally and in CI before
// Gradle) generates the real bindings, and Gradle then excludes this stub automatically.
//
// NEVER ship this stub: the release/debug APKS built by CI call build-ffi.sh first, and
// `OpenMeshApplication` reports "engine not linked" if it is somehow used.
package uniffi.openmesh_ffi

data class FfiConfig(
    val name: String,
    val nodeClass: String,
    val profile: String,
    val region: String,
    val storeForward: Boolean,
    val acceptChannels: Boolean,
    val announceIntervalS: Long,
)

sealed class FfiTarget {
    data class Direct(val nodeIdHex: String) : FfiTarget()
    data class Channel(val channelIdHex: String) : FfiTarget()
}

sealed class FfiSendMode {
    data object Flood : FfiSendMode()
    data class Direct(val peerHex: String) : FfiSendMode()
}

data class FfiOutbound(val frame: ByteArray, val mode: FfiSendMode) {
    override fun equals(other: Any?): Boolean = this === other
    override fun hashCode(): Int = System.identityHashCode(this)
}

data class FfiNode(
    val nodeIdHex: String,
    val name: String,
    val `class`: String,
    val caps: List<String>,
    val hops: Int,
    val lastSeenMs: Long,
    val rssi: Int? = null,
    val snr: Double? = null,
    val batteryPct: Int? = null,
    val lat: Double? = null,
    val lon: Double? = null,
)

data class FfiChannel(val channelIdHex: String, val name: String, val inviteUrl: String, val pskB64: String)
data class FfiChannelInfo(val channelIdHex: String, val name: String, val inviteUrl: String)

data class FfiStats(
    val txFrames: Long = 0,
    val rxFrames: Long = 0,
    val forwarded: Long = 0,
    val dropped: Long = 0,
    val nodes: Long = 0,
    val channels: Long = 0,
    val airtimeMs: Long = 0,
)

sealed class FfiEvent {
    data class Message(val from: String, val msgId: Long, val text: String?, val receivedAtMs: Long, val channel: String?) : FfiEvent()
    data class GroupMessage(val channel: String, val from: String, val msgId: Long, val text: String?) : FfiEvent()
    data class Ack(val msgId: Long, val kind: String) : FfiEvent()
    data class NodeSeen(val node: FfiNode) : FfiEvent()
    data class Position(val from: String, val channel: String?, val lat: Double, val lon: Double) : FfiEvent()
    data class Telemetry(val node: FfiNode) : FfiEvent()
    data class ResourceProgress(val peer: String, val msgId: Long, val resId: Long, val received: Long, val total: Long) : FfiEvent()
    data class ResourceReceived(
        val peer: String, val channel: String?, val msgId: Long, val mime: String,
        val filename: String?, val data: ByteArray, val durationMs: Long?,
    ) : FfiEvent() {
        override fun equals(other: Any?): Boolean = this === other
        override fun hashCode(): Int = System.identityHashCode(this)
    }
    data class AttachmentSent(val peer: String, val msgId: Long) : FfiEvent()
    data class StoreOffer(val relay: String, val count: Long) : FfiEvent()
    data class Error(val message: String) : FfiEvent()
}

data class FfiProcessed(val events: List<FfiEvent> = emptyList(), val outbound: List<FfiOutbound> = emptyList(), val msgId: Long? = null)

sealed class FfiException(message: String) : Exception(message) {
    class InvalidArgument(val msg: String) : FfiException(msg)
    class UnknownPeer(val msg: String) : FfiException(msg)
    class PayloadTooLarge(val msg: String) : FfiException(msg)
    class Internal(val msg: String) : FfiException(msg)
}

/**
 * Stub engine: deterministic, no network, no crypto. Only used for local compilation/unit tests
 * and only while real bindings are absent. It exists so the UI/service code paths can be
 * exercised; `OpenMeshApplication` flags the build as "engine not linked".
 */
class OpenMeshNode(seed: ByteArray, config: FfiConfig) {
    private val configJson = """{"node":{"name":"${config.name}","class":"${config.nodeClass}","announce_interval_s":${config.announceIntervalS}},"radio":{"profile":"${config.profile}","region":"${config.region}"},"features":{"store_forward":${config.storeForward}}}"""
    private var msgCounter = 1L
    private val nodesList = mutableListOf<FfiNode>()
    private val channelList = mutableListOf<FfiChannelInfo>()
    private val identity = "stub-identity-${seed.size}"

    fun nodeId(): String = "5a17c0de5a17c0de"
    fun publicIdentityB64(): String = identity
    fun configJson(): String = configJson
    fun setConfig(jsonPatch: String) = Unit
    fun ingestFrame(frame: ByteArray, nowMs: Long): FfiProcessed = FfiProcessed()
    fun tick(nowMs: Long): FfiProcessed = FfiProcessed()
    fun sendText(target: FfiTarget, text: String, nowMs: Long): FfiProcessed =
        FfiProcessed(msgId = msgCounter++)
    fun sendPosition(target: FfiTarget, lat: Double, lon: Double, altM: Int?, accuracyM: Int?, nowMs: Long): FfiProcessed =
        FfiProcessed(msgId = msgCounter++)
    fun sendAttachment(target: FfiTarget, mime: String, filename: String?, data: ByteArray, durationMs: Long?, nowMs: Long): FfiProcessed =
        FfiProcessed(msgId = msgCounter++)
    fun announce(nowMs: Long): FfiProcessed = FfiProcessed()
    fun retryMessage(msgId: Long, nowMs: Long): FfiProcessed = FfiProcessed()
    fun markRead(msgId: Long, nowMs: Long): FfiProcessed = FfiProcessed()
    fun addContact(identityB64: String, name: String) = Unit
    fun removeContact(nodeIdHex: String) = Unit
    fun createChannel(name: String): FfiChannel =
        FfiChannel("c0de${msgCounter++}c0de", name, "openmesh://channel?name=$name&id=stub&psk=stub", "stub")
    fun joinChannel(name: String, pskB64: String): FfiChannel =
        FfiChannel("c0de${msgCounter++}c0de", name, "openmesh://channel?name=$name&id=stub&psk=stub", pskB64)
    fun channels(): List<FfiChannelInfo> = channelList.toList()
    fun nodes(): List<FfiNode> = nodesList.toList()
    fun stats(): FfiStats = FfiStats(nodes = nodesList.size.toLong(), channels = channelList.size.toLong())
    fun exportState(): ByteArray = "stub-state".toByteArray()
    fun importState(state: ByteArray) = Unit
}
