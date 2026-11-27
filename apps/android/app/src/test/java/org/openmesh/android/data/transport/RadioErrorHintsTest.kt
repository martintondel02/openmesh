package org.openmesh.android.data.transport

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * JVM tests for the shared radio-link error mapping: every [LinkError] must produce actionable,
 * beginner-language hints (docs/design/ux-help-system.md §6) and the health rule must be stable.
 */
class RadioErrorHintsTest {

    @Test
    fun `every link error has a non-empty hint of one or two bullets`() {
        for (error in LinkError.entries) {
            val hint = RadioErrorHints.hintFor(error)
            assertTrue("$error hint is blank", hint.isNotBlank())
            val bullets = hint.lines().filter { it.isNotBlank() }
            assertTrue("$error hint has too many bullets: ${bullets.size}", bullets.size in 1..2)
            for (bullet in bullets) {
                assertTrue("$error hint bullet does not start with a bullet point: $bullet", bullet.startsWith("•"))
            }
        }
    }

    @Test
    fun `hints contain a next action and no blame`() {
        for (error in LinkError.entries) {
            val hint = RadioErrorHints.hintFor(error).lowercase()
            assertTrue(
                "$error hint offers no next action",
                hint.contains("try") || hint.contains("turn") || hint.contains("check") ||
                    hint.contains("move") || hint.contains("allow") || hint.contains("accept") ||
                    hint.contains("unplug") || hint.contains("connect") || hint.contains("open") ||
                    hint.contains("plug") || hint.contains("switch") || hint.contains("close"),
            )
            for (blaming in listOf("invalid", "illegal", "you failed", "error code")) {
                assertTrue("$error hint blames the user: $blaming", !hint.contains(blaming))
            }
        }
    }

    @Test
    fun `specific failures map to specific guidance`() {
        assertTrue(RadioErrorHints.hintFor(LinkError.BLUETOOTH_OFF).contains("Bluetooth"))
        assertTrue(RadioErrorHints.hintFor(LinkError.BLUETOOTH_PERMISSION).contains("Allow"))
        assertTrue(RadioErrorHints.hintFor(LinkError.NO_RADIO_FOUND).contains("switched on"))
        assertTrue(RadioErrorHints.hintFor(LinkError.USB_NO_DEVICE).contains("USB data cable"))
        assertTrue(RadioErrorHints.hintFor(LinkError.HANDSHAKE_TIMEOUT).contains("firmware"))
        assertTrue(RadioErrorHints.hintFor(LinkError.LINK_LOST).contains("out of range"))
        assertTrue(RadioErrorHints.hintFor(LinkError.SERVICE_MISSING).contains("firmware"))
    }

    @Test
    fun `all hint texts are distinct so the mapping cannot silently collapse`() {
        val hints = LinkError.entries.map { RadioErrorHints.hintFor(it) }
        assertEquals(LinkError.entries.size, hints.toSet().size)
        for (hint in hints) {
            assertNotEquals("", hint)
        }
    }

    @Test
    fun `health policy handles boundary and non-ready phases`() {
        val now = 50_000_000L
        assertEquals(RadioHealth.UNKNOWN, RadioHealthPolicy.compute(RadioPhase.SCANNING, now, now))
        assertEquals(RadioHealth.UNKNOWN, RadioHealthPolicy.compute(RadioPhase.ERROR, now, now))
        assertEquals(RadioHealth.STALE, RadioHealthPolicy.compute(RadioPhase.READY, 0L, now))
        assertEquals(RadioHealth.HEALTHY, RadioHealthPolicy.compute(RadioPhase.READY, now, now))
        assertEquals(
            RadioHealth.HEALTHY,
            RadioHealthPolicy.compute(RadioPhase.READY, now - RadioHealthPolicy.STALE_AFTER_MS, now),
        )
        assertEquals(
            RadioHealth.STALE,
            RadioHealthPolicy.compute(RadioPhase.READY, now - RadioHealthPolicy.STALE_AFTER_MS - 1L, now),
        )
    }

    @Test
    fun `stale threshold is two ping intervals`() {
        // The companion client pings every 15 s; two missed pings is the stale threshold.
        assertEquals(30_000L, RadioHealthPolicy.STALE_AFTER_MS)
    }

    @Test
    fun `link progress reports whether an attempt is in progress`() {
        assertTrue(LinkProgress(phase = RadioPhase.SCANNING).isInProgress)
        assertTrue(LinkProgress(phase = RadioPhase.CONNECTING).isInProgress)
        assertTrue(LinkProgress(phase = RadioPhase.HANDSHAKING).isInProgress)
        assertTrue(!LinkProgress(phase = RadioPhase.READY).isInProgress)
        assertTrue(!LinkProgress(phase = RadioPhase.ERROR).isInProgress)
        assertTrue(!LinkProgress().isInProgress)
    }

    @Test
    fun `link progress defaults are idle and safe`() {
        val idle = LinkProgress()
        assertEquals(RadioPhase.NONE, idle.phase)
        assertEquals("", idle.message)
        assertEquals(0, idle.devicesFound)
        assertEquals(null, idle.error)
    }
}
