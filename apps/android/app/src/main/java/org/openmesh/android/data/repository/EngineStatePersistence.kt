package org.openmesh.android.data.repository

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * Pure decisions for persisting the engine's opaque state across process death.
 *
 * Root cause of "phone→PC media never arrives": the engine's frame sequence (`last_seq`) and message
 * counters were only exported on stop/channel changes, so a process restart re-originated frames
 * with already-seen `(src, seq)` values and the bridge/peer deduplicated them for its TTL. Persisting
 * a recent state after sends (debounced) and periodically keeps the restored floor ahead of
 * everything a peer has already seen.
 *
 * Android-free; unit-tested in `EngineStatePersistenceTest`.
 */
object EngineStatePersistence {
    /** After any send/frame, write state at most this often (cheap JSON + DataStore write). */
    const val SEND_DEBOUNCE_MS: Long = 1_500L

    /** Safety net while running: export state at least this often. */
    const val PERIODIC_MS: Long = 12_000L

    /** Minimum spacing for cheap floor writes while frames are flowing. */
    const val SEQ_FLOOR_THROTTLE_MS: Long = 1_000L

    /** True when a persistence triggered by a send/frame should actually run. */
    fun shouldPersistAfterSend(lastPersistedAtMs: Long, nowMs: Long): Boolean =
        lastPersistedAtMs <= 0L || nowMs - lastPersistedAtMs >= SEND_DEBOUNCE_MS

    /** True when the periodic safety-net export is due. */
    fun shouldPersistPeriodically(lastPersistedAtMs: Long, nowMs: Long): Boolean =
        lastPersistedAtMs <= 0L || nowMs - lastPersistedAtMs >= PERIODIC_MS

    /** True when the throttled per-frame floor write is due. */
    fun shouldPersistSequenceFloor(lastPersistedAtMs: Long, nowMs: Long): Boolean =
        lastPersistedAtMs <= 0L || nowMs - lastPersistedAtMs >= SEQ_FLOOR_THROTTLE_MS

    /**
     * Effective floor after restore: the max of the cheap persisted floor and the floor from the
     * last full state export. A stale full state must never pull the counter back below what a peer
     * has already seen, so the persisted floor wins when it is higher. The next originated frame is
     * `restored + 1`, strictly above both inputs.
     */
    fun restoredSequenceFloor(persistedFloor: Long, importedStateFloor: Long?): Long =
        maxOf(persistedFloor.coerceAtLeast(0L), importedStateFloor?.coerceAtLeast(0L) ?: 0L)

    /**
     * Extract the engine's `last_seq` from the opaque JSON state (state version 2). Null when the
     * bytes are not valid state or the field is missing (legacy state).
     */
    fun sequenceFloorFromState(state: ByteArray?): Long? {
        if (state == null || state.isEmpty()) return null
        return runCatching {
            Json.parseToJsonElement(state.decodeToString())
                .jsonObject["last_seq"]
                ?.jsonPrimitive
                ?.intOrNull
                ?.toLong()
        }.getOrNull()
    }
}
