package org.openmesh.android.data.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.openmesh.android.data.repository.QrInvite
import java.util.Base64

/**
 * JVM tests for the built-in Public Mesh constants, invite URL shape and pinning behaviour.
 * The canonical channel id itself is derived in Rust (bridge-web's protocol test); Android uses
 * the value the core exposes and logs any drift.
 */
class PublicMeshTest {
    @Test
    fun `canonical constants are the protocol values`() {
        assertEquals("Public Mesh", PublicMesh.NAME)
        assertEquals("a16d63142854cef4", PublicMesh.CANONICAL_CHANNEL_ID)
        assertEquals(16, PublicMesh.CANONICAL_CHANNEL_ID.length)
        assertTrue(PublicMesh.CANONICAL_CHANNEL_ID.all { it in "0123456789abcdef" })
        assertEquals("-w-KmLkWsRmxrtQqcPT2sXrlcTHTiaCNu1F-Qp_YqZI", PublicMesh.PSK_BASE64URL)
    }

    @Test
    fun `published psk decodes to 32 bytes`() {
        val psk = PublicMesh.pskBytes()
        assertEquals(32, psk.size)
        // base64url without padding must decode without the standard alphabet substitution.
        assertNotNull(Base64.getUrlDecoder().decode(PublicMesh.PSK_BASE64URL))
    }

    @Test
    fun `join url carries name and psk only - no cosmetic id`() {
        assertTrue(PublicMesh.JOIN_URL.startsWith("openmesh://channel?"))
        assertTrue(PublicMesh.JOIN_URL.contains("name=Public%20Mesh"))
        assertTrue(PublicMesh.JOIN_URL.contains("psk=${PublicMesh.PSK_BASE64URL}"))
        assertFalse(PublicMesh.JOIN_URL.contains("id="))
        assertFalse(PublicMesh.JOIN_URL.contains("baac"))
    }

    @Test
    fun `public join url parses as a channel invite without an id`() {
        val invite = QrInvite.parse(PublicMesh.JOIN_URL)
        assertEquals(QrInvite.Kind.CHANNEL, invite.kind)
        assertEquals(PublicMesh.NAME, invite.name)
        assertEquals(PublicMesh.PSK_BASE64URL, invite.pskBase64)
        // The canonical id is filled in for Public Mesh so existing channel plumbing works.
        assertEquals(PublicMesh.CANONICAL_CHANNEL_ID, invite.nodeId)
    }

    @Test
    fun `channel id or name identifies the public channel`() {
        assertTrue(PublicMesh.isPublicChannel(PublicMesh.CANONICAL_CHANNEL_ID, "anything"))
        assertTrue(PublicMesh.isPublicChannel("baac0f9144756da1", PublicMesh.NAME))
        assertFalse(PublicMesh.isPublicChannel("baac0f9144756da1", "family"))
    }

    @Test
    fun `public channel is pinned first in channel lists`() {
        val family = Channel("baac0f9144756da1", "family")
        val public = Channel(PublicMesh.CANONICAL_CHANNEL_ID, PublicMesh.NAME, isPublic = true)
        val dev = Channel("1111111111111111", "openmesh-dev")
        val pinned = PublicMesh.pinFirst(listOf(family, dev, public))
        assertEquals(PublicMesh.NAME, pinned.first().name)
        assertEquals(3, pinned.size)
        assertTrue(pinned.first().isPublic)
    }

    @Test
    fun `warning copy is explicit about privacy and signatures`() {
        assertTrue(PublicMesh.warning().contains("anyone", ignoreCase = true))
        assertTrue(PublicMesh.warning().contains("read", ignoreCase = true))
        assertTrue(PublicMesh.warning().contains("signed", ignoreCase = true))
        assertTrue(PublicMesh.headerSubtitle().contains("anyone can join"))
    }
}
