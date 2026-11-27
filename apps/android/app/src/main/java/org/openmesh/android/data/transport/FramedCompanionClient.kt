package org.openmesh.android.data.transport

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.openmesh.android.util.Hex
import org.openmesh.android.util.MeshLog

/**
 * Companion API client: SLIP de-framing, CRC check, CBOR payloads, HELLO handshake and
 * command/event exchange (docs/architecture/COMPANION-API.md).
 *
 * The class is deliberately transport-agnostic: USB serial, BLE and a fake in-memory radio can
 * all drive it. It never interprets OMP frames; [CompanionEvent.Rx] hands them to the
 * repository/core for processing.
 */
class FramedCompanionClient(
    private val transport: CompanionTransport,
    private val clientVersion: String = "openmesh-android/0.1.0",
) {
    object Type {
        const val HELLO = 1
        const val HELLO_ACK = 2
        const val CONFIG_GET = 3
        const val CONFIG_SET = 4
        const val CONFIG = 5
        const val TX = 6
        const val RX = 7
        const val STATS_GET = 8
        const val STATS = 9
        const val PING = 10
        const val PONG = 11
        const val EVENT = 12
        const val RESET = 13
    }

    data class HelloAck(
        val proto: Int,
        val firmwareVersion: String,
        val nodeIdHex: String,
        val maxFrame: Int,
        val caps: Long,
        val region: String,
        val profile: String,
    )

    data class Session(
        val connected: Boolean = false,
        val ready: Boolean = false,
        val hello: HelloAck? = null,
        val lastError: String? = null,
        val stats: Map<String, Long> = emptyMap(),
        val config: Map<String, String> = emptyMap(),
    )

    sealed interface CompanionEvent {
        data class Rx(val frame: ByteArray, val rssi: Int, val snr: Int, val link: String?, val atMs: Long) : CompanionEvent
        data class Config(val entries: Map<String, String>) : CompanionEvent
        data class Stats(val entries: Map<String, Long>) : CompanionEvent
        data class RadioEvent(val kind: String, val detail: String) : CompanionEvent
        data class Pong(val nonce: Long) : CompanionEvent
        data class HelloReceived(val hello: HelloAck) : CompanionEvent
        data class Failure(val message: String) : CompanionEvent
    }

    // Kotlin type alias cannot be nested; keep a short alias type at file scope below.
    private val _session = MutableStateFlow(Session())
    val session: StateFlow<Session> = _session.asStateFlow()

    private val _events = MutableSharedFlow<CompanionEvent>(extraBufferCapacity = 128)
    val events: SharedFlow<CompanionEvent> = _events.asSharedFlow()

    private val decoder = SlipFraming.Decoder()
    private val writeMutex = Mutex()
    private var job: Job? = null
    private var scope: CoroutineScope? = null
    private var helloAck = CompletableDeferred<HelloAck>()
    private var pingNonce = 1L

    // RX instrumentation: proves whether radio notifications carrying OMP frames reach the app at
    // all (e.g. a chunked bridge->phone attachment) or are dropped below the companion client.
    private var rxChunkCount = 0L
    private var rxByteCount = 0L
    private var lastRxSummaryMs = 0L

    /** Start the read/handshake loop in [scope]; idempotent. */
    fun start(scope: CoroutineScope) {
        if (job?.isActive == true) return
        this.scope = scope
        job = scope.launch {
            launch {
                transport.connected.collectLatest { connected ->
                    if (!connected) {
                        helloAck = CompletableDeferred()
                        _session.value = _session.value.copy(connected = false, ready = false)
                    } else {
                        _session.value = _session.value.copy(connected = true, lastError = null)
                        sendHello()
                    }
                }
            }
            launch {
                transport.incoming.collect { chunk ->
                    rxChunkCount += 1
                    rxByteCount += chunk.size
                    val now = System.currentTimeMillis()
                    if (lastRxSummaryMs == 0L) lastRxSummaryMs = now
                    if (now - lastRxSummaryMs >= 2_000L) {
                        MeshLog.i(
                            "companion rx chunks=$rxChunkCount bytes=$rxByteCount window=${now - lastRxSummaryMs}ms " +
                                "connected=${_session.value.connected} ready=${_session.value.ready}",
                        )
                        rxChunkCount = 0
                        rxByteCount = 0
                        lastRxSummaryMs = now
                    }
                    for (frame in decoder.feed(chunk)) {
                        handleFrame(frame)
                    }
                }
            }
            launch {
                // Keep-alive + stats poll while ready. The companion protocol is request/
                // response; polling avoids a heartbeat requirement in the firmware.
                while (isActive) {
                    delay(15_000)
                    if (_session.value.ready) {
                        runCatching { sendPing() }
                        runCatching { sendStatsGet() }
                    }
                }
            }
        }
    }

    fun stop() {
        job?.cancel()
        job = null
        _session.value = Session()
    }

    // -------------------------------------------------------------- commands

    suspend fun sendHello() {
        val payload = CborCodec.encode(
            CborCodec.Map(
                linkedMapOf(
                    "proto" to CborCodec.UInt(1),
                    "app_version" to CborCodec.Text(clientVersion),
                    "wants_events" to CborCodec.Bool(true),
                ),
            ),
        )
        write(Type.HELLO, payload)
    }

    suspend fun sendConfigGet() = write(Type.CONFIG_GET, CborCodec.encode(CborCodec.Map(linkedMapOf())))

    suspend fun sendConfigSet(key: String, value: String) {
        val payload = CborCodec.encode(
            CborCodec.Map(linkedMapOf("key" to CborCodec.Text(key), "value" to CborCodec.Text(value))),
        )
        write(Type.CONFIG_SET, payload)
    }

    suspend fun sendStatsGet() = write(Type.STATS_GET, CborCodec.encode(CborCodec.Map(linkedMapOf())))

    suspend fun sendPing() {
        val nonce = pingNonce++
        write(Type.PING, CborCodec.encode(CborCodec.Map(linkedMapOf("nonce" to CborCodec.UInt(nonce)))))
    }

    /** Send one complete OMP frame (≤ hello.maxFrame). */
    suspend fun sendFrame(frame: ByteArray) {
        val max = _session.value.hello?.maxFrame ?: 240
        if (frame.size > max) throw IllegalArgumentException("frame ${frame.size} > maxFrame $max")
        write(Type.TX, CborCodec.encode(CborCodec.Map(linkedMapOf("frame" to CborCodec.Bytes(frame)))))
    }

    suspend fun sendReset() = write(Type.RESET, CborCodec.encode(CborCodec.Map(linkedMapOf())))

    private suspend fun write(type: Int, payload: ByteArray) {
        val frame = SlipFraming.encode(SlipFraming.buildFrame(type, payload))
        MeshLog.i("companion write type=$type frame=${frame.size}")
        try {
            writeMutex.withLock { transport.write(frame) }
        } catch (e: Exception) {
            MeshLog.w("companion write failed type=$type frame=${frame.size}: ${e.message}", e)
            throw e
        }
    }

    // -------------------------------------------------------------- receive path

    private fun handleFrame(frame: ByteArray) {
        val parsed = SlipFraming.parse(frame) ?: run {
            MeshLog.w("companion rx bad frame (CRC/size) bytes=${frame.size}")
            _session.value = _session.value.copy(lastError = "bad companion frame (CRC/size)")
            return
        }
        // type 7 = RX (an OMP frame the radio heard); resource transfers arrive as many of these.
        if (parsed.type == Type.RX) {
            MeshLog.i("companion rx frame type=RX payload=${parsed.payload.size}")
        }
        val map = try {
            CborCodec.decodeMap(parsed.payload)
        } catch (e: Exception) {
            _session.value = _session.value.copy(lastError = "bad CBOR: ${e.message}")
            return
        } ?: return

        when (parsed.type) {
            Type.HELLO_ACK -> {
                val ack = parseHelloAck(map)
                if (ack == null) {
                    _session.value = _session.value.copy(lastError = "malformed HelloAck")
                    return
                }
                if (ack.proto != 1) {
                    _session.value = _session.value.copy(
                        ready = false,
                        lastError = "radio speaks companion proto ${ack.proto}, app speaks 1",
                    )
                    return
                }
                _session.value = _session.value.copy(ready = true, hello = ack, lastError = null)
                helloAck.complete(ack)
                _events.tryEmit(CompanionEvent.HelloReceived(ack))
                scope?.launch { runCatching { sendConfigGet() } }
            }
            Type.CONFIG -> {
                val entries = parseStringEntries(map)
                _session.value = _session.value.copy(config = entries)
                _events.tryEmit(CompanionEvent.Config(entries))
            }
            Type.RX -> {
                val rx = parseRx(map) ?: return
                _events.tryEmit(rx)
            }
            Type.STATS -> {
                val entries = parseLongEntries(map)
                _session.value = _session.value.copy(stats = entries)
                _events.tryEmit(CompanionEvent.Stats(entries))
            }
            Type.PONG -> {
                _events.tryEmit(CompanionEvent.Pong(map.longOrZero("nonce")))
            }
            Type.EVENT -> {
                val kind = map.textOrNull("kind") ?: "event"
                val detail = map.textOrNull("detail") ?: ""
                _events.tryEmit(CompanionEvent.RadioEvent(kind, detail))
            }
            else -> Unit // radio → app types we do not interpret yet
        }
    }

    private fun parseHelloAck(map: Map<String, CborCodec.Value>): HelloAck? {
        val nodeId = (map["node_id"] as? CborCodec.Bytes)?.value ?: return null
        return HelloAck(
            proto = map.longOrZero("proto").toInt(),
            firmwareVersion = map.textOrNull("fw_version") ?: "unknown",
            nodeIdHex = Hex.encode(nodeId),
            maxFrame = map.longOrZero("max_frame").toInt().takeIf { it in 1..1024 } ?: 240,
            caps = map.longOrZero("caps"),
            region = map.textOrNull("region") ?: "EU868",
            profile = map.textOrNull("profile") ?: "MEDIUM_FAST",
        )
    }

    private fun parseRx(map: Map<String, CborCodec.Value>): CompanionEvent.Rx? {
        val bytes = (map["frame"] as? CborCodec.Bytes)?.value ?: return null
        return CompanionEvent.Rx(
            frame = bytes,
            rssi = map.longOrZero("rssi").toInt(),
            snr = map.longOrZero("snr").toInt(),
            link = map.textOrNull("link"),
            atMs = map.longOrZero("at_ms"),
        )
    }

    private fun parseStringEntries(map: Map<String, CborCodec.Value>): Map<String, String> {
        val arr = map["entries"] as? CborCodec.Array ?: return emptyMap()
        val out = LinkedHashMap<String, String>()
        for (entry in arr.values) {
            val pair = (entry as? CborCodec.Array)?.values ?: continue
            if (pair.size >= 2) {
                val key = (pair[0] as? CborCodec.Text)?.value ?: continue
                val value = (pair[1] as? CborCodec.Text)?.value
                    ?: (pair[1] as? CborCodec.UInt)?.value?.toString() ?: continue
                out[key] = value
            }
        }
        return out
    }

    private fun parseLongEntries(map: Map<String, CborCodec.Value>): Map<String, Long> {
        val arr = map["entries"] as? CborCodec.Array ?: return emptyMap()
        val out = LinkedHashMap<String, Long>()
        for (entry in arr.values) {
            val pair = (entry as? CborCodec.Array)?.values ?: continue
            if (pair.size >= 2) {
                val key = (pair[0] as? CborCodec.Text)?.value ?: continue
                val value = (pair[1] as? CborCodec.UInt)?.value ?: continue
                out[key] = value
            }
        }
        return out
    }

    private fun Map<String, CborCodec.Value>.textOrNull(key: String): String? =
        (this[key] as? CborCodec.Text)?.value

    private fun Map<String, CborCodec.Value>.longOrZero(key: String): Long = when (val v = this[key]) {
        is CborCodec.UInt -> v.value
        is CborCodec.NInt -> v.value
        else -> 0L
    }
}
