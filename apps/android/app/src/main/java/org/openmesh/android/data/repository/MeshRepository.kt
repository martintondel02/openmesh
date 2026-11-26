package org.openmesh.android.data.repository

import android.content.Context
import android.util.Base64
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
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import org.openmesh.android.data.db.ChannelEntity
import org.openmesh.android.data.db.ContactEntity
import org.openmesh.android.data.db.MessageEntity
import org.openmesh.android.data.db.NodeEntity
import org.openmesh.android.data.db.OpenMeshDatabase
import org.openmesh.android.data.engine.ChannelInvite
import org.openmesh.android.data.engine.CoreEvent
import org.openmesh.android.data.engine.CoreStats
import org.openmesh.android.data.engine.CoreStatus
import org.openmesh.android.data.engine.DemoEngineBridge
import org.openmesh.android.data.engine.EngineBridge
import org.openmesh.android.data.engine.EngineConfig
import org.openmesh.android.data.engine.Target
import org.openmesh.android.data.engine.UniFfiEngineBridge
import org.openmesh.android.data.help.HelpRepository
import org.openmesh.android.data.help.HelpTopicMeta
import org.openmesh.android.data.model.AppSettings
import org.openmesh.android.data.model.Attachment
import org.openmesh.android.data.model.Channel
import org.openmesh.android.data.model.Contact
import org.openmesh.android.data.model.Conversation
import org.openmesh.android.data.model.DeliveryState
import org.openmesh.android.data.model.DemoData
import org.openmesh.android.data.model.Message
import org.openmesh.android.data.model.MessageKind
import org.openmesh.android.data.model.NodeClass
import org.openmesh.android.data.model.NodeInfo
import org.openmesh.android.data.model.Position
import org.openmesh.android.data.model.RadioStatus
import org.openmesh.android.data.notify.Notifications
import org.openmesh.android.data.prefs.AppPreferences
import org.openmesh.android.data.prefs.UiMode
import org.openmesh.android.data.presets.AppliedPreset
import org.openmesh.android.data.presets.BeginnerAnswers
import org.openmesh.android.data.presets.BeginnerPreset
import org.openmesh.android.data.presets.PresetLoader
import org.openmesh.android.data.regulatory.RegionLimits
import org.openmesh.android.data.regulatory.RegulatoryMapper
import org.openmesh.android.data.regulatory.RegulatoryOverride
import org.openmesh.android.data.security.IdentityException
import org.openmesh.android.data.security.IdentityStore
import org.openmesh.android.data.transport.BleTransport
import org.openmesh.android.data.transport.CompanionTransport
import org.openmesh.android.data.transport.FramedCompanionClient
import org.openmesh.android.data.transport.UsbSerialTransport
import org.openmesh.android.media.MediaMath
import org.openmesh.android.media.MediaTierLoader
import org.openmesh.android.media.MediaTierPolicy
import org.openmesh.android.media.MediaTierPresets
import org.openmesh.android.util.LoraProfile
import org.openmesh.android.util.MeshLog
import org.openmesh.android.util.Region
import org.openmesh.android.util.estimateResourceTransfer
import org.openmesh.android.util.wallClockMs
import java.io.File

enum class CompanionKind { NONE, USB, BLE }

sealed interface UiEvent {
    data class Snackbar(val text: String) : UiEvent
    data class InviteImported(val label: String, val channel: Boolean) : UiEvent
    data class Error(val text: String) : UiEvent
}

/**
 * Single source of truth for the Android UI and foreground service.
 *
 * Owns the Rust engine (via [EngineBridge]), the companion radio link, persistence (Room +
 * DataStore) and the 250 ms engine loop. The UI only reads StateFlows and calls intent-style
 * methods; it never touches the generated UniFFI classes.
 */
class MeshRepository(
    context: Context,
    private val prefs: AppPreferences = AppPreferences(context),
    private val database: OpenMeshDatabase = OpenMeshDatabase.get(context),
    private val help: HelpRepository = HelpRepository(context),
    private val identityStore: IdentityStore = IdentityStore(context),
) {
    private val appContext = context.applicationContext
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val dao = database.dao()

    // ---------------------------------------------------------------- observable state
    private val _settings = MutableStateFlow(AppSettings())
    val settings: StateFlow<AppSettings> = _settings.asStateFlow()

    private val _nodes = MutableStateFlow<List<NodeInfo>>(emptyList())
    val nodes: StateFlow<List<NodeInfo>> = _nodes.asStateFlow()

    private val _contacts = MutableStateFlow<List<Contact>>(emptyList())
    val contacts: StateFlow<List<Contact>> = _contacts.asStateFlow()

    private val _channels = MutableStateFlow<List<Channel>>(emptyList())
    val channels: StateFlow<List<Channel>> = _channels.asStateFlow()

    private val _messages = MutableStateFlow<Map<String, List<Message>>>(emptyMap())
    val messages: StateFlow<Map<String, List<Message>>> = _messages.asStateFlow()

    private val _channelMessages = MutableStateFlow<Map<String, List<Message>>>(emptyMap())
    val channelMessages: StateFlow<Map<String, List<Message>>> = _channelMessages.asStateFlow()

    private val _conversations = MutableStateFlow<List<Conversation>>(emptyList())
    val conversations: StateFlow<List<Conversation>> = _conversations.asStateFlow()

    private val _radio = MutableStateFlow(RadioStatus(connected = false, transportName = "none"))
    val radio: StateFlow<RadioStatus> = _radio.asStateFlow()

    private val _engineStatus = MutableStateFlow(CoreStatus())
    val engineStatus: StateFlow<CoreStatus> = _engineStatus.asStateFlow()

    private val _stats = MutableStateFlow(CoreStats())
    val stats: StateFlow<CoreStats> = _stats.asStateFlow()

    private val _selfNodeId = MutableStateFlow<String?>(null)
    val selfNodeId: StateFlow<String?> = _selfNodeId.asStateFlow()

    private val _selfIdentity = MutableStateFlow<String?>(null)
    val selfIdentity: StateFlow<String?> = _selfIdentity.asStateFlow()

    private val _uiEvents = MutableSharedFlow<UiEvent>(extraBufferCapacity = 32)
    val uiEvents: SharedFlow<UiEvent> = _uiEvents.asSharedFlow()

    val helpTopics: List<HelpTopicMeta> get() = help.topics

    /** Media tier presets from the shared content/presets/media.json asset (cached). */
    val mediaTiers: MediaTierPresets by lazy {
        runCatching { MediaTierLoader.fromAssets(appContext) }
            .getOrElse {
                MeshLog.w("media tiers asset missing; falling back to repo file: ${it.message}")
                MediaTierLoader.fromRepoFile()
            }
    }

    fun defaultImageTier() = _settings.value.defaultImageTier
    fun defaultVoiceTier() = _settings.value.defaultVoiceTier
    fun suggestImageTier(capBytes: Int = mediaCapBytes()): String = MediaTierPolicy.suggestImageTier(mediaTiers, capBytes)
    fun originalImageAllowed(): Boolean = MediaTierPolicy.originalAllowed(_settings.value.profile)
    fun voiceTier(id: String = defaultVoiceTier()): org.openmesh.android.media.VoiceTier =
        MediaTierPolicy.voiceTier(mediaTiers, id)
    fun effectiveVoiceMaxSeconds(voiceTierId: String): Int =
        MediaTierPolicy.effectiveVoiceMaxSeconds(voiceTier(voiceTierId), mediaCapBytes())
    fun setDefaultImageTier(id: String) = updateSettings { it.copy(defaultImageTier = id) }
    fun setDefaultVoiceTier(id: String) = updateSettings { it.copy(defaultVoiceTier = id) }

    fun helpTopic(slug: String): org.openmesh.android.data.help.HelpTopic? = help.topic(slug)

    // ---------------------------------------------------------------- internals
    private val _lastChannelInvite = MutableStateFlow<String?>(null)
    val lastChannelInvite: StateFlow<String?> = _lastChannelInvite.asStateFlow()

    private var engine: EngineBridge = DemoEngineBridge()
    private var engineJobs: MutableList<Job> = mutableListOf()
    private var tickJob: Job? = null
    private var startJob: Job? = null
    private var started = false
    private var startedLock = Any()

    private val _engineReady = MutableStateFlow(false)
    val engineReady: StateFlow<Boolean> = _engineReady.asStateFlow()

    private val _initialized = MutableStateFlow(false)
    /** True once DataStore/Room state has been loaded (MainActivity waits for this). */
    val initialized: StateFlow<Boolean> = _initialized.asStateFlow()

    private var transport: CompanionTransport? = null
    private var companion: FramedCompanionClient? = null
    private var companionKind = CompanionKind.NONE
    private var companionJob: Job? = null
    @Volatile private var latestPrefs = org.openmesh.android.data.prefs.LocalPrefs()

    init {
        scope.launch { load() }
    }

    private suspend fun load() {
        val saved = prefs.current()
        latestPrefs = saved
        _settings.value = saved.toSettings()
        loadFromDatabase()
        if (saved.demoMode) {
            seedDemo()
        } else if (saved.setupCompleted) {
            ensureStarted()
        }
        if (saved.autoConnect && saved.radioKind != "none" && saved.setupCompleted) {
            connectCompanion(if (saved.radioKind == "usb") CompanionKind.USB else CompanionKind.BLE)
        }
        _initialized.value = true
        MeshLog.i("repository loaded: setup_completed=${saved.setupCompleted} mode=${saved.uiMode.wire} radio=${saved.radioKind}")
    }

    private suspend fun loadFromDatabase() {
        val channels = dao.channels().map { Channel(it.id, it.name, pskSet = it.pskSet) }
        val channelIds = channels.map { it.id }.toSet()
        val all = dao.messages().map { it.toModel() }
        val direct = all.filter { it.peerId !in channelIds }
        val channelMsgs = all.filter { it.peerId in channelIds }
        _channels.value = channels
        _contacts.value = dao.contacts().map { it.toModel() }
        _nodes.value = dao.nodes().map { it.toModel() }
        _messages.value = direct.groupBy { it.peerId }.mapValues { (_, list) -> list.sortedBy { it.sentAtMs } }
        _channelMessages.value = channelMsgs.groupBy { it.peerId }.mapValues { (_, list) -> list.sortedBy { it.sentAtMs } }
        rebuildConversations()
    }

    private fun seedDemo() {
        _settings.value = _settings.value.copy(demoMode = true)
        _nodes.value = DemoData.nodes
        _contacts.value = DemoData.contacts
        _channels.value = DemoData.channels
        _messages.value = DemoData.messages
        _channelMessages.value = DemoData.channelMessages
        _radio.value = DemoData.radioStatus
        rebuildConversations()
    }

    // ---------------------------------------------------------------- lifecycle / engine

    /** Idempotent: starts the engine + tick loop in the background (UI/service entry point). */
    fun ensureStarted() {
        scope.launch { ensureStartedAwait() }
    }

    /**
     * Starts the engine and suspends until the Rust core has actually been constructed and its
     * state imported. JNA + UniFFI init can take seconds on first use, so callers must await this
     * before configuring/creating channels. Returns false on timeout/failure.
     */
    suspend fun ensureStartedAwait(timeoutMs: Long = 20_000): Boolean {
        if (_engineReady.value && engine.status.value.running) return true
        val job = synchronized(startedLock) {
            started = true
            if (startJob?.isActive != true) {
                startJob = scope.launch { startEngine() }
            }
            startJob!!
        }
        return kotlinx.coroutines.withTimeoutOrNull(timeoutMs) {
            job.join()
            _engineReady.value && engine.status.value.running
        } ?: false.also {
            MeshLog.w("engine start timed out after ${timeoutMs}ms")
            _uiEvents.tryEmit(UiEvent.Error("The protocol engine did not start in time. Try again, or check logcat tag OpenMesh."))
        }
    }

    private suspend fun startEngine() {
        _engineReady.value = false
        MeshLog.i("engine start: begin")
        val saved = prefs.current()
        latestPrefs = saved
        val seed = try {
            identityStore.loadOrCreateSeed(prefs)
        } catch (e: IdentityException) {
            MeshLog.e("engine start: identity error", e)
            _uiEvents.tryEmit(UiEvent.Error(e.message ?: "identity error"))
            synchronized(startedLock) { started = false }
            return
        }
        engineJobs.forEach { it.cancel() }
        engineJobs = mutableListOf()
        runCatching { engine.stop() }
        engine = if (saved.demoMode) DemoEngineBridge() else UniFfiEngineBridge()
        val config = EngineConfig(
            name = saved.nodeName,
            nodeClass = saved.role,
            profile = saved.profile,
            region = saved.region,
            storeForward = saved.storeForward,
            acceptChannels = true,
            announceIntervalS = saved.announceIntervalS,
        )
        engine.start(seed, config)
        val state = prefs.engineState()
        if (state != null && state.isNotEmpty()) runCatching { engine.importState(state) }
        _selfNodeId.value = engine.nodeIdHex()
        _selfIdentity.value = engine.publicIdentityB64()
        _engineStatus.value = engine.status.value
        engineJobs += scope.launch { engine.events.collect { handleCoreEvent(it) } }
        engineJobs += scope.launch { engine.outbound.collect { out -> companion?.let { c -> runCatching { c.sendFrame(out.frame) } } } }
        engineJobs += scope.launch { engine.status.collect { _engineStatus.value = it } }
        tickJob?.cancel()
        tickJob = scope.launch {
            while (isActive) {
                runCatching { engine.tick(System.currentTimeMillis()) }
                if (engine.status.value.running) {
                    _stats.value = runCatching { engine.stats() }.getOrDefault(CoreStats())
                }
                delay(250)
            }
        }
        runCatching { engine.announce(System.currentTimeMillis()) }
        if (saved.demoMode) seedDemo()
        _engineReady.value = true
        MeshLog.i("engine start: ready node=${engine.nodeIdHex() ?: "?"} name=${saved.nodeName} profile=${saved.profile}/${saved.region}")
    }

    /** Persist engine state and stop timers (called when the user stops the mesh). */
    fun stop() {
        MeshLog.i("engine stop requested")
        synchronized(startedLock) { started = false }
        _engineReady.value = false
        tickJob?.cancel()
        tickJob = null
        val export = runCatching { engine.exportState() }.getOrNull()
        if (export != null && export.isNotEmpty()) scope.launch { prefs.setEngineState(export) }
        engineJobs.forEach { it.cancel() }
        engineJobs = mutableListOf()
        disconnectCompanion()
        scope.launch { runCatching { engine.stop() } }
    }

    fun selfNodeId(): String? = _selfNodeId.value

    /** Contact invite for your own node (QR/paste). Null until the engine has started. */
    fun selfContactInvite(): String? {
        val id = _selfNodeId.value ?: return null
        val identity = _selfIdentity.value ?: return null
        return QrInvite.contact(id, _settings.value.nodeName, identity)
    }

    // ---------------------------------------------------------------- messaging

    fun messageThread(peerId: String): List<Message> =
        _messages.value[peerId] ?: _channelMessages.value[peerId] ?: emptyList()

    fun contactFor(nodeId: String): Contact? = _contacts.value.firstOrNull { it.nodeId == nodeId }

    fun nodeFor(nodeId: String): NodeInfo? = _nodes.value.firstOrNull { it.nodeId == nodeId }

    fun displayNameFor(nodeId: String): String =
        _channels.value.firstOrNull { it.id == nodeId }?.name
            ?: contactFor(nodeId)?.name
            ?: nodeFor(nodeId)?.name
            ?: nodeId.take(8)

    fun sendText(peerId: String, text: String, channel: Boolean = false): Long {
        val trimmed = text.trim()
        if (trimmed.isEmpty()) return -1L
        val target = if (channel) Target.Channel(peerId) else Target.Direct(peerId)
        val result = engine.sendText(target, trimmed)
        val id = result.getOrElse {
            _uiEvents.tryEmit(UiEvent.Error(it.message ?: "send failed"))
            System.currentTimeMillis()
        }
        val message = Message(
            id = id,
            peerId = peerId,
            fromId = _selfNodeId.value ?: "self",
            toId = peerId,
            kind = MessageKind.TEXT,
            text = trimmed,
            sentAtMs = System.currentTimeMillis(),
            outgoing = true,
            delivery = if (result.isSuccess) DeliveryState.SENT else DeliveryState.FAILED,
        )
        appendMessage(peerId, message, channel)
        scope.launch { dao.upsertMessage(MessageEntity.fromModel(message)) }
        if (engine.isDemo()) simulateAck(peerId, id, channel)
        return id
    }

    fun sendAttachment(
        peerId: String,
        name: String,
        mime: String,
        sizeBytes: Int,
        channel: Boolean = false,
    ): Long {
        // Kept for the scaffold's demo voice/image dialog: synthesize deterministic bytes only in
        // demo mode; real media uses [sendAttachmentBytes] with compressed/recorded data.
        val bytes = ByteArray(sizeBytes.coerceAtLeast(1)) { 0 }
        return sendAttachmentBytes(peerId, name, mime, bytes, null, channel)
    }

    fun sendVoiceNote(peerId: String, durationMs: Int, bytes: Int, channel: Boolean = false): Long =
        sendAttachment(peerId, "voice-${durationMs / 1000}s.m4a", "audio/mp4", bytes, channel)

    /** Send real media bytes (compressed image, recorded AAC voice note) through the engine. */
    fun sendAttachmentBytes(
        peerId: String,
        name: String,
        mime: String,
        data: ByteArray,
        durationMs: Long? = null,
        channel: Boolean = false,
    ): Long {
        val target = if (channel) Target.Channel(peerId) else Target.Direct(peerId)
        val result = engine.sendAttachment(target, mime, name, data, durationMs)
        val id = result.getOrElse {
            _uiEvents.tryEmit(UiEvent.Error(it.message ?: "attachment rejected (profile cap?)"))
            System.currentTimeMillis()
        }
        val local = File(attachmentsDir(), "att_$id").apply { writeBytes(data) }
        val message = Message(
            id = id,
            peerId = peerId,
            fromId = _selfNodeId.value ?: "self",
            toId = peerId,
            kind = MessageKind.ATTACHMENT,
            text = name,
            sentAtMs = System.currentTimeMillis(),
            outgoing = true,
            delivery = if (result.isSuccess) DeliveryState.SENT else DeliveryState.FAILED,
            attachment = Attachment(
                id = "att_$id",
                mime = mime,
                sizeBytes = data.size,
                durationMs = durationMs?.toInt(),
                name = name,
                progress = 1f,
            ),
        )
        appendMessage(peerId, message, channel)
        scope.launch { dao.upsertMessage(MessageEntity.fromModel(message)) }
        if (engine.isDemo()) simulateAck(peerId, id, channel)
        // Keep the local file reference for playback/retry.
        local.absolutePath
        return id
    }

    fun retryMessage(messageId: Long) {
        engine.retryMessage(messageId)
        scope.launch {
            updateDeliveryAnywhere(messageId, DeliveryState.SENT, null)
        }
    }

    fun markThreadRead(peerId: String, channel: Boolean = false) {
        val thread = messageThread(peerId)
        thread.filter { !it.outgoing && !it.read }.forEach { engine.markRead(it.id) }
        scope.launch { dao.markThreadRead(peerId) }
        val mapped = thread.map { if (it.outgoing) it else it.copy(read = true) }
        if (channel) {
            _channelMessages.value = _channelMessages.value + (peerId to mapped)
            _channels.value = _channels.value.map { if (it.id == peerId) it.copy(unreadCount = 0) else it }
        } else {
            _messages.value = _messages.value + (peerId to mapped)
        }
        rebuildConversations()
    }

    fun attachmentFile(attachmentId: String): File? =
        File(attachmentsDir(), attachmentId).takeIf { it.isFile }

    private fun attachmentsDir(): File = File(appContext.filesDir, "attachments").apply { mkdirs() }

    private fun simulateAck(peerId: String, id: Long, channel: Boolean) {
        scope.launch {
            delay(800)
            updateDeliveryAnywhere(id, DeliveryState.SENT, channel)
            delay(1200)
            updateDeliveryAnywhere(id, DeliveryState.DELIVERED, channel)
        }
    }

    private fun appendMessage(peerId: String, message: Message, channel: Boolean) {
        if (channel) {
            val thread = _channelMessages.value[peerId].orEmpty()
            _channelMessages.value = _channelMessages.value + (peerId to (thread + message))
        } else {
            val thread = _messages.value[peerId].orEmpty()
            _messages.value = _messages.value + (peerId to (thread + message))
            rebuildConversations()
        }
    }

    private fun updateDeliveryAnywhere(id: Long, state: DeliveryState, channel: Boolean?) {
        fun mapThreads(map: Map<String, List<Message>>): Map<String, List<Message>> =
            map.mapValues { (_, list) -> list.map { if (it.id == id) it.copy(delivery = state) else it } }
        _messages.value = mapThreads(_messages.value)
        _channelMessages.value = mapThreads(_channelMessages.value)
        rebuildConversations()
        scope.launch {
            val entity = dao.messages().firstOrNull { it.id == id }
            if (entity != null) dao.updateDelivery(id, state.name, entity.read)
        }
    }

    private fun rebuildConversations() {
        val contacts = _contacts.value
        _conversations.value = _messages.value.map { (peerId, thread) ->
            val contact = contacts.firstOrNull { it.nodeId == peerId }
            Conversation(
                peerId = peerId,
                peerName = contact?.name ?: displayNameFor(peerId),
                lastMessage = thread.lastOrNull(),
                unreadCount = thread.count { !it.outgoing && !it.read },
                online = _nodes.value.any { it.nodeId == peerId && it.lastSeenMs > System.currentTimeMillis() - 5 * 60_000 },
                contact = contact != null,
            )
        }.sortedByDescending { it.lastMessage?.sentAtMs ?: 0L }
    }

    // ---------------------------------------------------------------- core events

    private fun handleCoreEvent(event: CoreEvent) {
        when (event) {
            is CoreEvent.MessageReceived -> {
                val channel = event.message.peerId in _channels.value.map { it.id }
                val peerId = event.message.peerId
                if (!event.message.outgoing) {
                    appendMessage(peerId, event.message, channel)
                    scope.launch { dao.upsertMessage(MessageEntity.fromModel(event.message)) }
                    if (channel) {
                        _channels.value = _channels.value.map {
                            if (it.id == peerId) it.copy(unreadCount = it.unreadCount + 1) else it
                        }
                    }
                    if (_settings.value.notificationsEnabled) {
                        MeshLog.i("notification: new message peer=$peerId channel=$channel")
                        Notifications.showMessage(
                            appContext,
                            peerId = peerId,
                            sender = if (channel) displayNameFor(peerId) else displayNameFor(event.message.fromId),
                            body = event.message.text ?: "New message",
                            isChannel = channel,
                        )
                    }
                }
            }

            is CoreEvent.DeliveryChanged -> updateDeliveryAnywhere(event.messageId, event.state, null)

            is CoreEvent.NodeSeen -> {
                val contact = contactFor(event.node.nodeId) != null
                val node = event.node.copy(isContact = contact)
                _nodes.value = _nodes.value.filterNot { it.nodeId == node.nodeId } + node
                scope.launch { dao.upsertNode(NodeEntity.fromModel(node)) }
            }

            is CoreEvent.ResourceProgress -> {
                fun progress(list: List<Message>) = list.map {
                    if (it.id == event.messageId && it.attachment != null) {
                        it.copy(attachment = it.attachment.copy(progress = event.received.toFloat() / event.total.coerceAtLeast(1)))
                    } else {
                        it
                    }
                }
                _messages.value = _messages.value.mapValues { (_, list) -> progress(list) }
                _channelMessages.value = _channelMessages.value.mapValues { (_, list) -> progress(list) }
            }

            is CoreEvent.ResourceReceived -> {
                val file = File(attachmentsDir(), "att_${event.messageId}").apply { writeBytes(event.data) }
                val text = if (event.mime.startsWith("text/")) String(event.data, Charsets.UTF_8) else event.filename ?: "Attachment"
                val channel = event.channelId != null && event.channelId in _channels.value.map { it.id }
                val peerId = event.channelId ?: event.peerId
                val message = Message(
                    id = event.messageId,
                    peerId = peerId,
                    fromId = event.peerId,
                    toId = peerId,
                    kind = MessageKind.ATTACHMENT,
                    text = text,
                    sentAtMs = System.currentTimeMillis(),
                    receivedAtMs = System.currentTimeMillis(),
                    outgoing = false,
                    delivery = DeliveryState.DELIVERED,
                    attachment = Attachment(
                        id = "att_${event.messageId}",
                        mime = event.mime,
                        sizeBytes = event.data.size,
                        durationMs = event.durationMs?.toInt(),
                        name = event.filename ?: file.name,
                        progress = 1f,
                    ),
                )
                appendMessage(peerId, message, channel)
                scope.launch { dao.upsertMessage(MessageEntity.fromModel(message)) }
                if (_settings.value.notificationsEnabled) {
                    Notifications.showMessage(
                        appContext,
                        peerId = peerId,
                        sender = if (channel) displayNameFor(peerId) else displayNameFor(event.peerId),
                        body = if (event.mime.startsWith("image/")) "📷 Photo" else if (event.mime.startsWith("audio/")) "🎤 Voice note" else text,
                        isChannel = channel,
                    )
                }
            }

            is CoreEvent.StoreOffer -> _uiEvents.tryEmit(
                UiEvent.Snackbar("${displayNameFor(event.relayId)} holds ${event.count} message(s) for you"),
            )

            is CoreEvent.CoreError -> _uiEvents.tryEmit(UiEvent.Snackbar(event.message))
        }
    }

    // ---------------------------------------------------------------- companion link

    fun connectCompanion(kind: CompanionKind) {
        if (kind == CompanionKind.NONE) {
            disconnectCompanion()
            return
        }
        if (companionKind == kind && _radio.value.connected) return
        disconnectCompanion()
        val newTransport: CompanionTransport = when (kind) {
            CompanionKind.USB -> UsbSerialTransport(appContext)
            CompanionKind.BLE -> BleTransport(appContext)
            CompanionKind.NONE -> return
        }
        val client = FramedCompanionClient(newTransport)
        transport = newTransport
        companion = client
        companionKind = kind
        MeshLog.i("radio connect: kind=$kind")
        client.start(scope)
        scope.launch { prefs.setRadioKind(if (kind == CompanionKind.USB) "usb" else "ble") }
        companionJob = scope.launch {
            val result = newTransport.connect()
            if (result.isFailure) {
                MeshLog.w("radio connect failed: ${result.exceptionOrNull()?.message}")
                _radio.value = _radio.value.copy(connected = false, transportName = newTransport.label)
                _uiEvents.tryEmit(UiEvent.Error(result.exceptionOrNull()?.message ?: "Could not connect to radio"))
                return@launch
            }
            MeshLog.i("radio connected: ${newTransport.label}")
            MeshLog.i("radio connected: ${newTransport.label}")
            _radio.value = _radio.value.copy(connected = true, transportName = newTransport.label)
            pushRadioConfig(client)
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
                        engine.ingestFrame(event.frame, System.currentTimeMillis())
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
                        updateSettings {
                            it.copy(
                                profile = entries["radio.profile"] ?: it.profile,
                                region = entries["radio.region"] ?: it.region,
                                nodeName = entries["node.name"] ?: it.nodeName,
                                storeForward = (entries["mesh.store"] ?: "1") == "1",
                            )
                        }
                    }

                    is FramedCompanionClient.CompanionEvent.RadioEvent ->
                        _uiEvents.tryEmit(UiEvent.Snackbar("Radio: ${event.kind} ${event.detail}"))

                    is FramedCompanionClient.CompanionEvent.Pong,
                    is FramedCompanionClient.CompanionEvent.Failure,
                    -> Unit
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

    // ---------------------------------------------------------------- contacts / invites / channels

    fun importInvite(uri: String): Result<QrInvite> = runCatching {
        val invite = QrInvite.parse(uri)
        when (invite.kind) {
            QrInvite.Kind.CONTACT -> {
                engine.addContact(invite.identityBase64, invite.name)
                val contact = Contact(
                    nodeId = invite.nodeId,
                    name = invite.name,
                    identityBase64 = invite.identityBase64,
                    lastSeenMs = System.currentTimeMillis(),
                    verified = true,
                )
                _contacts.value = _contacts.value.filterNot { it.nodeId == contact.nodeId } + contact
                val node = NodeInfo(
                    nodeId = contact.nodeId,
                    name = contact.name,
                    nodeClass = NodeClass.CLIENT,
                    lastSeenMs = contact.lastSeenMs,
                    isContact = true,
                )
                _nodes.value = _nodes.value.filterNot { it.nodeId == node.nodeId } + node
                scope.launch {
                    dao.upsertContact(ContactEntity(contact.nodeId, contact.name, contact.identityBase64, contact.lastSeenMs, contact.verified))
                    dao.upsertNode(NodeEntity.fromModel(node))
                }
                rebuildConversations()
            }

            QrInvite.Kind.CHANNEL -> {
                val joined = engine.joinChannel(invite.name, invite.pskBase64)
                val channelId = joined.getOrNull()?.channelIdHex ?: invite.nodeId
                val channel = Channel(channelId, invite.name, pskSet = invite.pskBase64.isNotEmpty())
                if (_channels.value.none { it.id == channelId }) {
                    _channels.value = _channels.value + channel
                }
                scope.launch {
                    dao.upsertChannel(ChannelEntity(channelId, invite.name, channel.pskSet, joined.getOrNull()?.inviteUrl))
                }
            }
        }
        _uiEvents.tryEmit(UiEvent.InviteImported(invite.name, invite.kind == QrInvite.Kind.CHANNEL))
        invite
    }

    fun createChannel(name: String): Result<ChannelInvite> {
        val result = engine.createChannel(name)
        result.onFailure { MeshLog.w("channel create failed: ${it.message}") }
        result.getOrNull()?.let { invite ->
            MeshLog.i("channel created ${invite.channelIdHex} name=${invite.name}")
            _lastChannelInvite.value = invite.inviteUrl
            val channel = Channel(invite.channelIdHex, invite.name)
            _channels.value = _channels.value.filterNot { it.id == channel.id } + channel
            scope.launch { dao.upsertChannel(ChannelEntity(channel.id, channel.name, true, invite.inviteUrl)) }
        }
        return result
    }

    fun joinChannel(name: String, pskBase64: String): Result<ChannelInvite> {
        val result = engine.joinChannel(name, pskBase64)
        result.getOrNull()?.let { invite ->
            _lastChannelInvite.value = invite.inviteUrl
            val channel = Channel(invite.channelIdHex, invite.name)
            _channels.value = _channels.value.filterNot { it.id == channel.id } + channel
            scope.launch { dao.upsertChannel(ChannelEntity(channel.id, channel.name, true, invite.inviteUrl)) }
        }
        return result
    }

    /** Invite URL for a joined channel, for the QR/share sheet. */
    fun channelInvite(channelId: String): String? =
        engine.channels().firstOrNull { it.channelIdHex == channelId }?.inviteUrl
            ?: _lastChannelInvite.value

    fun removeContact(nodeId: String) {
        _contacts.value = _contacts.value.filterNot { it.nodeId == nodeId }
        _nodes.value = _nodes.value.map { if (it.nodeId == nodeId) it.copy(isContact = false) else it }
        engine.removeContact(nodeId)
        scope.launch { dao.deleteContact(nodeId) }
    }

    // ---------------------------------------------------------------- onboarding / presets

    /** Load the shared presets from assets (cached). */
    fun presets(): org.openmesh.android.data.presets.SetupPresets =
        runCatching { PresetLoader.fromAssets(appContext) }
            .getOrElse { PresetLoader.fromRepoFile() }

    /** Apply the beginner secure preset locally, persist it and create the private channel. */
    suspend fun applyBeginnerPreset(answers: BeginnerAnswers): Result<AppliedPreset> = runCatching {
        MeshLog.i("onboarding: applying preset role=${answers.role} country=${answers.country} privacy=${answers.privacy}")
        val presetFile = presets()
        val applied = BeginnerPreset.apply(presetFile, answers)
        val settings = _settings.value.copy(
            nodeName = answers.nodeName.ifBlank { "openmesh-android" },
            role = answers.role,
            region = applied.region,
            profile = applied.profile,
            storeForward = applied.storeForward,
            announceIntervalSec = applied.announceIntervalS.toInt(),
            positionSharing = BeginnerPreset.positionEnabled(applied),
            demoMode = false,
        )
        _settings.value = settings
        prefs.setNodeName(settings.nodeName)
        prefs.setRole(answers.role)
        prefs.setRegion(applied.region)
        prefs.setProfile(applied.profile)
        prefs.setStoreForward(applied.storeForward)
        prefs.setAnnounceInterval(applied.announceIntervalS)
        prefs.setPositionSharing(applied.positionSharing)
        prefs.setDemoMode(false)
        prefs.setAutoConnect(true)
        val ready = ensureStartedAwait(20_000)
        if (!ready) {
            MeshLog.e("onboarding: engine not ready; cannot apply engine config or create channel")
            return Result.failure(IllegalStateException("The protocol engine did not start. Check logcat tag OpenMesh and try again."))
        }
        runCatching {
            engine.setConfig(
                """{"name":${jsonString(settings.nodeName)},"nodeClass":"${answers.role}","profile":"${applied.profile}","region":"${applied.region}","storeForward":${applied.storeForward},"announceIntervalS":${applied.announceIntervalS}}""",
            )
        }.onFailure {
            MeshLog.w("onboarding: engine setConfig failed: ${it.message}")
            return Result.failure(IllegalStateException("Could not apply radio settings: ${it.message}"))
        }
        MeshLog.i("onboarding: engine config applied")
        prefs.setSetupCompleted(true)
        _settings.value = _settings.value.copy(setupCompleted = true)
        MeshLog.i("onboarding: preset persisted role=${answers.role} country=${answers.country} region=${applied.region} profile=${applied.profile} setup_completed=true")
        if (answers.privacy == "private") {
            // Re-running onboarding (or a QA reset) must not create a second "family" channel.
            if (_channels.value.any { it.name == "family" }) {
                MeshLog.i("onboarding: reusing existing private channel named family")
            } else {
                val channel = createChannel("family")
                if (channel.isFailure) {
                    MeshLog.w("onboarding: channel creation failed: ${channel.exceptionOrNull()?.message}")
                    // Setup stays complete (identity/radio/preset persisted); onboarding offers retry.
                }
            }
        }
        applied
    }

    /** Retry the private-channel creation from onboarding without redoing the whole preset. */
    suspend fun retryDefaultChannel(name: String = "family"): Result<ChannelInvite> {
        val ready = ensureStartedAwait(20_000)
        if (!ready) return Result.failure(IllegalStateException("The protocol engine is not running yet."))
        val result = createChannel(name)
        MeshLog.i("onboarding: retry channel -> ${result.isSuccess}")
        return result
    }

    /** QA/debug: clear the onboarding flag (keeps identity and data) so the wizard can be re-run. */
    fun resetOnboarding() {
        MeshLog.w("QA reset: onboarding flag cleared")
        _lastChannelInvite.value = null
        _settings.value = _settings.value.copy(setupCompleted = false, demoMode = false)
        scope.launch { prefs.setSetupCompleted(false) }
    }

    fun completeSetupWithoutChannel() {
        scope.launch { prefs.setSetupCompleted(true) }
    }

    // ---------------------------------------------------------------- regulatory override (v0.5.0)

    fun regulatoryOverride(): RegulatoryOverride = RegulatoryMapper.fromSettings(_settings.value)

    /** Effective override status line for radio/status cards. */
    fun regulatoryStatusLine(): String =
        RegulatoryMapper.statusLine(_settings.value.region, regulatoryOverride())

    fun legalLine(): String = RegionLimits.legalLine(_settings.value.region)

    /**
     * Validate, apply to the engine (camelCase `regulatoryOverride` patch) and, only on success,
     * persist + push the effective values to the companion radio. Returns the engine/policy error
     * so the modal can show it instead of silently failing.
     */
    fun setRegulatoryOverride(override: RegulatoryOverride): Result<Unit> {
        val region = _settings.value.region
        override.validationError(region)?.let { message ->
            MeshLog.w("regulatory: rejected locally: $message")
            return Result.failure(IllegalArgumentException(message))
        }
        val ready = _engineReady.value
        if (ready) {
            val applied = runCatching { engine.setConfig(override.toEnginePatch()) }
            applied.exceptionOrNull()?.let { error ->
                MeshLog.w("regulatory: engine rejected override: ${error.message}")
                return Result.failure(IllegalStateException(error.message ?: "The engine rejected the regulatory override."))
            }
        } else if (override.enabled) {
            return Result.failure(IllegalStateException("The protocol engine is not running yet; try again in a moment."))
        }
        _settings.value = RegulatoryMapper.applyTo(_settings.value, override)
        MeshLog.i(
            "regulatory: override ${if (override.enabled) "enabled" else "disabled"} " +
                "region=$region duty=${override.dutyPct ?: "legal"} power=${override.powerDbm ?: "legal"} " +
                "freq=${override.freqKhz ?: "legal"} acknowledged=${override.acknowledged}",
        )
        scope.launch {
            prefs.setOverride(
                enabled = override.enabled,
                acknowledged = override.acknowledged,
                dutyPct = override.dutyPct,
                powerDbm = override.powerDbm,
                freqKhz = override.freqKhz,
                reason = override.reason,
            )
            companion?.let { client -> pushRadioConfig(client) }
        }
        return Result.success(Unit)
    }

    /** One-tap disable: clears the effect and the acknowledgement (re-enabling requires a new ack). */
    fun disableRegulatoryOverride(): Result<Unit> =
        setRegulatoryOverride(regulatoryOverride().copy(enabled = false, acknowledged = false))

    private fun pushRadioConfig(client: FramedCompanionClient) {
        val settings = _settings.value
        val override = regulatoryOverride()
        scope.launch {
            runCatching { client.sendConfigSet("radio.profile", settings.profile) }
            runCatching { client.sendConfigSet("radio.region", settings.region) }
            runCatching { client.sendConfigSet("node.name", settings.nodeName) }
            runCatching { client.sendConfigSet("mesh.store", if (settings.storeForward) "1" else "0") }
            runCatching {
                override.companionConfig(settings.region).forEach { (key, value) ->
                    client.sendConfigSet(key, value)
                }
            }.onFailure { MeshLog.w("radio config push failed: ${it.message}") }
            MeshLog.i("radio config pushed: ${override.companionConfig(settings.region)}")
        }
    }

    // ---------------------------------------------------------------- settings

    fun updateSettings(transform: (AppSettings) -> AppSettings) {
        val previous = _settings.value
        val updated = transform(previous)
        _settings.value = updated
        scope.launch {
            if (updated.uiMode != previous.uiMode) prefs.setMode(UiMode.from(updated.uiMode))
            if (updated.nodeName != previous.nodeName) prefs.setNodeName(updated.nodeName)
            if (updated.profile != previous.profile) prefs.setProfile(updated.profile)
            if (updated.region != previous.region) prefs.setRegion(updated.region)
            if (updated.storeForward != previous.storeForward) prefs.setStoreForward(updated.storeForward)
            if (updated.announceIntervalSec != previous.announceIntervalSec) prefs.setAnnounceInterval(updated.announceIntervalSec.toLong())
            if (updated.positionSharing != previous.positionSharing) prefs.setPositionSharing(if (updated.positionSharing) "channel" else "private")
            if (updated.autoConnect != previous.autoConnect) prefs.setAutoConnect(updated.autoConnect)
            if (updated.notificationsEnabled != previous.notificationsEnabled) prefs.setNotifications(updated.notificationsEnabled)
            if (updated.defaultImageTier != previous.defaultImageTier) prefs.setDefaultImageTier(updated.defaultImageTier)
            if (updated.defaultVoiceTier != previous.defaultVoiceTier) prefs.setDefaultVoiceTier(updated.defaultVoiceTier)
        }
        if (started) {
            val patch = """{"name":${jsonString(updated.nodeName)},"profile":"${updated.profile}","region":"${updated.region}","storeForward":${updated.storeForward},"announceIntervalS":${updated.announceIntervalSec.toLong()}}"""
            runCatching { engine.setConfig(patch) }
            val radioChanged = updated.profile != previous.profile || updated.region != previous.region
            if (radioChanged) {
                companion?.let { client -> pushRadioConfig(client) }
            }
        }
    }

    fun setUiMode(mode: UiMode) = updateSettings { it.copy(uiMode = mode.wire) }

    fun setDemoMode(enabled: Boolean) = updateSettings { it.copy(demoMode = enabled) }

    fun setRole(role: String) = updateSettings { it.copy(role = role) }

    // ---------------------------------------------------------------- airtime UX

    fun estimateMedia(bytes: Int, kind: MediaKind = MediaKind.IMAGE): MediaEstimate {
        val profile = LoraProfile.fromName(_settings.value.profile)
        val region = Region.fromName(_settings.value.region)
        val transfer = estimateResourceTransfer(profile, bytes)
        val wall = wallClockMs(transfer.airtimeMs, region.dutyCyclePct)
        val hourlyBudgetMs = region.dutyCyclePct?.let { it * 36_000f }
        val ratio = if (hourlyBudgetMs == null || hourlyBudgetMs <= 0f) 0f else transfer.airtimeMs.toFloat() / hourlyBudgetMs
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

    fun mediaCapBytes(): Int = MediaMath.capBytes(_settings.value.profile)

    fun maxVoiceSeconds(): Int = MediaMath.maxVoiceSeconds(_settings.value.profile)

    // ---------------------------------------------------------------- helpers

    private fun org.openmesh.android.data.prefs.LocalPrefs.toSettings(): AppSettings = AppSettings(
        nodeName = nodeName,
        profile = profile,
        region = region,
        announceIntervalSec = announceIntervalS.toInt(),
        positionSharing = positionSharing == "channel",
        storeForward = storeForward,
        autoConnect = autoConnect,
        preferUsb = radioKind != "ble",
        showAirtimeEstimates = true,
        demoMode = demoMode,
        uiMode = uiMode.wire,
        notificationsEnabled = notificationsEnabled,
        setupCompleted = setupCompleted,
        radioKind = radioKind,
        overrideEnabled = overrideEnabled,
        overrideAcknowledged = overrideAcknowledged,
        overrideDutyPct = overrideDutyPct,
        overridePowerDbm = overridePowerDbm,
        overrideFreqKhz = overrideFreqKhz,
        overrideReason = overrideReason,
        defaultImageTier = defaultImageTier,
        defaultVoiceTier = defaultVoiceTier,
    )

    private fun jsonString(value: String): String = buildString {
        append('"')
        value.forEach { ch ->
            when (ch) {
                '"' -> append("\\\"")
                '\\' -> append("\\\\")
                '\n' -> append("\\n")
                else -> if (ch.code < 0x20) append("\\u%04x".format(ch.code)) else append(ch)
            }
        }
        append('"')
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

    const val ACTION_USB_PERMISSION = "org.openmesh.android.USB_PERMISSION"
}
