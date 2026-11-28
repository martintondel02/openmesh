package org.openmesh.android.data.repository

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** JVM tests for the engine-state persistence decisions and sequence-floor extraction. */
class EngineStatePersistenceTest {
    @Test
    fun `send-triggered persistence is debounced`() {
        assertTrue(EngineStatePersistence.shouldPersistAfterSend(lastPersistedAtMs = 0L, nowMs = 0L))
        assertFalse(
            EngineStatePersistence.shouldPersistAfterSend(
                lastPersistedAtMs = 1_000L,
                nowMs = 1_000L + EngineStatePersistence.SEND_DEBOUNCE_MS - 1,
            ),
        )
        assertTrue(
            EngineStatePersistence.shouldPersistAfterSend(
                lastPersistedAtMs = 1_000L,
                nowMs = 1_000L + EngineStatePersistence.SEND_DEBOUNCE_MS,
            ),
        )
    }

    @Test
    fun `periodic safety net runs every twelve seconds`() {
        assertTrue(EngineStatePersistence.shouldPersistPeriodically(0L, 0L))
        assertFalse(
            EngineStatePersistence.shouldPersistPeriodically(
                lastPersistedAtMs = 5_000L,
                nowMs = 5_000L + EngineStatePersistence.PERIODIC_MS - 1,
            ),
        )
        assertTrue(
            EngineStatePersistence.shouldPersistPeriodically(
                lastPersistedAtMs = 5_000L,
                nowMs = 5_000L + EngineStatePersistence.PERIODIC_MS,
            ),
        )
    }

    @Test
    fun `restored floor is the max of persisted floor and stale imported state`() {
        // Stale full-state export (older) + newer persisted per-send floor.
        assertEquals(1_050L, EngineStatePersistence.restoredSequenceFloor(persistedFloor = 1_050L, importedStateFloor = 100L))
        // Full state ahead of the persisted floor (e.g. periodic export right before kill).
        assertEquals(2_000L, EngineStatePersistence.restoredSequenceFloor(persistedFloor = 1_000L, importedStateFloor = 2_000L))
        // No floor anywhere.
        assertEquals(0L, EngineStatePersistence.restoredSequenceFloor(persistedFloor = 0L, importedStateFloor = null))
        assertEquals(0L, EngineStatePersistence.restoredSequenceFloor(persistedFloor = -5L, importedStateFloor = -1L))
    }

    @Test
    fun `first originated frame after restore is strictly above the persisted floor`() {
        val persisted = 42L
        val restored = EngineStatePersistence.restoredSequenceFloor(persisted, importedStateFloor = 7L)
        // The engine's next frame is restored + 1 (set_sequence_floor keeps the counter monotonic),
        // so a peer that has seen seq <= persisted will not deduplicate the next frame.
        assertEquals(43L, restored + 1)
        org.junit.Assert.assertTrue(restored + 1 > persisted)
    }

    @Test
    fun `per-frame floor writes are throttled to one second`() {
        assertTrue(EngineStatePersistence.shouldPersistSequenceFloor(0L, 0L))
        assertFalse(
            EngineStatePersistence.shouldPersistSequenceFloor(
                lastPersistedAtMs = 1_000L,
                nowMs = 1_000L + EngineStatePersistence.SEQ_FLOOR_THROTTLE_MS - 1,
            ),
        )
        assertTrue(
            EngineStatePersistence.shouldPersistSequenceFloor(
                lastPersistedAtMs = 1_000L,
                nowMs = 1_000L + EngineStatePersistence.SEQ_FLOOR_THROTTLE_MS,
            ),
        )
    }

    @Test
    fun `sequence floor is read from exported state json`() {
        val state = """{"version":2,"contacts":[],"channels":[],"last_seq":4242,"msg_counter":7,"msg_id_epoch":0}"""
            .toByteArray()
        assertEquals(4242L, EngineStatePersistence.sequenceFloorFromState(state))
    }

    @Test
    fun `legacy state without last_seq and invalid states return null`() {
        assertNull(EngineStatePersistence.sequenceFloorFromState(null))
        assertNull(EngineStatePersistence.sequenceFloorFromState(ByteArray(0)))
        assertNull(EngineStatePersistence.sequenceFloorFromState("not json".toByteArray()))
        assertNull(EngineStatePersistence.sequenceFloorFromState("""{"version":1,"contacts":[]}""".toByteArray()))
        assertNull(EngineStatePersistence.sequenceFloorFromState(byteArrayOf(0x1f, 0x8b.toByte(), 0x08)))
    }
}
