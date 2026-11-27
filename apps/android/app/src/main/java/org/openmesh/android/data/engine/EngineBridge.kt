package org.openmesh.android.data.engine

import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import org.openmesh.android.data.model.Attachment
import org.openmesh.android.data.model.DeliveryState
import org.openmesh.android.data.model.Message
import org.openmesh.android.data.model.MessageKind
import org.openmesh.android.data.model.NodeClass
import org.openmesh.android.data.model.NodeInfo
import org.openmesh.android.data.model.Position
import uniffi.openmesh_ffi.FfiChannel
import uniffi.openmesh_ffi.FfiChannelInfo
import uniffi.openmesh_ffi.FfiConfig
import uniffi.openmesh_ffi.FfiEvent
import uniffi.openmesh_ffi.FfiException
import uniffi.openmesh_ffi.FfiNode
import uniffi.openmesh_ffi.FfiProcessed
import uniffi.openmesh_ffi.FfiSendMode
import uniffi.openmesh_ffi.FfiTarget
import uniffi.openmesh_ffi.OpenMeshNode
import java.util.concurrent.atomic.AtomicLong

/** Engine configuration (mirrors `FfiConfig` in android-ga.md §1). */
data class EngineConfig(
    val name: String = "openmesh-android",
    val nodeClass: String = "client",
    val profile: String = "MEDIUM_FAST",
    val region: String = "EU868",
    val storeForward: Boolean = false,
    val acceptChannels: Boolean = true,
    val announceIntervalS: Long = 1800,
)

/** Destination of an application message. */
sealed interface Target {
    data class Direct(val nodeIdHex: String) : Target
    data class Channel(val channelIdHex: String) : Target
}

/** One OMP frame the service must hand to the radio, in order. */
data class OutboundFrame(val frame: ByteArray, val directPeerHex: String? = null)

data class CoreStatus(
    val running: Boolean = false,
    val nodeIdHex: String? = null,
    val knownNodes: Int = 0,
    val txFrames: Long = 0,
    val rxFrames: Long = 0,
    val storedEnvelopes: Int = 0,
    val error: String? = null,
)

data class CoreStats(
    val txFrames: Long = 0,
    val rxFrames: Long = 0,
    val forwarded: Long = 0,
    val dropped: Long = 0,
    val nodes: Long = 0,
    val channels: Long = 0,
    val airtimeMs: Long = 0,
    /** Effective duty cycle after the region/override policy (v0.5.0 FFI additive field). */
    val effectiveDutyPct: Int? = null,
)

data class ChannelInvite(
    val channelIdHex: String,
    val name: String,
    val inviteUrl: String,
    val pskB64Once: String? = null,
)

sealed interface CoreEvent {
    data class MessageReceived(val message: Message) : CoreEvent
    data class DeliveryChanged(val messageId: Long, val state: DeliveryState) : CoreEvent
    data class NodeSeen(val node: NodeInfo) : CoreEvent
    data class ResourceProgress(
        val peerId: String,
        val messageId: Long,
        val received: Int,
        val total: Int,
    ) : CoreEvent

    /** A complete attachment arrived; [data] is already decrypted and verified. */
    data class ResourceReceived(
        val peerId: String,
        val channelId: String?,
        val messageId: Long,
        val mime: String,
        val filename: String?,
        val data: ByteArray,
        val durationMs: Long?,
    ) : CoreEvent

    data class StoreOffer(val relayId: String, val count: Int) : CoreEvent
    data class CoreError(val message: String) : CoreEvent
}

/**
 * Boundary to the OpenMesh protocol core. The production implementation is
 * [UniFfiEngineBridge], a thin wrapper over the UniFFI-generated `uniffi.openmesh_ffi` bindings
 * (crates/openmesh-ffi). UI code never calls the generated classes directly.
 */
interface EngineBridge {
    val status: StateFlow<CoreStatus>
    val events: SharedFlow<CoreEvent>
    val outbound: SharedFlow<OutboundFrame>

    /** Create/restore the engine. Safe to call again after [stop] (e.g. mode change). */
    suspend fun start(seed: ByteArray, config: EngineConfig)

    suspend fun stop()

    fun nodeIdHex(): String?
    fun publicIdentityB64(): String?
    fun configJson(): String

    /** Apply a partial config patch (FFI field names, e.g. `{"profile":"LONG_FAST"}`). */
    fun setConfig(patch: String)

    /** Feed one complete OMP frame from the radio; any resulting outbound frames are emitted. */
    fun ingestFrame(frame: ByteArray, nowMs: Long = System.currentTimeMillis()): Result<Unit>

    /** Advance engine timers (call every 250 ms from the service loop). */
    fun tick(nowMs: Long = System.currentTimeMillis()): Result<Unit>

    fun sendText(target: Target, text: String, nowMs: Long = System.currentTimeMillis()): Result<Long>
    fun sendAttachment(
        target: Target,
        mime: String,
        filename: String?,
        data: ByteArray,
        durationMs: Long? = null,
        nowMs: Long = System.currentTimeMillis(),
    ): Result<Long>

    fun sendPosition(
        target: Target,
        lat: Double,
        lon: Double,
        altM: Int? = null,
        accuracyM: Int? = null,
        nowMs: Long = System.currentTimeMillis(),
    ): Result<Long>

    fun announce(nowMs: Long = System.currentTimeMillis()): Result<Unit>
    fun retryMessage(msgId: Long, nowMs: Long = System.currentTimeMillis()): Result<Unit>
    fun markRead(msgId: Long, nowMs: Long = System.currentTimeMillis()): Result<Unit>

    fun addContact(identityB64: String, name: String)
    fun removeContact(nodeIdHex: String)
    fun createChannel(name: String): Result<ChannelInvite>
    fun joinChannel(name: String, pskB64: String): Result<ChannelInvite>
    fun channels(): List<ChannelInvite>
    fun nodes(): List<NodeInfo>
    fun stats(): CoreStats

    /** Opaque JSON state (contacts/channels/counters). Message history lives in Room. */
    fun exportState(): ByteArray
    fun importState(state: ByteArray)

    /** Demo implementation used when the user explicitly enables demo mode. */
    fun isDemo(): Boolean = false
}

/**
 * Production engine: every call goes through the Rust `OpenMeshNode` generated by UniFFI.
 *
 * Threading: the generated object is backed by `Arc<Mutex<Engine>>`; we additionally serialize
 * Kotlin calls with [lock] because the generated Kotlin wrapper is not documented as reentrant.
 * All methods take `nowMs` from the caller (the service loop), so tests can be deterministic.
 */
class UniFfiEngineBridge : EngineBridge {
    private val lock = Any()
    private var node: OpenMeshNode? = null
    private var currentNodeId: String? = null
    private var currentIdentity: String? = null

    private val _status = MutableStateFlow(CoreStatus(running = false))
    override val status: StateFlow<CoreStatus> = _status.asStateFlow()

    private val _events = MutableSharedFlow<CoreEvent>(extraBufferCapacity = 128)
    override val events: SharedFlow<CoreEvent> = _events.asSharedFlow()

    private val _outbound = MutableSharedFlow<OutboundFrame>(extraBufferCapacity = 256)
    override val outbound: SharedFlow<OutboundFrame> = _outbound.asSharedFlow()

    private val msgIdFloor = AtomicLong(0)

    override suspend fun start(seed: ByteArray, config: EngineConfig) {
        stop()
        val ffiConfig = FfiConfig(
            name = config.name,
            nodeClass = config.nodeClass,
            profile = config.profile,
            region = config.region,
            storeForward = config.storeForward,
            acceptChannels = config.acceptChannels,
            announceIntervalS = config.announceIntervalS,
        )
        val created = try {
            OpenMeshNode(seed, ffiConfig)
        } catch (e: FfiException) {
            _status.value = CoreStatus(running = false, error = e.message)
            return
        }
        node = created
        currentNodeId = runCatching { created.nodeId() }.getOrNull()
        currentIdentity = runCatching { created.publicIdentityB64() }.getOrNull()
        _status.value = CoreStatus(running = true, nodeIdHex = currentNodeId)
        val processed = created.tick(System.currentTimeMillis())
        dispatch(processed)
    }

    override suspend fun stop() {
        val current = node ?: return
        runCatching { current.close() }
        node = null
        _status.value = CoreStatus(running = false)
    }

    override fun nodeIdHex(): String? = currentNodeId
    override fun publicIdentityB64(): String? = currentIdentity

    override fun configJson(): String = withNode { it.configJson() } ?: "{}"

    override fun setConfig(patch: String) {
        withNode { it.setConfig(patch) }
    }

    private inline fun <T> withNode(block: (OpenMeshNode) -> T): T? {
        val current = node ?: return null
        return synchronized(lock) {
            try {
                block(current)
            } catch (e: FfiException) {
                _events.tryEmit(CoreEvent.CoreError(e.message ?: "engine error"))
                null
            }
        }
    }

    override fun ingestFrame(frame: ByteArray, nowMs: Long): Result<Unit> = runEngine {
        dispatch(it.ingestFrame(frame, nowMs))
    }

    override fun tick(nowMs: Long): Result<Unit> = runEngine { dispatch(it.tick(nowMs)) }

    override fun sendText(target: Target, text: String, nowMs: Long): Result<Long> = runEngine {
        dispatchAndId(it.sendText(target.toFfi(), text, nowMs))
    }

    override fun sendAttachment(
        target: Target,
        mime: String,
        filename: String?,
        data: ByteArray,
        durationMs: Long?,
        nowMs: Long,
    ): Result<Long> = runEngine {
        dispatchAndId(it.sendAttachment(target.toFfi(), mime, filename, data, durationMs, nowMs))
    }

    override fun sendPosition(
        target: Target,
        lat: Double,
        lon: Double,
        altM: Int?,
        accuracyM: Int?,
        nowMs: Long,
    ): Result<Long> = runEngine {
        dispatchAndId(it.sendPosition(target.toFfi(), lat, lon, altM, accuracyM, nowMs))
    }

    override fun announce(nowMs: Long): Result<Unit> = runEngine { dispatch(it.announce(nowMs)) }

    override fun retryMessage(msgId: Long, nowMs: Long): Result<Unit> = runEngine {
        dispatch(it.retryMessage(msgId, nowMs))
    }

    override fun markRead(msgId: Long, nowMs: Long): Result<Unit> = runEngine {
        dispatch(it.markRead(msgId, nowMs))
    }

    override fun addContact(identityB64: String, name: String) {
        withNode { it.addContact(identityB64, name) }
    }

    override fun removeContact(nodeIdHex: String) {
        withNode { it.removeContact(nodeIdHex) }
    }

    override fun createChannel(name: String): Result<ChannelInvite> = runEngine {
        it.createChannel(name).toInvite(withPsk = true)
    }

    override fun joinChannel(name: String, pskB64: String): Result<ChannelInvite> = runEngine {
        it.joinChannel(name, pskB64).toInvite(withPsk = true)
    }

    override fun channels(): List<ChannelInvite> = withNode { node ->
        node.channels().map { info: FfiChannelInfo ->
            ChannelInvite(info.channelIdHex, info.name, info.inviteUrl)
        }
    } ?: emptyList()

    override fun nodes(): List<NodeInfo> = withNode { node -> node.nodes().map { it.toDomain() } } ?: emptyList()

    override fun stats(): CoreStats = withNode { node ->
        val stats = node.stats()
        CoreStats(
            txFrames = stats.txFrames,
            rxFrames = stats.rxFrames,
            forwarded = stats.forwarded,
            dropped = stats.dropped,
            nodes = stats.nodes,
            channels = stats.channels,
            airtimeMs = stats.airtimeMs,
            effectiveDutyPct = stats.effectiveDutyPct,
        )
    } ?: CoreStats()

    override fun exportState(): ByteArray = withNode { it.exportState() } ?: ByteArray(0)

    override fun importState(state: ByteArray) {
        if (state.isEmpty()) return
        withNode { it.importState(state) }
    }

    private inline fun <T> runEngine(block: (OpenMeshNode) -> T): Result<T> {
        val current = node ?: return Result.failure(IllegalStateException("OpenMesh engine is not running"))
        return synchronized(lock) {
            try {
                Result.success(block(current))
            } catch (e: FfiException) {
                _events.tryEmit(CoreEvent.CoreError(e.message ?: "engine error"))
                Result.failure(e)
            } catch (e: Exception) {
                _events.tryEmit(CoreEvent.CoreError(e.message ?: "engine error"))
                Result.failure(e)
            }
        }
    }

    private fun dispatchAndId(processed: FfiProcessed): Long {
        dispatch(processed)
        val id = processed.msgId ?: 0L
        if (id > 0) msgIdFloor.updateAndGet { maxOf(it, id) }
        return id
    }

    private fun dispatch(processed: FfiProcessed) {
        processed.outbound.forEach { out ->
            val peer = (out.mode as? FfiSendMode.Direct)?.peerHex
            _outbound.tryEmit(OutboundFrame(out.frame, peer))
        }
        processed.events.forEach { event -> mapEvent(event)?.let { _events.tryEmit(it) } }
        val s = runCatching { node?.stats() }.getOrNull()
        if (s != null) {
            _status.value = _status.value.copy(
                running = true,
                nodeIdHex = currentNodeId,
                knownNodes = s.nodes.toInt(),
                txFrames = s.txFrames,
                rxFrames = s.rxFrames,
                error = null,
            )
        }
        if (processed.events.isNotEmpty() || processed.outbound.isNotEmpty()) {
            _status.value = _status.value.copy(running = true)
        }
    }

    private fun mapEvent(event: FfiEvent): CoreEvent? = when (event) {
        is FfiEvent.Message -> {
            val peer = event.channel ?: event.from
            CoreEvent.MessageReceived(
                Message(
                    id = event.msgId,
                    peerId = peer,
                    fromId = event.from,
                    toId = if (event.channel != null) peer else currentNodeId.orEmpty(),
                    kind = MessageKind.TEXT,
                    text = event.text,
                    sentAtMs = event.receivedAtMs,
                    receivedAtMs = event.receivedAtMs,
                    outgoing = false,
                    delivery = DeliveryState.DELIVERED,
                    read = false,
                ),
            )
        }

        is FfiEvent.GroupMessage -> {
            CoreEvent.MessageReceived(
                Message(
                    id = event.msgId,
                    peerId = event.channel,
                    fromId = event.from,
                    toId = event.channel,
                    kind = MessageKind.TEXT,
                    text = event.text,
                    sentAtMs = System.currentTimeMillis(),
                    receivedAtMs = System.currentTimeMillis(),
                    outgoing = false,
                    delivery = DeliveryState.DELIVERED,
                    read = false,
                ),
            )
        }

        is FfiEvent.Ack -> CoreEvent.DeliveryChanged(
            event.msgId,
            when (event.kind) {
                "read" -> DeliveryState.READ
                "delivered", "resource_complete" -> DeliveryState.DELIVERED
                else -> DeliveryState.SENT
            },
        )

        is FfiEvent.NodeSeen -> CoreEvent.NodeSeen(event.node.toDomain())
        is FfiEvent.Telemetry -> CoreEvent.NodeSeen(event.node.toDomain())
        is FfiEvent.Position -> null // merged into NodeSeen by repositories when needed
        is FfiEvent.ResourceProgress -> CoreEvent.ResourceProgress(
            peerId = event.peer,
            messageId = event.msgId,
            received = event.received.toInt(),
            total = event.total.toInt(),
        )

        is FfiEvent.ResourceReceived -> CoreEvent.ResourceReceived(
            peerId = event.peer,
            channelId = event.channel,
            messageId = event.msgId,
            mime = event.mime,
            filename = event.filename,
            data = event.data,
            durationMs = event.durationMs,
        )

        is FfiEvent.AttachmentSent -> CoreEvent.DeliveryChanged(event.msgId, DeliveryState.DELIVERED)
        is FfiEvent.StoreOffer -> CoreEvent.StoreOffer(event.relay, event.count.toInt())
        is FfiEvent.Error -> CoreEvent.CoreError(event.message)
    }

    private fun FfiNode.toDomain(): NodeInfo = NodeInfo(
        nodeId = nodeIdHex,
        name = name.ifBlank { nodeIdHex.take(8) },
        nodeClass = NodeClass.fromWire(`class`),
        caps = caps.map { it.uppercase() }.toSet(),
        hops = hops,
        lastSeenMs = lastSeenMs,
        rssi = rssi,
        snr = snr?.toFloat(),
        batteryPct = batteryPct,
        position = if (lat != null && lon != null) {
            Position(lat = lat!!, lon = lon!!, accuracyM = null, timestampMs = lastSeenMs)
        } else {
            null
        },
    )

    private fun FfiChannel.toInvite(withPsk: Boolean): ChannelInvite = ChannelInvite(
        channelIdHex = channelIdHex,
        name = name,
        inviteUrl = inviteUrl,
        pskB64Once = if (withPsk) pskB64 else null,
    )

    private fun Target.toFfi(): FfiTarget = when (this) {
        is Target.Direct -> FfiTarget.Direct(nodeIdHex)
        is Target.Channel -> FfiTarget.Channel(channelIdHex)
    }
}

/**
 * Demo engine used only when the user turns on demo mode. It keeps the UI state machine honest
 * (queued → sent → delivered) and never claims to have talked to a radio or the Rust core.
 */
class DemoEngineBridge : EngineBridge {
    private val _status = MutableStateFlow(CoreStatus(running = true, nodeIdHex = "a1b2c3d4e5f60718"))
    override val status: StateFlow<CoreStatus> = _status.asStateFlow()
    private val _events = MutableSharedFlow<CoreEvent>(extraBufferCapacity = 32)
    override val events: SharedFlow<CoreEvent> = _events.asSharedFlow()
    private val _outbound = MutableSharedFlow<OutboundFrame>(extraBufferCapacity = 8)
    override val outbound: SharedFlow<OutboundFrame> = _outbound.asSharedFlow()
    private val nextId = AtomicLong(10_000L)

    override suspend fun start(seed: ByteArray, config: EngineConfig) {
        _status.value = CoreStatus(running = true, nodeIdHex = _status.value.nodeIdHex)
    }

    override suspend fun stop() {
        _status.value = CoreStatus(running = false)
    }

    override fun nodeIdHex(): String? = _status.value.nodeIdHex
    override fun publicIdentityB64(): String? = null
    override fun configJson(): String = "{}"
    override fun setConfig(patch: String) = Unit

    override fun ingestFrame(frame: ByteArray, nowMs: Long): Result<Unit> = Result.success(Unit)
    override fun tick(nowMs: Long): Result<Unit> = Result.success(Unit)

    override fun sendText(target: Target, text: String, nowMs: Long): Result<Long> =
        Result.success(nextId.incrementAndGet())

    override fun sendAttachment(
        target: Target,
        mime: String,
        filename: String?,
        data: ByteArray,
        durationMs: Long?,
        nowMs: Long,
    ): Result<Long> = Result.success(nextId.incrementAndGet())

    override fun sendPosition(
        target: Target,
        lat: Double,
        lon: Double,
        altM: Int?,
        accuracyM: Int?,
        nowMs: Long,
    ): Result<Long> = Result.success(nextId.incrementAndGet())

    override fun announce(nowMs: Long): Result<Unit> = Result.success(Unit)
    override fun retryMessage(msgId: Long, nowMs: Long): Result<Unit> = Result.success(Unit)
    override fun markRead(msgId: Long, nowMs: Long): Result<Unit> = Result.success(Unit)
    override fun addContact(identityB64: String, name: String) = Unit
    override fun removeContact(nodeIdHex: String) = Unit
    override fun createChannel(name: String): Result<ChannelInvite> =
        Result.success(ChannelInvite("ff00ff00ff00ff00", name, "openmesh://channel?name=$name&id=ff00ff00ff00ff00&psk=demo", "demo"))

    override fun joinChannel(name: String, pskB64: String): Result<ChannelInvite> = createChannel(name)
    override fun channels(): List<ChannelInvite> = emptyList()
    override fun nodes(): List<NodeInfo> = emptyList()
    override fun stats(): CoreStats = CoreStats()
    override fun exportState(): ByteArray = ByteArray(0)
    override fun importState(state: ByteArray) = Unit
    override fun isDemo(): Boolean = true
}
