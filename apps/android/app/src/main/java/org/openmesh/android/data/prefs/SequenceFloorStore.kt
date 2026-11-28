package org.openmesh.android.data.prefs

import android.content.Context

/**
 * Tiny synchronous SharedPreferences store for the engine's frame sequence floor.
 *
 * Written on every send (and throttled while frames flow), read on start before the first
 * announce/send. A stale floor makes peers deduplicate the phone's frames after a restart, which was
 * the root cause of "phone→PC media never arrives".
 */
class SequenceFloorStore(context: Context) {
    private val prefs = context.applicationContext
        .getSharedPreferences(FILE_NAME, Context.MODE_PRIVATE)

    fun read(): Long = prefs.getLong(KEY_SEQ_FLOOR, 0L)

    fun write(floor: Long) {
        prefs.edit().putLong(KEY_SEQ_FLOOR, floor.coerceAtLeast(0L)).apply()
    }

    /** Factory reset: drop the persisted frame-sequence floor with the rest of the app data. */
    fun clear() {
        prefs.edit().clear().apply()
    }

    companion object {
        const val FILE_NAME: String = "openmesh_engine_seq"
        const val KEY_SEQ_FLOOR: String = "frame_seq_floor"
    }
}
