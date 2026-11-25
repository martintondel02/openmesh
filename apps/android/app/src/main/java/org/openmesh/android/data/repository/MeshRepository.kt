package org.openmesh.android.data.repository

import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import org.openmesh.android.data.engine.CoreEvent
import org.openmesh.android.data.engine.DemoEngineBridge
import org.openmesh.android.data.engine.EngineBridge
import org.openmesh.android.data.model.AppSettings
import org.openmesh.android.data.model.Channel
import org.openmesh.android.data.model.Contact
import org.openmesh.android.data.model.Conversation
import org.openmesh.android.data.model.DeliveryState
import org.openmesh.android.data.model.DemoData
import org.openmesh.android.data.model.Message
import org.openmesh.android.data.model.MessageKind
import org.openmesh.android.data.model.NodeInfo
import org.openmesh.android.data.model.NodeClass
import org.openmesh.android.data.model.RadioStatus
import org.openmesh.android.data.transport.BleTransport
import org.openmesh.android.data.transport.CompanionTransport
import org.openmesh.android.data.transport.FramedCompanionClient
import org.openmesh.android.data.transport.UsbSerialTransport
import org.openmesh.android.util.LoraProfile
import org.openmesh.android.util.Region
import org.openmesh.android.util.estimateResourceTransfer
import org.openmesh.android.util.wallClockMs

enum class CompanionKind { NONE, USB, BLE }

sealed interface UiEvent {
    data class Snackbar(val text: String) : UiEvent
    data class InviteImported(val label: String, val channel: Boolean) : UiEvent
    data class Error(val text: String) : UiEvent
}

/**
 * Single source of truth for the Android UI.
 *
 * Responsibilities:
 *  * choose demo data vs a connected radio / future FFI core,
 *  * own the [FramedCompanionClient] lifecycle and translate companion events into state,
 *  * expose StateFlows the Compose screens collect,
 *  * provide the honest airtime estimate shown before sending media
 *    (docs/architecture/PROTOCOL.md §9.4, docs/research/lora-capacity-and-media.md).
 *
 * Threading: methods are safe to call from the main thread; IO happens on [scope].
 */
class MeshRepository(
    private val context: Context,
    private val engine: EngineBridge = DemoEngineBridge(),
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    private val _settings = MutableStateFlow(DemoData.settings)
    val settings: StateFlow<AppSettings> = _settings.asStateFlow()

    private val _nodes = MutableStateFlow(DemoData.nodes)
    val nodes: StateFlow<List<NodeInfo>> = _nodes.asStateFlow()

    private val _contacts = MutableStateFlow(DemoData.contacts)
    val contacts: StateFlow<List<Contact>> = _contacts.asStateFlow()

    private val _channels = MutableStateFlow(DemoData.channels)
    val channels: StateFlow<List<Channel>> = _channels.asStateFlow()

    private val _messages = MutableStateFlow(DemoData.messages)
    val messages: StateFlow<Map<String, List<Message>>> = _messages.asStateFlow()

    private val _channelMessages = MutableStateFlow(DemoData.channelMessages)
    val channelMessages: StateFlow<Map<String, List<Message>>> = _channelMessages.asStateFlow()

    private val _conversations = MutableStateFlow(buildConversations(DemoData.messages, DemoData.contacts))
    val conversations: StateFlow<List<Conversation>> = _conversations.asStateFlow()

    private val _radio = MutableStateFlow(DemoData.radioStatus)
    val radio: StateFlow<RadioStatus> = _radio.asStateFlow()

    private val _engineStatus = engine.status
    val engineStatus: StateFlow<org.openmesh.android.data.engine.CoreStatus> = _engineStatus

    private val _uiEvents = MutableSharedFlow<UiEvent>(extraBufferCapacity = 16)
    val uiEvents: SharedFlow<UiEvent> = _uiEvents.asSharedFlow()

    private var transport: CompanionTransport? = null
    private var companion: FramedCompanionClient? = null
    private var companionKind = CompanionKind.NONE
    private var companionJob: Job? = null

    init {
        scope.launch { engine.start() }
        scope.launch {
            engine.events.collect { event -> handleCoreEvent(event) }
        }
    }

    // ------------------------------------------------------------------ messages

    fun messageThread(peerId: String): List<Message> =
        _messages.value[peerId] ?: _channelMessages.value[peerId] ?: emptyList()

    fun contactFor(nodeId: String): Contact? = _contacts.value.firstOrNull { it.nodeId == nodeId }

    fun nodeFor(nodeId: String): NodeInfo? = _nodes.value.firstOrNull { it.nodeId == nodeId }

    fun displayNameFor(nodeId: String): String =
        contactFor(nodeId)?.name ?: nodeFor(nodeId)?.name ?: nodeId.take(8)

    /** Send a text into a direct conversation. Returns the local message id. */
    fun sendText(peerId: String, text: String, channel: Boolean = false): Long {
        val trimmed = text.trim()
        if (trimmed.isEmpty()) return -1L
        val id = System.currentTimeMillis()
        val message = Message(
            id = id,
            peerId = peerId,
            fromId = engine.selfNodeId() ?: "demo",
            toId = peerId,
            kind = MessageKind.TEXT,
            text = trimmed,
            sentAtMs = id,
            outgoing = true,
            delivery = DeliveryState.QUEUED,
        )
        appendMessage(peerId, message, channel)
        if (_radio.value.connected && companion != null) {
            // The companion transport is live, but building a valid OMP frame needs the Rust
            // core (UniFFI). Until that lands, be explicit instead of pretending to send.
            markDelivery(peerId, id, DeliveryState.FAILED, channel)
            _uiEvents.tryEmit(
                UiEvent.Error(
                    "Radio connected, but the OpenMesh core is not linked in this build; " +
                        "see apps/android/README.md (M2).",
                ),
            )
        } else {
            // Demo/direct mode: simulate an ack so the UI state machine is exercised.
            scope.launch {
                delay(900)
                markDelivery(peerId, id, DeliveryState.SENT, channel)
                delay(1200)
                markDelivery(peerId, id, DeliveryState.DELIVERED, channel)
            }
        }
        return id
    }

    /**
     * Queue an image attachment. The real client compresses locally to [mediaCapBytes] and
     * shows [estimateMedia] before sending; the demo path records the chosen bytes and
     * simulates the resource transfer so the progress UI is exercised.
     */
    fun sendAttachment(
        peerId: String,
        name: String,
        mime: String,
        sizeBytes: Int,
        channel: Boolean = false,
    ): Long {
        val id = System.currentTimeMillis()
        val message = Message(
            id = id,
            peerId = peerId,
            fromId = engine.selfNodeId() ?: "demo",
            toId = peerId,
            kind = MessageKind.ATTACHMENT,
            text = name,
            sentAtMs = id,
            outgoing = true,
            delivery = DeliveryState.QUEUED,
            attachment = org.openmesh.android.data.model.Attachment(
                id = "att_${id.toString(16)}",
                mime = mime,
                sizeBytes = sizeBytes,
                name = name,
                progress = 1f,
            ),
        )
        appendMessage(peerId, message, channel)
        scope.launch {
            delay(600)
            markDelivery(peerId, id, DeliveryState.SENT, channel)
            delay(1_500)
            markDelivery(peerId, id, DeliveryState.DELIVERED, channel)
        }
        return id
    }

    /** Queue a voice note (Opus/Ogg, recorded by the app in M2; simulated in the scaffold). */
    fun sendVoiceNote(peerId: String, durationMs: Int, bytes: Int, channel: Boolean = false): Long {
        val id = System.currentTimeMillis()
        val message = Message(
            id = id,
            peerId = peerId,
            fromId = engine.selfNodeId() ?: "demo",
            toId = peerId,
            kind = MessageKind.ATTACHMENT,
            text = "Voice note",
            sentAtMs = id,
            outgoing = true,
            delivery = DeliveryState.QUEUED,
            attachment = org.openmesh.android.data.model.Attachment(
                id = "att_${id.toString(16)}",
                mime = "audio/ogg; codecs=opus",
                sizeBytes = bytes,
                durationMs = durationMs,
                name = "voice-${durationMs / 1000}s.ogg",
                progress = 1f,
            ),
        )
        appendMessage(peerId, message, channel)
        scope.launch {
            delay(600)
            markDelivery(peerId, id, DeliveryState.SENT, channel)
            delay(1_500)
            markDelivery(peerId, id, DeliveryState.DELIVERED, channel)
        }
        return id
    }

    private fun appendMessage(peerId: String, message: Message, channel: Boolean) {
        if (channel) {
            val existing = _channelMessages.value[peerId].orEmpty()
            _channelMessages.value = _channelMessages.value + (peerId to (existing + message))
            val updatedChannels = _channels.value.map {
                if (it.id == peerId) it.copy(unreadCount = 0) else it
            }
            _channels.value = updatedChannels
        } else {
            val existing = _messages.value[peerId].orEmpty()
            _messages.value = _messages.value + (peerId to (existing + message))
            _conversations.value = buildConversations(_messages.value, _contacts.value)
        }
    }

    private fun markDelivery(peerId: String, messageId: Long, state: DeliveryState, channel: Boolean) {
        if (channel) {
            _channelMessages.value = updateDelivery(_channelMessages.value, peerId, messageId, state)
        } else {
            _messages.value = updateDelivery(_messages.value, peerId, messageId, state)
            _conversations.value = buildConversations(_messages.value, _contacts.value)
        }
    }

    private fun updateDelivery(
        map: Map<String, List<Message>>,
        peerId: String,
        messageId: Long,
        state: DeliveryState,
    ): Map<String, List<Message>> {
        val thread = map[peerId] ?: return map
        return map + (peerId to thread.map { if (it.id == messageId) it.copy(delivery = state) else it })
    }

    // ------------------------------------------------------------------ companion link

    fun connectCompanion(kind: CompanionKind) {
        if (companionKind == kind && _radio.value.connected) return
        disconnectCompanion()
        val newTransport: CompanionTransport = when (kind) {
            CompanionKind.USB -> UsbSerialTransport(context)
            CompanionKind.BLE -> BleTransport(context)
            CompanionKind.NONE -> return
        }
        val client = FramedCompanionClient(newTransport)
        transport = newTransport
        companion = client
        companionKind = kind
        client.start(scope)
        companionJob = scope.launch {
            val result = newTransport.connect()
            if (result.isFailure) {
                _radio.value = _radio.value.copy(connected = false, transportName = newTransport.label)
                _uiEvents.tryEmit(UiEvent.Error(result.exceptionOrNull()?.message ?: "Could not connect to radio"))
                return@launch
            }
            _radio.value = _radio.value.copy(connected = true, transportName = newTransport.label)
            // Push the persisted radio settings to the device.
            client.sendConfigSet("radio.profile", _settings.value.profile)
            client.sendConfigSet("radio.region", _settings.value.region)
            client.sendConfigSet("node.name", _settings.value.nodeName)
            client.sendConfigSet("mesh.store", if (_settings.value.storeForward) "1" else "0")
            client.events.collect { event ->
                when (event) {
                    is FramedCompanionClient.CompanionEvent.HelloReceived -> {
                        _radio.value = _radio.value.copy(
                            firmwareVersion = event.hello.firmwareVersion,
                            nodeId = event.hello.nodeIdHex,
                            region = event.hello.region,
                            profile = event.hello.profile,
                        )
                        _uiEvents.tryEmit(UiEvent.Snackbar("Radio ready: ${event.hello.firmwareVersion}"))
                    }
                    is FramedCompanionClient.CompanionEvent.Rx -> {
                        scope.launch { engine.ingest(event.frame, event.rssi, event.snr.toFloat()) }
                    }
                    is FramedCompanionClient.CompanionEvent.Stats -> {
                        _radio.value = _radio.value.copy(
                            txQueue = event.entries["tx_queue"]?.toInt() ?: _radio.value.txQueue,
                            airtimePct = event.entries["airtime_used_pct"]?.toFloat() ?: _radio.value.airtimePct,
                            rssi = event.entries["last_rssi"]?.toInt() ?: _radio.value.rssi,
                        )
                    }
                    is FramedCompanionClient.CompanionEvent.Config -> {
                        val entries = event.entries
                        _settings.value = _settings.value.copy(
                            profile = entries["radio.profile"] ?: _settings.value.profile,
                            region = entries["radio.region"] ?: _settings.value.region,
                            nodeName = entries["node.name"] ?: _settings.value.nodeName,
                            storeForward = (entries["mesh.store"] ?: "1") == "1",
                        )
                    }
                    is FramedCompanionClient.CompanionEvent.RadioEvent -> {
                        _uiEvents.tryEmit(UiEvent.Snackbar("Radio: ${event.kind} ${event.detail}"))
                    }
                    is FramedCompanionClient.CompanionEvent.Pong -> Unit
                    is FramedCompanionClient.CompanionEvent.Failure -> {
                        _uiEvents.tryEmit(UiEvent.Error(event.message))
                    }
                }
            }
        }
    }

    fun disconnectCompanion() {
        companionJob?.cancel()
        companionJob = null
        val client = companion
        val t = transport
        companion = null
        transport = null
        companionKind = CompanionKind.NONE
        scope.launch {
            runCatching { client?.stop() }
            runCatching { t?.disconnect() }
        }
        _radio.value = _radio.value.copy(connected = false)
    }

    val companionConnected: StateFlow<RadioStatus> = _radio

    fun currentCompanionKind(): CompanionKind = companionKind

    // ------------------------------------------------------------------ contacts / invites

    fun importInvite(uri: String): Result<QrInvite> = runCatching {
        val invite = QrInvite.parse(uri)
        when (invite.kind) {
            QrInvite.Kind.CONTACT -> {
                val contact = Contact(
                    nodeId = invite.nodeId,
                    name = invite.name,
                    identityBase64 = invite.identityBase64,
                    lastSeenMs = System.currentTimeMillis(),
                    verified = true,
                )
                if (_contacts.value.none { it.nodeId == contact.nodeId }) {
                    _contacts.value = _contacts.value + contact
                }
                _nodes.value = _nodes.value.filterNot { it.nodeId == contact.nodeId } + NodeInfo(
                    nodeId = contact.nodeId,
                    name = contact.name,
                    nodeClass = NodeClass.CLIENT,
                    caps = emptySet(),
                    lastSeenMs = contact.lastSeenMs,
                    isContact = true,
                )
                _conversations.value = buildConversations(_messages.value, _contacts.value)
            }
            QrInvite.Kind.CHANNEL -> {
                val channel = Channel(
                    id = invite.nodeId,
                    name = invite.name,
                    pskSet = invite.pskBase64.isNotEmpty(),
                )
                if (_channels.value.none { it.id == channel.id }) {
                    _channels.value = _channels.value + channel
                }
            }
        }
        _uiEvents.tryEmit(UiEvent.InviteImported(invite.name, invite.kind == QrInvite.Kind.CHANNEL))
        invite
    }

    fun removeContact(nodeId: String) {
        _contacts.value = _contacts.value.filterNot { it.nodeId == nodeId }
        _nodes.value = _nodes.value.map { if (it.nodeId == nodeId) it.copy(isContact = false) else it }
    }

    // ------------------------------------------------------------------ settings

    fun updateSettings(transform: (AppSettings) -> AppSettings) {
        val previous = _settings.value
        val updated = transform(previous)
        _settings.value = updated
        val radioChanged = updated.profile != previous.profile || updated.region != previous.region
        if (radioChanged) {
            companion?.let { client ->
                scope.launch {
                    runCatching { client.sendConfigSet("radio.profile", updated.profile) }
                    runCatching { client.sendConfigSet("radio.region", updated.region) }
                }
            }
        }
    }

    fun setDemoMode(enabled: Boolean) = updateSettings { it.copy(demoMode = enabled) }

    // ------------------------------------------------------------------ airtime UX

    fun estimateMedia(bytes: Int, kind: MediaKind = MediaKind.IMAGE): MediaEstimate {
        val profile = LoraProfile.fromName(_settings.value.profile)
        val region = Region.fromName(_settings.value.region)
        val transfer = estimateResourceTransfer(profile, bytes)
        val wall = wallClockMs(transfer.airtimeMs, region.dutyCyclePct)
        // Duty-cycle budget in milliseconds of airtime per hour: 1 % = 36 000 ms.
        val hourlyBudgetMs = region.dutyCyclePct?.let { it * 36_000f }
        val ratio = if (hourlyBudgetMs == null || hourlyBudgetMs <= 0f) {
            0f
        } else {
            transfer.airtimeMs.toFloat() / hourlyBudgetMs
        }
        return MediaEstimate(
            kind = kind,
            bytes = bytes,
            profile = profile,
            region = region,
            airtimeMs = transfer.airtimeMs,
            wallClockMs = wall,
            fractionOfHourlyBudget = ratio,
            chunks = (bytes + 179) / 180,
        )
    }

    fun mediaCapBytes(): Int = when (_settings.value.profile) {
        "SHORT_TURBO" -> 24 * 1024
        "SHORT_FAST" -> 24 * 1024
        "MEDIUM_FAST" -> 12 * 1024
        "LONG_FAST" -> 6 * 1024
        "LONG_SLOW" -> 1536
        else -> 12 * 1024
    }

    // ------------------------------------------------------------------ internals

    private fun handleCoreEvent(event: CoreEvent) {
        when (event) {
            is CoreEvent.MessageReceived -> {
                appendMessage(event.message.peerId, event.message, channel = false)
                _conversations.value = buildConversations(_messages.value, _contacts.value)
            }
            is CoreEvent.DeliveryChanged -> {
                val updated = _messages.value.mapValues { (_, thread) ->
                    thread.map { if (it.id == event.messageId) it.copy(delivery = event.state) else it }
                }
                _messages.value = updated
                _conversations.value = buildConversations(updated, _contacts.value)
            }
            is CoreEvent.NodeSeen -> {
                _nodes.value = _nodes.value.filterNot { it.nodeId == event.node.nodeId } + event.node
            }
            is CoreEvent.ResourceProgress -> Unit
            is CoreEvent.CoreError -> _uiEvents.tryEmit(UiEvent.Snackbar(event.message))
        }
    }

    private fun buildConversations(
        messages: Map<String, List<Message>>,
        contacts: List<Contact>,
    ): List<Conversation> = messages.map { (peerId, thread) ->
        val contact = contacts.firstOrNull { it.nodeId == peerId }
        Conversation(
            peerId = peerId,
            peerName = contact?.name ?: peerId.take(8),
            lastMessage = thread.lastOrNull(),
            unreadCount = thread.count { !it.outgoing && !it.read },
            online = false,
            contact = contact != null,
        )
    }.sortedByDescending { it.lastMessage?.sentAtMs ?: 0L }

    fun stop() {
        disconnectCompanion()
        scope.launch { engine.stop() }
    }
}

enum class MediaKind { IMAGE, VOICE }

data class MediaEstimate(
    val kind: MediaKind,
    val bytes: Int,
    val profile: LoraProfile,
    val region: Region,
    val airtimeMs: Int,
    val wallClockMs: Long,
    val fractionOfHourlyBudget: Float,
    val chunks: Int,
) {
    val airtimeSeconds: Double get() = airtimeMs / 1000.0
    val wallClockSeconds: Long get() = wallClockMs / 1000
}

/** Process-wide graph; the Application initialises it, screens/services read it. */
object MeshGraph {
    @Volatile
    private var repository: MeshRepository? = null

    fun initialize(context: Context) {
        if (repository == null) {
            synchronized(this) {
                if (repository == null) {
                    repository = MeshRepository(context.applicationContext)
                }
            }
        }
    }

    fun repository(): MeshRepository = requireNotNull(repository) { "MeshGraph not initialized" }

    /** Intent action used by MainActivity's USB attach filter. */
    const val ACTION_USB_PERMISSION = "org.openmesh.android.USB_PERMISSION"
}
