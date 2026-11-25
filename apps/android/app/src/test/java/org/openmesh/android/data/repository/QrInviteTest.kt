package org.openmesh.android.data.repository

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class QrInviteTest {
    private val nodeId = "1122334455667788"
    private val channelId = "aabbccddeeff0011"
    private val identity = "A".repeat(96)

    @Test
    fun `contact invite round-trips node id name and identity`() {
        val url = QrInvite.contact(nodeId, "Alice", identity)
        val parsed = QrInvite.parse(url)
        assertEquals(QrInvite.Kind.CONTACT, parsed.kind)
        assertEquals(nodeId, parsed.nodeId)
        assertEquals("Alice", parsed.name)
        assertEquals(identity, parsed.identityBase64)
    }

    @Test
    fun `channel invite round-trips psk and radio hints`() {
        val url = QrInvite.channel(channelId, "family", "cGtr", "MEDIUM_FAST", "EU868")
        val parsed = QrInvite.parse(url)
        assertEquals(QrInvite.Kind.CHANNEL, parsed.kind)
        assertEquals(channelId, parsed.nodeId)
        assertEquals("family", parsed.name)
        assertEquals("cGtr", parsed.pskBase64)
        assertEquals("MEDIUM_FAST", parsed.profile)
        assertEquals("EU868", parsed.region)
    }

    @Test
    fun `ids must be 16 hex characters`() {
        assertThrows(IllegalArgumentException::class.java) {
            QrInvite.parse("openmesh://contact?node_id=xyz&name=A")
        }
        assertThrows(IllegalArgumentException::class.java) {
            QrInvite.parse("openmesh://channel?id=1122&psk=x")
        }
    }

    @Test
    fun `non openmesh schemes and unknown hosts are rejected`() {
        assertThrows(IllegalArgumentException::class.java) { QrInvite.parse("https://example.com/x") }
        assertThrows(IllegalArgumentException::class.java) { QrInvite.parse("openmesh://backbone?host=x") }
    }

    @Test
    fun `names are bounded to the announce limit`() {
        val long = "x".repeat(120)
        val parsed = QrInvite.parse(QrInvite.contact(nodeId, long, identity))
        assertEquals(32, parsed.name.length)
        assertTrue(parsed.name.all { it == 'x' })
    }
}
