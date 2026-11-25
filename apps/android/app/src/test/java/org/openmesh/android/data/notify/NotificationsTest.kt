package org.openmesh.android.data.notify

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class NotificationsTest {
    @Test
    fun `preview strips newlines and bounds length`() {
        assertEquals("hello world", Notifications.previewText("hello\nworld"))
        val long = "x".repeat(400)
        val preview = Notifications.previewText(long)
        assertEquals(140, preview.length)
        assertTrue(preview.endsWith("…"))
    }

    @Test
    fun `empty previews fall back to a safe generic body`() {
        assertEquals("New message", Notifications.previewText("   "))
        assertEquals("New message", Notifications.previewText(""))
    }

    @Test
    fun `message ids separate direct and channel conversations`() {
        assertNotEquals(
            Notifications.messageId("1122334455667788", false),
            Notifications.messageId("1122334455667788", true),
        )
        assertEquals(
            Notifications.messageId("aabbccddeeff0011", false),
            Notifications.messageId("aabbccddeeff0011", false),
        )
    }
}
