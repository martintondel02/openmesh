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
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeoutOrNull
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
import org.openmesh.android.data.model.PublicMesh
import org.openmesh.android.data.model.Position
import org.openmesh.android.data.model.RadioStatus
import org.openmesh.android.data.notify.Notifications
import org.openmesh.android.data.prefs.AppPreferences
import org.openmesh.android.data.prefs.SequenceFloorStore
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
import org.openmesh.android.data.transport.LinkError
import org.openmesh.android.data.transport.LinkProgress
import org.openmesh.android.data.transport.RadioErrorHints
import org.openmesh.android.data.transport.RadioHealth
import org.openmesh.android.data.transport.RadioHealthPolicy
import org.openmesh.android.data.transport.RadioPhase
import org.openmesh.android.data.transport.UsbSerialTransport
import org.openmesh.android.media.MediaMath
import org.openmesh.android.media.OutgoingMediaPolicy
import org.openmesh.android.media.OutgoingMediaSnapshot
import org.openmesh.android.media.OutgoingMediaUi
import org.openmesh.android.media.OutgoingMediaUiState
import org.openmesh.android.ui.people.DeliveryPresentation
import org.openmesh.android.ui.people.NamePolicy
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

    // ---- Outgoing media upload tracking (in-memory only; progress is not persisted). ----
    private val _outgoingMedia = MutableStateFlow<Map<Long, OutgoingMediaSnapshot>>(emptyMap())
    val outgoingMedia: StateFlow<Map<Long, OutgoingMediaSnapshot>> = _outgoingMedia.asStateFlow()
    private val _outgoingMediaUi = MutableStateFlow<Map<Long, OutgoingMediaUi>>(emptyMap())
    val outgoingMediaUi: StateFlow<Map<Long, OutgoingMediaUi>> = _outgoingMediaUi.asStateFlow()

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
    private var companionAttempt = 0
    /** True while the user asked to disconnect, so the link watcher does not report "lost". */
    @Volatile private var userDisconnect = false
    @Volatile private var latestPrefs = org.openmesh.android.data.prefs.LocalPrefs()

    /** Wall-clock of the last resource frame/ACK activity (inbound or outbound), for TX safety. */
    @Volatile private var lastResourceActivityMs: Long = 0L

    /** True while an automatic upload retry is being started, so only one runs at a time. */
    @Volatile private var autoRetryInFlight = false

    /** Wall-clock of the last successful engine-state export (debounced + periodic writes). */
    @Volatile private var lastStatePersistMs = 0L

    /** While a factory reset is wiping stores, no persistence may recreate old state. */
    @Volatile private var factoryResetInProgress = false

    /** Cheap per-send frame sequence floor (SharedPreferences; survives process death). */
    private val seqFloorStore = SequenceFloorStore(appContext)
    @Volatile private var lastSeqFloorPersistMs = 0L

    // ---- Queued outbound messages (radio not READY): held locally, sent on the next READY link. ----
    private val MAX_QUEUED_MESSAGES = 50
    private val HANDSHAKE_TIMEOUT_MS = 20_000L
    private val queueLock = Any()
    private val outboundQueue = ArrayDeque<QueuedOutbound>()
    private val flushMutex = Mutex()
    private var nextLocalMessageId = -1L

    /** A message the user wrote while the radio link was not usable yet. */
    private data class QueuedOutbound(
        val localId: Long,
        val peerId: String,
        val channel: Boolean,
        val text: String? = null,
        val attachmentMime: String? = null,
        val attachmentName: String? = null,
        val attachmentBytes: Int = 0,
        val attachmentDurationMs: Long? = null,
        val attachmentFile: File? = null,
    )

    init {
        scope.launch { load() }
        // Liveness of a READY link: the companion client pings every 15 s; if the radio is silent
        // for ~30 s the UI switches to "not answering" (phase stays READY, health STALE).
        scope.launch {
            while (isActive) {
                delay(1_000)
                val current = _radio.value
                val health = RadioHealthPolicy.compute(current.phase, current.lastReplyMs, System.currentTimeMillis())
                if (health != current.health) _radio.value = current.copy(health = health)
            }
        }
        // Keep the persisted engine state (frame sequence floor + message counters) fresh while
        // running: a process restart must never re-originate frames a peer has already seen.
        scope.launch {
            while (isActive) {
                delay(2_000)
                val now = System.currentTimeMillis()
                if (_engineReady.value &&
                    EngineStatePersistence.shouldPersistPeriodically(lastStatePersistMs, now)
                ) {
                    persistEngineState(force = true)
                }
            }
        }
        // Re-evaluate upload health once a second so stalled uploads flip to "Stalled — tap Retry"
        // and get one bounded automatic retry at a time with exponential backoff.
        scope.launch {
            while (isActive) {
                delay(1_000)
                refreshOutgoingMediaUi()
                maybeAutoRetryStalledUploads()
            }
        }
        // Flush locally queued messages whenever a link becomes READY (connect or reconnect).
        scope.launch {
            _radio.map { it.phase }.distinctUntilChanged().collect { phase ->
                if (phase == RadioPhase.READY) flushQueuedMessages()
            }
        }
    }

    private suspend fun load() {
        val saved = prefs.current()
        latestPrefs = saved
        _settings.value = saved.toSettings()
        loadFromDatabase()
        if (saved.demoMode) {
            seedDemo()
        } else {
            rehydrateQueuedMessages()
            if (saved.setupCompleted) ensureStarted()
        }
        if (!saved.demoMode && saved.autoConnect && saved.radioKind != "none" && saved.setupCompleted) {
            connectCompanion(if (saved.radioKind == "usb") CompanionKind.USB else CompanionKind.BLE)
        }
        _initialized.value = true
        MeshLog.i("repository loaded: setup_completed=${saved.setupCompleted} mode=${saved.uiMode.wire} radio=${saved.radioKind}")
    }

    private suspend fun loadFromDatabase() {
        val channels = dao.channels().map {
            Channel(
                id = it.id,
                name = it.name,
                pskSet = it.pskSet,
                isPublic = PublicMesh.isPublicChannel(it.id, it.name),
            )
        }
        val channelIds = channels.map { it.id }.toSet()
        val all = dao.messages().map { it.toModel() }
        val direct = all.filter { it.peerId !in channelIds }
        val channelMsgs = all.filter { it.peerId in channelIds }
        _channels.value = channels
        _contacts.value = dao.contacts().map { it.toModel() }
        _nodes.value = dao.nodes().map { it.toModel() }
        _messages.value = direct.groupBy { it.peerId }.mapValues { (_, list) -> list.sortedBy { it.sentAtMs } }
        _channelMessages.value = channelMsgs.groupBy { it.peerId }.mapValues { (_, list) -> list.sortedBy { it.sentAtMs } }
        seedOutgoingMedia(direct, channelMsgs, channelIds)
        rebuildConversations()
    }

    /**
     * After a restart the engine no longer reports progress for a transfer it lost. Seed a tracker
     * for every outgoing attachment still marked SENT so the stall detector can surface it and the
     * user can retry instead of watching "Sent" forever.
     */
    private fun seedOutgoingMedia(
        direct: List<Message>,
        channelMsgs: List<Message>,
        channelIds: Set<String>,
    ) {
        val seeded = LinkedHashMap<Long, OutgoingMediaSnapshot>()
        fun add(list: List<Message>, isChannel: Boolean) {
            list.filter { it.outgoing && it.kind == MessageKind.ATTACHMENT && it.delivery == DeliveryState.SENT }
                .forEach { message ->
                    val attachment = message.attachment ?: return@forEach
                    seeded[message.id] = OutgoingMediaPolicy.initial(
                        isChannel = isChannel,
                        mime = attachment.mime,
                        sizeBytes = attachment.sizeBytes,
                        sentAtMs = message.sentAtMs,
                        expectedWindowMs = expectedMediaWindowMs(attachment.sizeBytes),
                    ).copy(manualOnly = true)
                }
        }
        add(direct.filter { it.peerId !in channelIds }, false)
        add(channelMsgs, true)
        _outgoingMedia.value = seeded
        refreshOutgoingMediaUi()
    }

    private fun seedDemo() {
        _settings.value = _settings.value.copy(demoMode = true)
        _nodes.value = DemoData.nodes
        _contacts.value = DemoData.contacts
        _channels.value = DemoData.channels
        _messages.value = DemoData.messages
        _channelMessages.value = DemoData.channelMessages
        // Demo mode still shows how many real messages are waiting for a radio link.
        val waiting = synchronized(queueLock) { outboundQueue.size }
        _radio.value = DemoData.radioStatus.copy(queuedMessages = waiting)
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
        // Restore the frame sequence floor BEFORE any channel heal/announce/send. The persisted
        // per-send floor wins over a stale full-state export so peers never see a reused (src, seq).
        val importedFloor = EngineStatePersistence.sequenceFloorFromState(state)
        val persistedFloor = seqFloorStore.read()
        val restoredFloor = EngineStatePersistence.restoredSequenceFloor(persistedFloor, importedFloor)
        if (restoredFloor > 0L) {
            runCatching { engine.setSequenceFloor(restoredFloor) }
                .onFailure { MeshLog.w("setSequenceFloor($restoredFloor) failed: ${it.message}") }
            lastSeqFloorPersistMs = System.currentTimeMillis()
            val seqAfter = runCatching { engine.lastSeq() }.getOrNull()
            MeshLog.i(
                "sequence floor restored: persisted=$persistedFloor import=${importedFloor ?: 0} " +
                    "-> applied=$restoredFloor seq_after=$seqAfter (next frame must be > $restoredFloor)",
            )
        } else {
            MeshLog.i("sequence floor restored: none persisted yet (fresh install)")
        }
        // Heal channel keys after a process restart. Engine state is exported on stop(); if the
        // process was killed (or the app updated) channels joined earlier can be missing from the
        // engine even though they are persisted in the DB, which made sending fail with UnknownNode.
        runCatching {
            val known = engine.channels().map { it.channelIdHex }.toSet()
            for (row in dao.channels()) {
                if (row.id in known) continue
                val invite = row.inviteUrl?.let { runCatching { QrInvite.parse(it) }.getOrNull() } ?: continue
                if (invite.kind != QrInvite.Kind.CHANNEL || invite.pskBase64.isEmpty()) continue
                engine.joinChannel(invite.name, invite.pskBase64)
            }
        }.onFailure { MeshLog.w("channel restore failed: ${it.message}") }
        ensurePublicMesh()
        _selfNodeId.value = engine.nodeIdHex()
        _selfIdentity.value = engine.publicIdentityB64()
        _engineStatus.value = engine.status.value
        engineJobs += scope.launch { engine.events.collect { handleCoreEvent(it) } }
        engineJobs += scope.launch {
            engine.outbound.collect { out ->
                val c = companion
                if (c == null) {
                    MeshLog.w("radio tx dropped (no companion) frame=${out.frame.size}")
                    return@collect
                }
                MeshLog.i("radio tx frame size=${out.frame.size} peer=${out.directPeerHex ?: "broadcast"}")
                runCatching { c.sendFrame(out.frame) }
                    .onFailure { MeshLog.w("radio tx failed: ${it.message}", it) }
                // Every originated frame consumes a sequence number; keep the cheap floor fresh
                // (throttled to ~1/s) and the full state on its slower periodic schedule.
                persistSequenceFloor()
                persistEngineState()
            }
        }
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
        applyResourcePaceHint()
        runCatching { engine.announce(System.currentTimeMillis()) }
        persistSequenceFloor(force = true)
        if (saved.demoMode) seedDemo()
        _engineReady.value = true
        // Messages written while the engine/link was not ready are sent now (if the link is READY).
        if (_radio.value.phase == RadioPhase.READY) maybeFlushQueuedMessages()
        MeshLog.i("engine start: ready node=${engine.nodeIdHex() ?: "?"} name=${saved.nodeName} profile=${saved.profile}/${saved.region}")
    }

    /** Persist engine state and stop timers (called when the user stops the mesh). */
    fun stop() {
        MeshLog.i("engine stop requested")
        synchronized(startedLock) { started = false }
        _engineReady.value = false
        tickJob?.cancel()
        tickJob = null
        persistSequenceFloor(force = true)
        scope.launch { persistEngineStateNow() }
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

    /**
     * Best display name: channel name → name the peer last announced → locally pinned
     * invite/contact name → "node <first8>". An announced rename replaces the stale cached name;
     * the pinned invite name is only a fallback (there is no alias-editing UI yet).
     */
    fun displayNameFor(nodeId: String): String = NamePolicy.displayName(
        peerId = nodeId,
        announcedName = nodeFor(nodeId)?.name,
        inviteName = contactFor(nodeId)?.name,
        channelName = _channels.value.firstOrNull { it.id == nodeId }?.name,
    )

    /** Short id (first 8 hex chars) shown next to friendly names in peer details. */
    fun shortIdFor(nodeId: String): String = NamePolicy.shortId(nodeId)

    fun sendText(peerId: String, text: String, channel: Boolean = false): Long {
        val trimmed = text.trim()
        if (trimmed.isEmpty()) return -1L
        // Honest delivery: with no READY radio the message is not handed to the engine yet. It is
        // stored locally as QUEUED and actually sent (once) when the link becomes READY.
        if (shouldQueueLocally()) return enqueueText(peerId, trimmed, channel)
        return sendTextNow(peerId, trimmed, channel)
    }

    /** Sends a text through the engine immediately (the link is READY, or demo mode is on). */
    private fun sendTextNow(peerId: String, trimmed: String, channel: Boolean): Long {
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
        persistEngineState()
        persistSequenceFloor(force = true)
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
        // Same honest-delivery rule as text: keep the media locally and send it when READY.
        if (shouldQueueLocally()) return enqueueAttachment(peerId, name, mime, data, durationMs, channel)
        return sendAttachmentNow(peerId, name, mime, data, durationMs, channel)
    }

    private fun sendAttachmentNow(
        peerId: String,
        name: String,
        mime: String,
        data: ByteArray,
        durationMs: Long?,
        channel: Boolean,
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
                progress = 0f,
            ),
        )
        appendMessage(peerId, message, channel)
        scope.launch { dao.upsertMessage(MessageEntity.fromModel(message)) }
        trackOutgoingMedia(message, channel)
        lastResourceActivityMs = System.currentTimeMillis()
        persistEngineState()
        persistSequenceFloor(force = true)
        if (engine.isDemo()) simulateAck(peerId, id, channel)
        // Keep the local file reference for playback/retry.
        local.absolutePath
        return id
    }

    fun retryMessage(messageId: Long) {
        if (messageId < 0) {
            // A locally queued message: do not ask the engine (it never saw this message id).
            if (_radio.value.phase == RadioPhase.READY) {
                maybeFlushQueuedMessages()
            } else {
                _uiEvents.tryEmit(UiEvent.Snackbar("Connect a radio to send this message."))
            }
            return
        }
        engine.retryMessage(messageId)
        scope.launch {
            updateDeliveryAnywhere(messageId, DeliveryState.SENT, null)
        }
    }

    // ---------------------------------------------------------------- queued outbound messages

    /**
     * True when a user send must be held locally instead of being handed to the engine.
     *
     * The radio link must be READY (HelloAck received) and the engine running: sending earlier
     * silently dropped frames because no companion existed to carry them, while the UI claimed
     * "sent". Demo mode is simulated end-to-end and never queues.
     */
    private fun shouldQueueLocally(): Boolean =
        !engine.isDemo() && (_radio.value.phase != RadioPhase.READY || !_engineReady.value)

    private fun enqueueText(peerId: String, text: String, channel: Boolean): Long {
        val localId: Long? = synchronized(queueLock) {
            if (outboundQueue.size >= MAX_QUEUED_MESSAGES) {
                null
            } else {
                val id = nextLocalMessageId--
                outboundQueue.addLast(QueuedOutbound(localId = id, peerId = peerId, channel = channel, text = text))
                id
            }
        }
        if (localId == null) {
            _uiEvents.tryEmit(UiEvent.Error("Too many queued messages (max $MAX_QUEUED_MESSAGES). Connect a radio to send them first."))
            return -1L
        }
        val message = Message(
            id = localId,
            peerId = peerId,
            fromId = _selfNodeId.value ?: "self",
            toId = peerId,
            kind = MessageKind.TEXT,
            text = text,
            sentAtMs = System.currentTimeMillis(),
            outgoing = true,
            delivery = DeliveryState.QUEUED,
        )
        appendMessage(peerId, message, channel)
        scope.launch { dao.upsertMessage(MessageEntity.fromModel(message)) }
        refreshQueuedCount()
        MeshLog.i("send queued (radio not ready) local_id=$localId channel=$channel")
        return localId
    }

    private fun enqueueAttachment(
        peerId: String,
        name: String,
        mime: String,
        data: ByteArray,
        durationMs: Long?,
        channel: Boolean,
    ): Long {
        val localId: Long? = synchronized(queueLock) {
            if (outboundQueue.size >= MAX_QUEUED_MESSAGES) null else nextLocalMessageId--
        }
        if (localId == null) {
            _uiEvents.tryEmit(UiEvent.Error("Too many queued messages (max $MAX_QUEUED_MESSAGES). Connect a radio to send them first."))
            return -1L
        }
        val local = File(attachmentsDir(), "att_$localId")
        if (runCatching { local.writeBytes(data) }.isFailure) {
            _uiEvents.tryEmit(UiEvent.Error("Could not save the queued attachment. Try again."))
            return -1L
        }
        synchronized(queueLock) {
            outboundQueue.addLast(
                QueuedOutbound(
                    localId = localId,
                    peerId = peerId,
                    channel = channel,
                    text = name,
                    attachmentMime = mime,
                    attachmentName = name,
                    attachmentBytes = data.size,
                    attachmentDurationMs = durationMs,
                    attachmentFile = local,
                ),
            )
        }
        val message = Message(
            id = localId,
            peerId = peerId,
            fromId = _selfNodeId.value ?: "self",
            toId = peerId,
            kind = MessageKind.ATTACHMENT,
            text = name,
            sentAtMs = System.currentTimeMillis(),
            outgoing = true,
            delivery = DeliveryState.QUEUED,
            attachment = Attachment(
                id = "att_$localId",
                mime = mime,
                sizeBytes = data.size,
                durationMs = durationMs?.toInt(),
                name = name,
                progress = 1f,
            ),
        )
        appendMessage(peerId, message, channel)
        scope.launch { dao.upsertMessage(MessageEntity.fromModel(message)) }
        refreshQueuedCount()
        MeshLog.i("attachment queued (radio not ready) local_id=$localId channel=$channel bytes=${data.size}")
        return localId
    }

    /** Fire-and-forget flush; coalesced by [flushMutex] and a READY-link check inside. */
    private fun maybeFlushQueuedMessages() {
        if (engine.isDemo()) return
        scope.launch { flushQueuedMessages() }
    }

    /**
     * Sends locally queued messages in order once the link is READY, converting each temp row into
     * the engine-id row with delivery SENT. A failed send keeps the bubble and marks it FAILED.
     */
    private suspend fun flushQueuedMessages() {
        if (engine.isDemo() || _radio.value.phase != RadioPhase.READY) return
        if (!_engineReady.value && !ensureStartedAwait(20_000)) return
        flushMutex.withLock {
            while (true) {
                if (engine.isDemo() || _radio.value.phase != RadioPhase.READY) return@withLock
                val next = synchronized(queueLock) { outboundQueue.firstOrNull() } ?: return@withLock
                val result = runCatching { sendQueuedNow(next) }
                    .getOrElse { error -> Result.failure(error) }
                synchronized(queueLock) {
                    if (outboundQueue.firstOrNull()?.localId == next.localId) outboundQueue.removeFirst()
                }
                refreshQueuedCount()
                result.onSuccess { realId -> convertQueuedToSent(next, realId) }
                    .onFailure { error ->
                        MeshLog.w("queued send failed local=${next.localId}: ${error.message}")
                        updateDeliveryAnywhere(next.localId, DeliveryState.FAILED, next.channel)
                        _uiEvents.tryEmit(UiEvent.Error("A queued message could not be sent: ${error.message ?: "radio busy"}"))
                    }
            }
        }
    }

    private fun sendQueuedNow(queued: QueuedOutbound): Result<Long> {
        val target = if (queued.channel) Target.Channel(queued.peerId) else Target.Direct(queued.peerId)
        return when {
            queued.attachmentFile != null -> {
                val data = queued.attachmentFile.readBytes()
                engine.sendAttachment(
                    target,
                    queued.attachmentMime ?: "application/octet-stream",
                    queued.attachmentName,
                    data,
                    queued.attachmentDurationMs,
                )
            }
            queued.text != null -> engine.sendText(target, queued.text)
            else -> Result.failure(IllegalStateException("queued message has no content"))
        }
    }

    private fun convertQueuedToSent(queued: QueuedOutbound, realId: Long) {
        var attachment: Attachment? = null
        val source = queued.attachmentFile
        if (source != null) {
            val realFile = File(attachmentsDir(), "att_$realId")
            runCatching { source.copyTo(realFile, overwrite = true) }
            runCatching { source.delete() }
            attachment = Attachment(
                id = "att_$realId",
                mime = queued.attachmentMime ?: "application/octet-stream",
                sizeBytes = queued.attachmentBytes,
                durationMs = queued.attachmentDurationMs?.toInt(),
                name = queued.attachmentName,
                progress = 0f,
            )
        }
        // Replace the temporary negative-id bubble with the real engine-id message.
        removeMessageEverywhere(queued.localId)
        val message = Message(
            id = realId,
            peerId = queued.peerId,
            fromId = _selfNodeId.value ?: "self",
            toId = queued.peerId,
            kind = if (attachment != null) MessageKind.ATTACHMENT else MessageKind.TEXT,
            text = queued.text,
            sentAtMs = System.currentTimeMillis(),
            outgoing = true,
            delivery = DeliveryState.SENT,
            attachment = attachment,
        )
        appendMessage(queued.peerId, message, queued.channel)
        trackOutgoingMedia(message, queued.channel)
        persistEngineState(force = true)
        persistSequenceFloor(force = true)
        scope.launch {
            dao.deleteMessage(queued.localId)
            dao.upsertMessage(MessageEntity.fromModel(message))
        }
        MeshLog.i("queued message sent local=${queued.localId} engine_id=$realId channel=${queued.channel}")
    }

    private fun removeMessageEverywhere(id: Long) {
        fun strip(list: List<Message>) = list.filterNot { it.id == id }
        _messages.value = _messages.value.mapValues { (_, list) -> strip(list) }
        _channelMessages.value = _channelMessages.value.mapValues { (_, list) -> strip(list) }
        rebuildConversations()
    }

    private fun refreshQueuedCount() {
        val count = synchronized(queueLock) { outboundQueue.size }
        if (_radio.value.queuedMessages != count) updateRadio { it.copy(queuedMessages = count) }
    }

    /**
     * Restores queued messages from the database after a process restart (they were persisted as
     * QUEUED with a temporary negative id). Payloads: text lives in the row; attachments are read
     * back from the local file written when the message was queued.
     */
    private fun rehydrateQueuedMessages() {
        if (_settings.value.demoMode) return
        val channelIds = _channels.value.map { it.id }.toSet()
        val queuedRows = (_messages.value.values.flatten() + _channelMessages.value.values.flatten())
            .filter { it.outgoing && it.delivery == DeliveryState.QUEUED && it.id < 0 }
            .sortedBy { it.sentAtMs }
        if (queuedRows.isEmpty()) return
        val overflow = mutableListOf<Message>()
        synchronized(queueLock) {
            outboundQueue.clear()
            for (message in queuedRows) {
                if (outboundQueue.size >= MAX_QUEUED_MESSAGES) {
                    overflow += message
                    continue
                }
                val attachment = message.attachment
                val file = attachment?.let { File(attachmentsDir(), it.id) }
                outboundQueue.addLast(
                    QueuedOutbound(
                        localId = message.id,
                        peerId = message.peerId,
                        channel = message.peerId in channelIds,
                        text = if (message.kind == MessageKind.ATTACHMENT) null else message.text,
                        attachmentMime = attachment?.mime,
                        attachmentName = attachment?.name ?: message.text,
                        attachmentBytes = attachment?.sizeBytes ?: 0,
                        attachmentDurationMs = attachment?.durationMs?.toLong(),
                        attachmentFile = file?.takeIf { it.isFile },
                    ),
                )
            }
            if (queuedRows.isNotEmpty()) {
                nextLocalMessageId = minOf(nextLocalMessageId, queuedRows.minOf { it.id } - 1)
            }
        }
        overflow.forEach { updateDeliveryAnywhere(it.id, DeliveryState.FAILED, null) }
        refreshQueuedCount()
        MeshLog.i("queued messages restored: ${outboundQueue.size} (failed overflow=${overflow.size})")
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
        val isAttachment = findMessageWithThread(id)?.third?.attachment != null
        if (isAttachment) {
            // Demo mode simulates an upload window so the progress/stall UI can be reviewed.
            scope.launch {
                delay(400)
                noteOutgoingMediaActivity(id, 1, 8)
                delay(600)
                noteOutgoingMediaActivity(id, 3, 8)
                delay(700)
                noteOutgoingMediaActivity(id, 5, 8)
                delay(600)
                noteOutgoingMediaActivity(id, 7, 8)
                delay(500)
                updateDeliveryAnywhere(id, DeliveryState.DELIVERED, channel)
                delay(1500)
                if (!channel) updateDeliveryAnywhere(id, DeliveryState.READ, channel)
            }
            return
        }
        scope.launch {
            delay(800)
            updateDeliveryAnywhere(id, DeliveryState.SENT, channel)
            delay(1200)
            updateDeliveryAnywhere(id, DeliveryState.DELIVERED, channel)
            delay(1500)
            // Demo mode simulates the peer opening/reading a direct message so READ is reviewable.
            // Channels have no per-recipient read receipts, so they stop at Delivered.
            if (!channel) updateDeliveryAnywhere(id, DeliveryState.READ, channel)
        }
    }

    /**
     * Merge a newly received copy of a message into the in-memory threads and Room.
     *
     * Inbound attachments arrive as several events for one message id (text/GroupMessage event,
     * oversized-envelope resource, attachment-payload resource) in unspecified order; see
     * [MessageMerge]. Only used for inbound events — locally sent messages replace their row.
     */
    private fun upsertReceivedMessage(peerId: String, incoming: Message, channel: Boolean): Message {
        val existing = findMessageAnywhere(peerId, incoming.id)
        val merged = MessageMerge.merge(existing, incoming)

        fun upsert(list: List<Message>): List<Message> =
            (list.filterNot { it.id == merged.id } + merged).sortedBy { it.sentAtMs }
        if (channel) {
            val thread = _channelMessages.value[peerId].orEmpty()
            _channelMessages.value = _channelMessages.value + (peerId to upsert(thread))
        } else {
            val thread = _messages.value[peerId].orEmpty()
            _messages.value = _messages.value + (peerId to upsert(thread))
            rebuildConversations()
        }
        scope.launch { dao.upsertMessage(MessageEntity.fromModel(merged)) }
        return merged
    }

    private fun findMessageAnywhere(peerId: String, id: Long): Message? =
        _channelMessages.value[peerId]?.firstOrNull { it.id == id }
            ?: _messages.value[peerId]?.firstOrNull { it.id == id }

    private fun appendMessage(peerId: String, message: Message, channel: Boolean) {
        // Upsert by message id: an own frame can be echoed back through the mesh (relay/loopback)
        // and re-delivered as an inbound event, and the UI keys messages by id. Appending blindly
        // produced duplicate ids which crashed the Compose LazyColumn ("Key ... was already used").
        fun upsert(list: List<Message>): List<Message> =
            (list.filterNot { it.id == message.id } + message).sortedBy { it.sentAtMs }
        if (channel) {
            val thread = _channelMessages.value[peerId].orEmpty()
            _channelMessages.value = _channelMessages.value + (peerId to upsert(thread))
        } else {
            val thread = _messages.value[peerId].orEmpty()
            _messages.value = _messages.value + (peerId to upsert(thread))
            rebuildConversations()
        }
    }

    private fun updateDeliveryAnywhere(id: Long, state: DeliveryState, channel: Boolean?) {
        fun applyTo(message: Message): Message =
            if (message.id == id && DeliveryPresentation.isUpgrade(message.delivery, state)) {
                val completed = state == DeliveryState.DELIVERED || state == DeliveryState.READ
                message.copy(
                    delivery = state,
                    attachment = if (completed) message.attachment?.copy(progress = 1f) else message.attachment,
                )
            } else {
                message
            }
        fun mapThreads(map: Map<String, List<Message>>): Map<String, List<Message>> =
            map.mapValues { (_, list) -> list.map(::applyTo) }
        _messages.value = mapThreads(_messages.value)
        _channelMessages.value = mapThreads(_channelMessages.value)
        rebuildConversations()
        if (state == DeliveryState.DELIVERED || state == DeliveryState.READ ||
            state == DeliveryState.FAILED || state == DeliveryState.CANCELLED
        ) {
            clearOutgoingMedia(id)
        }
        scope.launch {
            val entity = dao.messages().firstOrNull { it.id == id } ?: return@launch
            val current = runCatching { DeliveryState.valueOf(entity.delivery) }
                .getOrDefault(DeliveryState.QUEUED)
            // Status updates are monotonic: a late SENT/resource ack must not downgrade
            // DELIVERED/READ, and a stale FAILED must not override a confirmed delivery.
            if (DeliveryPresentation.isUpgrade(current, state)) {
                dao.updateDelivery(id, state.name, entity.read)
            }
        }
    }

    // ---------------------------------------------------------------- resource pacing

    /**
     * Ask the core for the duty-aware upload pace for the active profile/region and apply it to the
     * engine instance. The core paces resource chunks by wall time, so this is what keeps a fast
     * phone tick from overflowing the radio's companion path. Falls back to the core default when
     * the helper is unavailable (demo/older FFI).
     */
    private fun applyResourcePaceHint() {
        val settings = _settings.value
        val pace = runCatching {
            engine.resourcePaceIntervalMs(settings.profile, settings.region, batch = 2)
        }.getOrNull()
        if (pace != null && pace > 0L) {
            runCatching { engine.setResourcePaceIntervalMs(pace) }
            MeshLog.i(
                "resource pace: profile=${settings.profile} region=${settings.region} batch=2 -> ${pace}ms",
            )
        } else {
            MeshLog.i("resource pace: helper unavailable; keeping core default")
        }
    }

    // ---------------------------------------------------------------- public mesh / factory reset

    /**
     * Idempotently join the built-in Public Mesh channel on every engine start (fresh install,
     * restart, factory reset). The core derives the canonical channel address; the returned id must
     * match [PublicMesh.CANONICAL_CHANNEL_ID] (logged if not, so drift is visible).
     */
    private fun ensurePublicMesh() {
        if (engine.isDemo()) return
        val result = engine.joinChannel(PublicMesh.NAME, PublicMesh.PSK_BASE64URL)
        result.onSuccess { invite ->
            val id = invite.channelIdHex.lowercase()
            val channel = Channel(
                id = id,
                name = PublicMesh.NAME,
                pskSet = true,
                isPublic = true,
            )
            _channels.value = _channels.value.filterNot { it.id == id } + channel
            if (id != PublicMesh.CANONICAL_CHANNEL_ID) {
                MeshLog.w(
                    "public mesh: engine channel id=$id does not match canonical " +
                        PublicMesh.CANONICAL_CHANNEL_ID,
                )
            }
            MeshLog.i("public mesh ensured id=$id name=${PublicMesh.NAME}")
            scope.launch { dao.upsertChannel(ChannelEntity(id, PublicMesh.NAME, true, invite.inviteUrl)) }
            persistEngineState(force = true)
        }.onFailure {
            // Already-present channel can still fail on some engines; at least keep it pinned/public.
            MeshLog.w("public mesh ensure failed: ${it.message}")
            _channels.value = _channels.value.map {
                if (PublicMesh.isPublicChannel(it.id, it.name)) it.copy(isPublic = true) else it
            }
        }
    }

    /** Join URL for the Public Mesh channel (name + published PSK only; no cosmetic id). */
    fun publicMeshJoinUrl(): String = PublicMesh.JOIN_URL

    /**
     * Full factory reset: stop the engine/radio, wipe Room + prefs + engine state + identity keys,
     * attachments and the frame-sequence floor, then leave the app in first-run/onboarding state.
     * The next onboarding/start generates a NEW identity (node id/QR changes; old contacts/QRs are
     * invalid) and [ensurePublicMesh] re-creates the public channel.
     */
    fun factoryReset() {
        MeshLog.i("factory reset: begin (wipe DB/prefs/identity/state)")
        factoryResetInProgress = true
        synchronized(startedLock) { started = false }
        _engineReady.value = false
        tickJob?.cancel()
        tickJob = null
        engineJobs.forEach { it.cancel() }
        engineJobs = mutableListOf()
        companionJob?.cancel()
        companionJob = null
        scope.launch {
            runCatching { engine.stop() }
            runCatching { disconnectCompanion() }
            synchronized(queueLock) {
                outboundQueue.clear()
                nextLocalMessageId = -1L
            }
            runCatching { database.clearAllTables() }
            runCatching { File(appContext.filesDir, "attachments").deleteRecursively() }
            runCatching { identityStore.clear(prefs) }
            runCatching { prefs.clearAll() }
            seqFloorStore.clear()
            latestPrefs = org.openmesh.android.data.prefs.LocalPrefs()
            // Reset in-memory state so no old rows/uploads/floors survive in this process.
            _settings.value = AppSettings()
            _nodes.value = emptyList()
            _contacts.value = emptyList()
            _channels.value = emptyList()
            _messages.value = emptyMap()
            _channelMessages.value = emptyMap()
            _conversations.value = emptyList()
            _outgoingMedia.value = emptyMap()
            _outgoingMediaUi.value = emptyMap()
            _radio.value = RadioStatus()
            _stats.value = CoreStats()
            _engineStatus.value = CoreStatus()
            _selfNodeId.value = null
            _selfIdentity.value = null
            lastSeqFloorPersistMs = 0L
            lastStatePersistMs = 0L
            factoryResetInProgress = false
            runCatching {
                appContext.stopService(android.content.Intent(appContext, org.openmesh.android.service.MeshService::class.java))
            }
            _uiEvents.tryEmit(
                UiEvent.Snackbar("Factory reset complete — set up your node again (new identity/QR)."),
            )
            MeshLog.i("factory reset: complete (fresh identity on next start; public mesh will be re-added)")
        }
    }

    // ---------------------------------------------------------------- outgoing media uploads

    private fun expectedMediaWindowMs(sizeBytes: Int): Long =
        runCatching { estimateMedia(sizeBytes).wallClockMs }.getOrDefault(0L)

    private fun trackOutgoingMedia(message: Message, channel: Boolean) {
        val attachment = message.attachment ?: return
        if (!message.outgoing || message.kind != MessageKind.ATTACHMENT) return
        val existing = _outgoingMedia.value[message.id]
        _outgoingMedia.value = _outgoingMedia.value + (
            message.id to OutgoingMediaPolicy.initial(
                isChannel = channel,
                mime = attachment.mime,
                sizeBytes = attachment.sizeBytes,
                sentAtMs = message.sentAtMs,
                expectedWindowMs = expectedMediaWindowMs(attachment.sizeBytes),
            ).copy(attempts = existing?.attempts ?: 1)
            )
        refreshOutgoingMediaUi()
    }

    private fun noteOutgoingMediaActivity(messageId: Long, received: Int, total: Int) {
        val snapshot = _outgoingMedia.value[messageId] ?: return
        val progress = received.toFloat() / total.coerceAtLeast(1)
        _outgoingMedia.value = _outgoingMedia.value + (
            messageId to snapshot.copy(
                progress = progress,
                lastActivityMs = System.currentTimeMillis(),
            )
            )
        refreshOutgoingMediaUi()
    }

    private fun clearOutgoingMedia(messageId: Long) {
        if (!_outgoingMedia.value.containsKey(messageId)) return
        _outgoingMedia.value = _outgoingMedia.value - messageId
        refreshOutgoingMediaUi()
    }

    private fun refreshOutgoingMediaUi() {
        val now = System.currentTimeMillis()
        // One-time backoff scheduling per stall: show "Retrying in N s" before the automatic attempt.
        var snapshotsChanged = false
        val withSchedules = _outgoingMedia.value.mapValues { (_, snapshot) ->
            val ui = OutgoingMediaPolicy.uiState(snapshot, now)
            val shouldSchedule = ui.state == OutgoingMediaUiState.STALLED &&
                OutgoingMediaPolicy.autoRetryAllowed(snapshot) &&
                snapshot.nextAutoRetryAtMs == null
            if (shouldSchedule) {
                snapshotsChanged = true
                OutgoingMediaPolicy.scheduleAutoRetry(snapshot, now)
            } else {
                snapshot
            }
        }
        if (snapshotsChanged) _outgoingMedia.value = withSchedules
        val next = withSchedules.mapValues { (_, snapshot) ->
            OutgoingMediaPolicy.uiState(snapshot, now)
        }
        if (next != _outgoingMediaUi.value) _outgoingMediaUi.value = next
    }

    /**
     * Start at most one automatic retry per tick, only on a READY link and only when no other
     * resource transfer is in flight. Without these guards a batch of stale uploads retried on
     * app restart can flood the companion TX path and starve window ACKs for concurrent inbound
     * transfers (observed on device).
     */
    private fun maybeAutoRetryStalledUploads() {
        if (autoRetryInFlight) return
        if (_radio.value.phase != RadioPhase.READY) return
        val now = System.currentTimeMillis()
        val transferInFlight = now - lastResourceActivityMs < OutgoingMediaPolicy.TRANSFER_IN_FLIGHT_WINDOW_MS
        if (!OutgoingMediaPolicy.canStartAutoRetry(
                radioReady = true,
                transferInFlight = transferInFlight,
                anotherAutoRetryRunning = autoRetryInFlight,
            )
        ) {
            return
        }
        val candidateId = OutgoingMediaPolicy.pickAutoRetryCandidate(_outgoingMedia.value, now) ?: return
        val snapshot = _outgoingMedia.value[candidateId] ?: return
        val marked = OutgoingMediaPolicy.markAutoRetryStarted(snapshot, now)
        _outgoingMedia.value = _outgoingMedia.value + (candidateId to marked)
        refreshOutgoingMediaUi()
        autoRetryInFlight = true
        MeshLog.i(
            "auto-retry upload id=$candidateId attempt=${marked.autoRetries}/" +
                "${OutgoingMediaPolicy.MAX_AUTO_RETRIES} (manual retry required afterwards)",
        )
        retryUpload(candidateId, auto = true)
    }

    /**
     * Retry a stalled/failed outgoing attachment without duplicating its row.
     *
     * The core's `retry_message` only retransmits a still-pending *announcing message*; it cannot
     * restart a stalled resource transfer. So this path re-sends the stored bytes through the normal
     * attachment API as a new attempt and atomically replaces the old local row (delete old id +
     * insert the new engine id) so the user still sees exactly one bubble. Message-id semantics on
     * the wire are unchanged; only this local row's technical id moves to the new attempt.
     */
    fun retryUpload(messageId: Long, auto: Boolean = false) {
        val found = findMessageWithThread(messageId) ?: return
        val (peerId, channel, message) = found
        if (!message.outgoing || message.kind != MessageKind.ATTACHMENT) return
        if (message.delivery == DeliveryState.DELIVERED ||
            message.delivery == DeliveryState.READ ||
            message.delivery == DeliveryState.CANCELLED
        ) {
            return
        }
        if (shouldQueueLocally()) {
            _uiEvents.tryEmit(UiEvent.Snackbar("Connect a radio to retry this upload."))
            return
        }
        val attachment = message.attachment ?: return
        val file = attachmentFile(attachment.id)?.takeIf { it.isFile }
        if (file == null) {
            _uiEvents.tryEmit(UiEvent.Snackbar("The attachment file is not on this phone, so it can't be retried."))
            return
        }
        val now = System.currentTimeMillis()
        val previous = _outgoingMedia.value[messageId]
            ?: OutgoingMediaPolicy.initial(
                isChannel = channel,
                mime = attachment.mime,
                sizeBytes = attachment.sizeBytes,
                sentAtMs = message.sentAtMs,
                expectedWindowMs = expectedMediaWindowMs(attachment.sizeBytes),
            )
        val marked = if (auto) {
            OutgoingMediaPolicy.markAutoRetryStarted(previous, now)
        } else {
            OutgoingMediaPolicy.markManualRetry(previous, now)
        }
        _outgoingMedia.value = _outgoingMedia.value + (messageId to marked)
        refreshOutgoingMediaUi()
        updateDeliveryAnywhere(messageId, DeliveryState.SENT, channel)
        lastResourceActivityMs = now
        if (!auto) {
            _uiEvents.tryEmit(UiEvent.Snackbar("Retrying upload — starting a fresh attempt."))
        }
        scope.launch {
            try {
            val bytes = runCatching { file.readBytes() }.getOrNull()
            if (bytes == null) {
                _uiEvents.tryEmit(UiEvent.Snackbar("Could not read the attachment file; retry failed."))
                clearOutgoingMedia(messageId)
                return@launch
            }
            val target = if (channel) Target.Channel(peerId) else Target.Direct(peerId)
            engine.sendAttachment(
                target = target,
                mime = attachment.mime,
                filename = attachment.name,
                data = bytes,
                durationMs = attachment.durationMs?.toLong(),
            ).onSuccess { newId ->
                replaceMessageForRetry(peerId, channel, message, bytes, newId)
            }.onFailure { error ->
                MeshLog.w("retry upload failed id=$messageId: ${error.message}")
                updateDeliveryAnywhere(messageId, DeliveryState.FAILED, channel)
                clearOutgoingMedia(messageId)
                _uiEvents.tryEmit(UiEvent.Error("Retry failed: ${error.message ?: "radio busy"}"))
            }
            } finally {
                if (auto) autoRetryInFlight = false
            }
        }
    }

    private fun replaceMessageForRetry(
        peerId: String,
        channel: Boolean,
        old: Message,
        bytes: ByteArray,
        newId: Long,
    ) {
        if (newId == old.id) return
        val now = System.currentTimeMillis()
        val oldAttachmentId = old.attachment?.id
        val newFile = File(attachmentsDir(), "att_$newId")
        val written = runCatching { newFile.writeBytes(bytes) }.isSuccess
        val attachment = old.attachment?.copy(
            id = "att_$newId",
            sizeBytes = bytes.size,
            progress = 0f,
        )
        val replacement = old.copy(
            id = newId,
            sentAtMs = now,
            delivery = DeliveryState.SENT,
            attachment = attachment,
        )
        val priorSnapshot = _outgoingMedia.value[old.id]
        val priorAttempts = priorSnapshot?.attempts ?: 1
        val priorAutoRetries = priorSnapshot?.autoRetries ?: 0
        removeMessageEverywhere(old.id)
        if (oldAttachmentId != null) runCatching { File(attachmentsDir(), oldAttachmentId).delete() }
        appendMessage(peerId, replacement, channel)
        if (attachment != null) {
            _outgoingMedia.value = _outgoingMedia.value - old.id + (
                newId to OutgoingMediaPolicy.initial(
                    isChannel = channel,
                    mime = attachment.mime,
                    sizeBytes = attachment.sizeBytes,
                    sentAtMs = now,
                    expectedWindowMs = expectedMediaWindowMs(attachment.sizeBytes),
                ).copy(attempts = priorAttempts + 1, autoRetries = priorAutoRetries)
                )
            refreshOutgoingMediaUi()
        }
        scope.launch {
            runCatching { dao.deleteMessage(old.id) }
            runCatching { dao.upsertMessage(MessageEntity.fromModel(replacement)) }
        }
        persistEngineState()
        persistSequenceFloor(force = true)
        MeshLog.i(
            "outgoing media retried old_id=${old.id} new_id=$newId bytes=${bytes.size} " +
                "written=$written channel=$channel",
        )
    }

    /**
     * Cancel an outgoing send (in-flight attachment resource or a locally queued message).
     *
     * Sets the terminal [DeliveryState.CANCELLED] immediately when the engine confirms, clears the
     * upload tracker (no auto retry) and persists. The engine's MessageCancelled event is
     * idempotent confirmation on the next tick. Cancelled rows reject Retry.
     */
    fun cancelUpload(messageId: Long) {
        val found = findMessageWithThread(messageId) ?: return
        val (peerId, channel, message) = found
        if (!message.outgoing) return
        if (message.delivery == DeliveryState.DELIVERED ||
            message.delivery == DeliveryState.READ ||
            message.delivery == DeliveryState.CANCELLED
        ) {
            _uiEvents.tryEmit(UiEvent.Snackbar("This send already finished — it can't be cancelled."))
            return
        }
        if (messageId < 0L) {
            // A locally queued message the engine never saw: drop it from the queue and settle it.
            val removed = synchronized(queueLock) {
                outboundQueue.removeAll { it.localId == messageId }
            }
            if (removed) {
                updateDeliveryAnywhere(messageId, DeliveryState.CANCELLED, channel)
                clearOutgoingMedia(messageId)
                refreshQueuedCount()
                persistEngineState(force = true)
                MeshLog.i("queued send cancelled local_id=$messageId channel=$channel")
                _uiEvents.tryEmit(UiEvent.Snackbar("Queued send cancelled."))
            }
            return
        }
        val cancelled = runCatching { engine.cancelMessage(messageId) }.getOrDefault(false)
        if (cancelled) {
            updateDeliveryAnywhere(messageId, DeliveryState.CANCELLED, channel)
            clearOutgoingMedia(messageId)
            persistEngineState(force = true)
            MeshLog.i("send cancelled msg_id=$messageId channel=$channel")
            _uiEvents.tryEmit(UiEvent.Snackbar("Send cancelled — no more airtime is used."))
        } else {
            MeshLog.i("send cancel declined msg_id=$messageId (already complete/unknown)")
            _uiEvents.tryEmit(UiEvent.Snackbar("This send already finished — it can't be cancelled."))
        }
    }

    private fun findMessageWithThread(id: Long): Triple<String, Boolean, Message>? {
        _messages.value.forEach { (peerId, list) ->
            list.firstOrNull { it.id == id }?.let { return Triple(peerId, false, it) }
        }
        _channelMessages.value.forEach { (peerId, list) ->
            list.firstOrNull { it.id == id }?.let { return Triple(peerId, true, it) }
        }
        return null
    }

    private fun rebuildConversations() {
        val contacts = _contacts.value
        _conversations.value = _messages.value.map { (peerId, thread) ->
            val contact = contacts.firstOrNull { it.nodeId == peerId }
            Conversation(
                peerId = peerId,
                peerName = displayNameFor(peerId),
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
                val incoming = event.message
                if (incoming.outgoing) return
                val channel = incoming.peerId in _channels.value.map { it.id }
                val firstEventForMessage = findMessageAnywhere(incoming.peerId, incoming.id) == null
                val merged = upsertReceivedMessage(incoming.peerId, incoming, channel)
                MeshLog.i(
                    "rx message id=${incoming.id} channel=$channel first=$firstEventForMessage " +
                        "kind=${merged.kind} attachment=${merged.attachment?.mime} text=${merged.text?.take(32)}",
                )
                if (firstEventForMessage) {
                    if (channel) {
                        _channels.value = _channels.value.map {
                            if (it.id == incoming.peerId) it.copy(unreadCount = it.unreadCount + 1) else it
                        }
                    }
                    if (_settings.value.notificationsEnabled) {
                        val body = when {
                            merged.attachment?.mime?.startsWith("image/") == true -> "📷 Photo"
                            merged.attachment?.mime?.startsWith("audio/") == true -> "🎤 Voice note"
                            else -> merged.text ?: "New message"
                        }
                        Notifications.showMessage(
                            appContext,
                            peerId = incoming.peerId,
                            sender = if (channel) displayNameFor(incoming.peerId) else displayNameFor(incoming.fromId),
                            body = body,
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
                lastResourceActivityMs = System.currentTimeMillis()
                MeshLog.i(
                    "resource progress id=${event.messageId} chunk=${event.received}/${event.total} " +
                        "outgoing=${_outgoingMedia.value.containsKey(event.messageId)}",
                )
                if (_outgoingMedia.value.containsKey(event.messageId)) {
                    noteOutgoingMediaActivity(event.messageId, event.received, event.total)
                }
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
                lastResourceActivityMs = System.currentTimeMillis()
                val channel = event.channelId != null && event.channelId in _channels.value.map { it.id }
                val peerId = event.channelId ?: event.peerId
                MeshLog.i(
                    "rx resource id=${event.messageId} mime=${event.mime} bytes=${event.data.size} " +
                        "filename=${event.filename} duration=${event.durationMs} channel=$channel",
                )
                // Oversized group-message envelopes arrive as a LongText resource with this mime.
                // They are transport artifacts, not user attachments: both this envelope and the
                // real payload use files/attachments/att_<msgId>, so persisting the envelope would
                // overwrite the payload (the old bug) and create a bogus CBOR "attachment" row.
                // The GroupMessage event carries the message text; the payload resource carries
                // the real mime/size. Ordering-independent thanks to MessageMerge.
                if (event.mime.startsWith("application/cbor", ignoreCase = true)) {
                    MeshLog.i("rx resource: ignoring cbor envelope id=${event.messageId} bytes=${event.data.size}")
                    return
                }
                val existing = findMessageAnywhere(peerId, event.messageId)
                val file = File(attachmentsDir(), "att_${event.messageId}")
                if (runCatching { file.writeBytes(event.data) }.isFailure) {
                    MeshLog.w("rx resource: could not store id=${event.messageId} bytes=${event.data.size}")
                }
                val incoming = Message(
                    id = event.messageId,
                    peerId = peerId,
                    fromId = event.peerId,
                    toId = peerId,
                    kind = MessageKind.ATTACHMENT,
                    text = event.filename,
                    sentAtMs = System.currentTimeMillis(),
                    receivedAtMs = System.currentTimeMillis(),
                    outgoing = false,
                    delivery = DeliveryState.DELIVERED,
                    attachment = Attachment(
                        id = "att_${event.messageId}",
                        mime = event.mime,
                        sizeBytes = event.data.size,
                        durationMs = event.durationMs?.toInt(),
                        name = event.filename,
                        progress = 1f,
                    ),
                )
                val merged = upsertReceivedMessage(peerId, incoming, channel)
                MeshLog.i(
                    "rx attachment stored id=${event.messageId} file=${file.name} " +
                        "merged_kind=${merged.kind} mime=${merged.attachment?.mime} size=${merged.attachment?.sizeBytes}",
                )
                if (existing == null && _settings.value.notificationsEnabled) {
                    val body = if (event.mime.startsWith("image/")) "📷 Photo" else if (event.mime.startsWith("audio/")) "🎤 Voice note" else "Attachment"
                    Notifications.showMessage(
                        appContext,
                        peerId = peerId,
                        sender = if (channel) displayNameFor(peerId) else displayNameFor(event.peerId),
                        body = body,
                        isChannel = channel,
                    )
                }
            }

            is CoreEvent.StoreOffer -> _uiEvents.tryEmit(
                UiEvent.Snackbar("${displayNameFor(event.relayId)} holds ${event.count} message(s) for you"),
            )

            is CoreEvent.CoreError -> {
                MeshLog.w("core error: ${event.message}")
                _uiEvents.tryEmit(UiEvent.Snackbar(event.message))
            }
        }
    }

    // ---------------------------------------------------------------- companion link

    fun connectCompanion(kind: CompanionKind) {
        if (kind == CompanionKind.NONE) {
            // ACTION_START from the foreground service must not tear down a link that the engine
            // auto-reconnect or the user just started (that race cancelled the connect coroutine),
            // nor hide a persistent connection error the user still has to act on.
            val keepLink = _radio.value.connected ||
                companionJob?.isActive == true ||
                _radio.value.phase == RadioPhase.ERROR
            if (keepLink) return
            disconnectCompanion()
            return
        }
        if (companionKind == kind && _radio.value.connected) return
        userDisconnect = false
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
        companionAttempt += 1
        MeshLog.i("radio connect: kind=$kind attempt=$companionAttempt")
        client.start(scope)
        scope.launch { prefs.setRadioKind(if (kind == CompanionKind.USB) "usb" else "ble") }
        // A new attempt starts from a clean slate: clear the previous error, keep queued messages.
        updateRadio {
            it.copy(
                phase = RadioPhase.NONE,
                phaseMessage = "",
                deviceName = null,
                deviceAddress = null,
                devicesFound = 0,
                lastError = null,
                lastErrorHint = null,
                lastErrorKind = null,
                phaseSinceMs = System.currentTimeMillis(),
                health = RadioHealth.UNKNOWN,
                transportName = newTransport.label,
                txQueue = 0,
                airtimePct = 0f,
                rssi = null,
                snr = null,
                lastReplyMs = 0L,
            )
        }
        companionJob = scope.launch {
            val progressJob = launch {
                newTransport.progress.collect { progress -> applyTransportProgress(progress) }
            }
            try {
                val result = newTransport.connect()
                if (result.isFailure) {
                    progressJob.cancel()
                    // The transport already published the specific failure through [progress];
                    // prefer that reason over the thrown exception text.
                    val snapshot = newTransport.progress.value
                    val errorKind = snapshot.error ?: LinkError.UNKNOWN
                    val message = result.exceptionOrNull()?.message?.takeIf { it.isNotBlank() }
                        ?: snapshot.message.ifBlank { "Could not connect to the radio." }
                    failCompanion(errorKind, message)
                    closeCompanionResources(newTransport, client)
                    return@launch
                }
                // Transport is connected; the companion client has sent Hello and waits for HelloAck.
                val transportProgress = newTransport.progress.value
                applyTransportProgress(
                    if (transportProgress.phase == RadioPhase.HANDSHAKING) {
                        transportProgress
                    } else {
                        transportProgress.copy(
                            phase = RadioPhase.HANDSHAKING,
                            message = "Radio found. Waiting for the handshake…",
                            sinceMs = System.currentTimeMillis(),
                        )
                    },
                )
                // READY only after HelloAck — a GATT/USB write channel alone is not "connected".
                val session = withTimeoutOrNull(HANDSHAKE_TIMEOUT_MS) {
                    client.session.filter { it.ready || it.lastError != null }.first()
                }
                if (session == null || !session.ready) {
                    progressJob.cancel()
                    failCompanion(
                        LinkError.HANDSHAKE_TIMEOUT,
                        session?.lastError
                            ?: "The radio did not finish the handshake. Its firmware may be out of date.",
                    )
                    closeCompanionResources(newTransport, client)
                    return@launch
                }
                val hello = session.hello
                val now = System.currentTimeMillis()
                updateRadio {
                    it.copy(
                        phase = RadioPhase.READY,
                        phaseMessage = "Connected and answering",
                        firmwareVersion = hello?.firmwareVersion ?: it.firmwareVersion,
                        nodeId = hello?.nodeIdHex ?: it.nodeId,
                        region = hello?.region ?: it.region,
                        profile = hello?.profile ?: it.profile,
                        lastError = null,
                        lastErrorHint = null,
                        lastErrorKind = null,
                        phaseSinceMs = now,
                        health = RadioHealth.HEALTHY,
                        lastReplyMs = now,
                        deviceName = it.deviceName ?: "OpenMesh radio",
                    )
                }
                MeshLog.i(
                    "radio ready: fw=${hello?.firmwareVersion} node=${hello?.nodeIdHex} " +
                        "profile=${hello?.profile}/${hello?.region}",
                )
                _uiEvents.tryEmit(UiEvent.Snackbar("Radio ready: ${hello?.firmwareVersion ?: "connected"}"))
                // Announce once the companion session is up. The engine's startup announce fires
                // before a radio is linked (and is dropped), so peers/bridge never learned our
                // identity/public key otherwise.
                scope.launch {
                    delay(1_000)
                    runCatching { engine.announce(System.currentTimeMillis()) }
                        .onFailure { MeshLog.w("announce after radio ready failed: ${it.message}") }
                }
                pushRadioConfig(client)
                maybeFlushQueuedMessages()
                // Detect a link that dies after READY (radio out of range, cable pulled, BLE dropped).
                val linkWatcher = launch {
                    newTransport.connected.collect { connected ->
                        if (!connected && !userDisconnect && _radio.value.phase == RadioPhase.READY) {
                            MeshLog.w("radio link lost after ready (kind=$kind)")
                            failCompanion(
                                LinkError.LINK_LOST,
                                "Lost connection to the radio. It may be out of range or switched off.",
                            )
                            companionJob?.cancel()
                        }
                    }
                }
                try {
                    client.events.collect { event -> handleCompanionEvent(event) }
                } finally {
                    linkWatcher.cancel()
                }
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (e: Exception) {
                MeshLog.e("radio companion loop failed", e)
                failCompanion(LinkError.UNKNOWN, e.message ?: "The radio link stopped unexpectedly.")
                closeCompanionResources(newTransport, client)
            } finally {
                progressJob.cancel()
            }
        }
    }

    private fun closeCompanionResources(t: CompanionTransport, client: FramedCompanionClient) {
        scope.launch {
            runCatching { client.stop() }
            runCatching { t.disconnect() }
        }
    }

    private fun handleCompanionEvent(event: FramedCompanionClient.CompanionEvent) {
        markRadioReply()
        when (event) {
            is FramedCompanionClient.CompanionEvent.Rx -> {
                val ingested = engine.ingestFrame(event.frame, System.currentTimeMillis())
                if (ingested.isFailure) {
                    MeshLog.w("engine ingest failed bytes=${event.frame.size}: ${ingested.exceptionOrNull()?.message}")
                }
            }

            is FramedCompanionClient.CompanionEvent.Stats -> updateRadio {
                it.copy(
                    txQueue = event.entries["tx_queue"]?.toInt() ?: it.txQueue,
                    airtimePct = event.entries["airtime_used_pct"]?.toFloat() ?: it.airtimePct,
                    rssi = event.entries["last_rssi"]?.toInt() ?: it.rssi,
                )
            }

            is FramedCompanionClient.CompanionEvent.Config -> {
                val entries = event.entries
                // The app owns the display name. A radio Config echo can carry the pre-rename
                // value for a moment while the new name is being pushed, and adopting it would
                // silently revert the user's rename (seen live on the test phone). Node name is
                // therefore never taken from the radio config; radio-owned keys are still adopted.
                updateSettings {
                    it.copy(
                        profile = entries["radio.profile"] ?: it.profile,
                        region = entries["radio.region"] ?: it.region,
                        storeForward = (entries["mesh.store"] ?: "1") == "1",
                    )
                }
            }

            is FramedCompanionClient.CompanionEvent.RadioEvent ->
                _uiEvents.tryEmit(UiEvent.Snackbar("Radio: ${event.kind} ${event.detail}"))

            is FramedCompanionClient.CompanionEvent.Failure ->
                failCompanion(LinkError.UNKNOWN, event.message)

            is FramedCompanionClient.CompanionEvent.HelloReceived,
            is FramedCompanionClient.CompanionEvent.Pong,
            -> Unit
        }
    }

    /** Applies transport-published progress (scan/connect/handshake/error) to [RadioStatus]. */
    private fun applyTransportProgress(progress: LinkProgress) {
        if (progress.phase == RadioPhase.NONE) return
        updateRadio { current ->
            val newPhase = current.phase != progress.phase
            current.copy(
                phase = progress.phase,
                phaseMessage = progress.message,
                deviceName = progress.deviceName ?: current.deviceName,
                deviceAddress = progress.deviceAddress ?: current.deviceAddress,
                rssi = progress.rssi ?: current.rssi,
                devicesFound = progress.devicesFound,
                phaseSinceMs = if (newPhase) progress.sinceMs else current.phaseSinceMs,
                health = if (progress.phase == RadioPhase.ERROR) RadioHealth.UNKNOWN else current.health,
                lastError = if (progress.phase == RadioPhase.ERROR) progress.message else current.lastError,
                lastErrorHint = if (progress.phase == RadioPhase.ERROR) {
                    RadioErrorHints.hintFor(progress.error ?: LinkError.UNKNOWN)
                } else {
                    current.lastErrorHint
                },
                lastErrorKind = if (progress.phase == RadioPhase.ERROR) {
                    progress.error ?: LinkError.UNKNOWN
                } else {
                    current.lastErrorKind
                },
            )
        }
    }

    /** Makes a link failure persistent: visible on the banner, radio screen and notification. */
    private fun failCompanion(error: LinkError, message: String) {
        MeshLog.w("radio error: kind=$error message=$message")
        updateRadio {
            it.copy(
                phase = RadioPhase.ERROR,
                phaseMessage = message,
                lastError = message,
                lastErrorHint = RadioErrorHints.hintFor(error),
                lastErrorKind = error,
                phaseSinceMs = System.currentTimeMillis(),
                health = RadioHealth.UNKNOWN,
            )
        }
    }

    /** Records an inbound companion message so the health ticker can tell a live link from a dead one. */
    private fun markRadioReply(nowMs: Long = System.currentTimeMillis()) {
        val current = _radio.value
        _radio.value = if (current.phase == RadioPhase.READY) {
            current.copy(lastReplyMs = nowMs, health = RadioHealth.HEALTHY)
        } else {
            current.copy(lastReplyMs = nowMs)
        }
    }

    /** Enforces the invariant `connected == (phase == READY)` for real links. */
    private fun updateRadio(transform: (RadioStatus) -> RadioStatus) {
        val next = transform(_radio.value)
        _radio.value = next.copy(connected = next.phase == RadioPhase.READY)
    }

    fun disconnectCompanion() {
        userDisconnect = true
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
        updateRadio {
            it.copy(
                phase = RadioPhase.NONE,
                phaseMessage = "",
                deviceName = null,
                deviceAddress = null,
                devicesFound = 0,
                phaseSinceMs = System.currentTimeMillis(),
                health = RadioHealth.UNKNOWN,
                rssi = null,
                snr = null,
                lastReplyMs = 0L,
            )
        }
    }

    val companionConnected: StateFlow<RadioStatus> = _radio

    fun currentCompanionKind(): CompanionKind = companionKind

    // ---------------------------------------------------------------- contacts / invites / channels

    /**
     * Persist the engine state (frame sequence floor, message-id counters, contacts/channels).
     *
     * Triggered after sends/frames (debounced to at most every [EngineStatePersistence.SEND_DEBOUNCE_MS]),
     * periodically while running, after channel/contact mutations (force) and from lifecycle stop.
     * Without this a process restart restores a stale `last_seq`, reuses `(src, seq)` numbers and the
     * bridge/peer silently deduplicates the phone's frames for its dedup TTL (the phone→PC media bug).
     */
    private fun persistEngineState(force: Boolean = false) {
        if (factoryResetInProgress) return
        val now = System.currentTimeMillis()
        if (!force && !EngineStatePersistence.shouldPersistAfterSend(lastStatePersistMs, now)) return
        lastStatePersistMs = now
        scope.launch { persistEngineStateNow() }
    }

    private suspend fun persistEngineStateNow() {
        val export = runCatching { engine.exportState() }.getOrNull()
            ?.takeIf { it.isNotEmpty() }
            ?: return
        prefs.setEngineState(export)
        MeshLog.i(
            "engine state persisted: bytes=${export.size} " +
                "seq_floor=${EngineStatePersistence.sequenceFloorFromState(export)}",
        )
    }

    /** Called from the activity/service lifecycle: persist immediately (best effort). */
    fun persistStateOnLifecycleStop() {
        if (!_engineReady.value) return
        MeshLog.i("engine state persist: lifecycle stop")
        persistSequenceFloor(force = true)
        persistEngineState(force = true)
    }

    /**
     * Persist the engine's current frame sequence number as a cheap floor (SharedPreferences).
     * Forced after sends/stops; throttled to ~1/s while frames flow. Never writes a lower value.
     */
    private fun persistSequenceFloor(force: Boolean = false) {
        if (factoryResetInProgress) return
        val seq = runCatching { engine.lastSeq() }.getOrNull()?.takeIf { it > 0L } ?: return
        val now = System.currentTimeMillis()
        if (!force && !EngineStatePersistence.shouldPersistSequenceFloor(lastSeqFloorPersistMs, now)) return
        val previous = seqFloorStore.read()
        lastSeqFloorPersistMs = now
        if (seq <= previous) return
        seqFloorStore.write(seq)
        MeshLog.i("frame sequence floor persisted: lastSeq=$seq (was $previous)")
    }


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
                val channel = Channel(
                    channelId,
                    invite.name,
                    pskSet = invite.pskBase64.isNotEmpty(),
                    isPublic = PublicMesh.isPublicChannel(channelId, invite.name),
                )
                if (_channels.value.none { it.id == channelId }) {
                    _channels.value = _channels.value + channel
                }
                scope.launch {
                    dao.upsertChannel(ChannelEntity(channelId, invite.name, channel.pskSet, joined.getOrNull()?.inviteUrl))
                }
            }
        }
        persistEngineState(force = true)
        _uiEvents.tryEmit(UiEvent.InviteImported(invite.name, invite.kind == QrInvite.Kind.CHANNEL))
        invite
    }

    fun createChannel(name: String): Result<ChannelInvite> {
        val result = engine.createChannel(name)
        result.onFailure { MeshLog.w("channel create failed: ${it.message}") }
        result.getOrNull()?.let { invite ->
            MeshLog.i("channel created ${invite.channelIdHex} name=${invite.name}")
            _lastChannelInvite.value = invite.inviteUrl
            val channel = Channel(
                invite.channelIdHex,
                invite.name,
                isPublic = PublicMesh.isPublicChannel(invite.channelIdHex, invite.name),
            )
            _channels.value = _channels.value.filterNot { it.id == channel.id } + channel
            scope.launch { dao.upsertChannel(ChannelEntity(channel.id, channel.name, true, invite.inviteUrl)) }
            persistEngineState(force = true)
        }
        return result
    }

    fun joinChannel(name: String, pskBase64: String): Result<ChannelInvite> {
        val result = engine.joinChannel(name, pskBase64)
        result.getOrNull()?.let { invite ->
            _lastChannelInvite.value = invite.inviteUrl
            val channel = Channel(
                invite.channelIdHex,
                invite.name,
                isPublic = PublicMesh.isPublicChannel(invite.channelIdHex, invite.name),
            )
            _channels.value = _channels.value.filterNot { it.id == channel.id } + channel
            scope.launch { dao.upsertChannel(ChannelEntity(channel.id, channel.name, true, invite.inviteUrl)) }
            persistEngineState(force = true)
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
            if (updated.demoMode != previous.demoMode) prefs.setDemoMode(updated.demoMode)
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
        val nameChanged = updated.nodeName != previous.nodeName
        if (started) {
            val patch = """{"name":${jsonString(updated.nodeName)},"profile":"${updated.profile}","region":"${updated.region}","storeForward":${updated.storeForward},"announceIntervalS":${updated.announceIntervalSec.toLong()}}"""
            runCatching { engine.setConfig(patch) }
            val radioChanged = updated.profile != previous.profile || updated.region != previous.region
            if (radioChanged || nameChanged) {
                companion?.let { client -> pushRadioConfig(client) }
            }
            if (radioChanged) applyResourcePaceHint()
            if (nameChanged) {
                // A rename should reach peers now, not at the next 30-min announce cycle.
                MeshLog.i("display name changed: '${previous.nodeName}' -> '${updated.nodeName}'; announcing now")
                runCatching { engine.announce(System.currentTimeMillis()) }
                    .onFailure { MeshLog.w("announce after rename failed: ${it.message}") }
                _uiEvents.tryEmit(
                    UiEvent.Snackbar("Display name saved — announcing “${updated.nodeName}” to the mesh"),
                )
            }
        }
    }

    /**
     * Validate and save a new display name. The name travels in announces (32-byte limit); the
     * node id stays permanent and unique, so changing the name never changes identity or keys.
     */
    fun saveDisplayName(raw: String): Result<String> {
        val validation = NamePolicy.validate(raw)
        val name = validation.value
        if (!validation.ok || name == null) {
            return Result.failure(IllegalArgumentException(validation.error ?: "Enter a display name."))
        }
        if (name == _settings.value.nodeName) return Result.success(name)
        updateSettings { it.copy(nodeName = name) }
        MeshLog.i("display name saved: '$name' (bytes=${name.toByteArray(Charsets.UTF_8).size})")
        return Result.success(name)
    }

    fun setUiMode(mode: UiMode) = updateSettings { it.copy(uiMode = mode.wire) }

    /**
     * Toggle the simulated mesh. Demo mode changes which [EngineBridge] implementation runs, so
     * restart the engine after persisting the flag: otherwise the UI says "demo" while real
     * messages try to reach a radio (or get queued for one).
     */
    fun setDemoMode(enabled: Boolean) {
        if (_settings.value.demoMode == enabled) return
        updateSettings { it.copy(demoMode = enabled) }
        scope.launch {
            prefs.setDemoMode(enabled)
            if (enabled) {
                // Demo mode is a simulated mesh: drop any real link and show the demo radio status.
                _radio.value = DemoData.radioStatus
                disconnectCompanion()
            } else {
                // The demo seeds live only in memory; going back to the real engine must restore
                // the persisted conversations/contacts/nodes, otherwise demo chats keep showing.
                loadFromDatabase()
                rebuildConversations()
            }
            if (started) startEngine()
        }
    }

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

    /**
     * Encode RGBA8 pixels to a complete AVIF file through the shared Rust encoder. CPU-bound:
     * callers must use a background dispatcher (the tier dialog uses Dispatchers.IO). Null means
     * fall back to the platform WebP/JPEG encoder and label the payload honestly.
     */
    fun encodeAvif(rgba: ByteArray, width: Int, height: Int, quality: Int, speed: Int): ByteArray? =
        runCatching { engine.encodeAvif(rgba, width, height, quality, speed) }.getOrNull()

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
