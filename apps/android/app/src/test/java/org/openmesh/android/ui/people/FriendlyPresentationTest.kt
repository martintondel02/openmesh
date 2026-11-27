package org.openmesh.android.ui.people

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.openmesh.android.data.model.DeliveryState
import org.openmesh.android.data.model.Message
import org.openmesh.android.data.model.MessageKind
import org.openmesh.android.data.model.RadioStatus
import org.openmesh.android.data.transport.RadioHealth
import org.openmesh.android.data.transport.RadioPhase

/**
 * JVM tests for the friendly name, delivery/read and peer-link presentation rules.
 */
class FriendlyPresentationTest {

    // ------------------------------------------------------------ display names

    @Test
    fun `validated name is trimmed and stored without surrounding spaces`() {
        val result = NamePolicy.validate("  Ada Lovelace  ")
        assertTrue(result.ok)
        assertEquals("Ada Lovelace", result.value)
        assertNull(result.error)
    }

    @Test
    fun `empty or blank names are rejected`() {
        for (raw in listOf("", "   ", "\t\n")) {
            val result = NamePolicy.validate(raw)
            assertFalse(result.ok)
            assertNull(result.value)
            assertTrue(result.error!!.contains("display name", ignoreCase = true))
        }
    }

    @Test
    fun `names are limited to 32 utf8 bytes`() {
        assertEquals(32, "é".repeat(16).toByteArray(Charsets.UTF_8).size)
        assertTrue(NamePolicy.validate("é".repeat(16)).ok)

        val tooLong = NamePolicy.validate("é".repeat(17))
        assertFalse(tooLong.ok)
        assertTrue(tooLong.error!!.contains("32"))

        val ascii = NamePolicy.validate("a".repeat(NamePolicy.MAX_NAME_BYTES))
        assertTrue(ascii.ok)
        assertFalse(NamePolicy.validate("a".repeat(NamePolicy.MAX_NAME_BYTES + 1)).ok)
    }

    @Test
    fun `control characters are rejected`() {
        val result = NamePolicy.validate("Ada\nLovelace")
        assertFalse(result.ok)
        assertTrue(result.error!!.contains("plain text", ignoreCase = true))
    }

    @Test
    fun `short id fallback keeps the id recognisable`() {
        val id = "6d2dfb7be7948c4b"
        assertEquals("6d2dfb7b", NamePolicy.shortId(id))
        assertEquals("node 6d2dfb7b", NamePolicy.fallbackName(id))
        assertEquals("PC Bridge · 6d2dfb7b", NamePolicy.labelWithId("PC Bridge", id))
        assertTrue(NamePolicy.isFallbackName("node 6d2dfb7b", id))
        assertFalse(NamePolicy.isFallbackName("PC Bridge", id))
    }

    @Test
    fun `announced rename replaces a stale invite alias but alias remains the fallback`() {
        val id = "6d2dfb7be7948c4b"
        assertEquals(
            "New Name",
            NamePolicy.displayName(id, announcedName = "New Name", inviteName = "Old Aliased Name"),
        )
        assertEquals(
            "Old Aliased Name",
            NamePolicy.displayName(id, announcedName = null, inviteName = "Old Aliased Name"),
        )
        assertEquals(
            "PC Bridge",
            NamePolicy.displayName(id, announcedName = "PC Bridge", inviteName = null),
        )
        assertEquals("node 6d2dfb7b", NamePolicy.displayName(id, announcedName = " ", inviteName = ""))
        assertEquals(
            "family",
            NamePolicy.displayName(id, announcedName = "should not win", inviteName = "x", channelName = "family"),
        )
    }

    // ------------------------------------------------------------ delivery / read

    @Test
    fun `delivery labels and bubble status cover read and failed states explicitly`() {
        assertEquals("Queued", DeliveryPresentation.label(DeliveryState.QUEUED))
        assertEquals("Sent", DeliveryPresentation.label(DeliveryState.SENT))
        assertEquals("Delivered", DeliveryPresentation.label(DeliveryState.DELIVERED))
        assertEquals("Read", DeliveryPresentation.label(DeliveryState.READ))
        assertEquals("Stored at relay", DeliveryPresentation.label(DeliveryState.STORED_AT_RELAY))
        assertEquals("Failed", DeliveryPresentation.label(DeliveryState.FAILED))
        assertTrue(DeliveryPresentation.bubbleStatus(DeliveryState.FAILED).contains("retry"))
        assertEquals("Read", DeliveryPresentation.bubbleStatus(DeliveryState.READ))
    }

    @Test
    fun `explanations distinguish direct messages from channels`() {
        val dmSent = DeliveryPresentation.explanation(DeliveryState.SENT, isChannel = false, peerName = "Alice")
        val channelSent = DeliveryPresentation.explanation(DeliveryState.SENT, isChannel = true, peerName = "Alice")
        assertTrue(dmSent.contains("Alice"))
        assertTrue(channelSent.contains("no per-person receipt", ignoreCase = true))
        assertFalse(channelSent.contains("Alice"))

        val dmDelivered = DeliveryPresentation.explanation(DeliveryState.DELIVERED, isChannel = false, peerName = "Alice")
        assertTrue(dmDelivered.contains("Delivered to Alice"))
        val channelDelivered = DeliveryPresentation.explanation(DeliveryState.DELIVERED, isChannel = true, peerName = "Alice")
        assertTrue(channelDelivered.contains("No per-person receipts", ignoreCase = true))

        val dmRead = DeliveryPresentation.explanation(DeliveryState.READ, isChannel = false, peerName = "Alice")
        assertTrue(dmRead.contains("Alice has read"))
        val channelRead = DeliveryPresentation.explanation(DeliveryState.READ, isChannel = true, peerName = "Alice")
        assertTrue(channelRead.contains("not available", ignoreCase = true))

        assertTrue(
            DeliveryPresentation.explanation(DeliveryState.QUEUED, isChannel = false, peerName = "Alice")
                .contains("radio", ignoreCase = true),
        )
        assertTrue(
            DeliveryPresentation.explanation(DeliveryState.FAILED, isChannel = false, peerName = "Alice")
                .contains("retry", ignoreCase = true),
        )
    }

    @Test
    fun `delivery updates are monotonic and do not downgrade delivered or read`() {
        assertTrue(DeliveryPresentation.isUpgrade(DeliveryState.SENT, DeliveryState.DELIVERED))
        assertTrue(DeliveryPresentation.isUpgrade(DeliveryState.DELIVERED, DeliveryState.READ))
        assertFalse(DeliveryPresentation.isUpgrade(DeliveryState.DELIVERED, DeliveryState.SENT))
        assertFalse(DeliveryPresentation.isUpgrade(DeliveryState.READ, DeliveryState.DELIVERED))
        assertFalse(DeliveryPresentation.isUpgrade(DeliveryState.READ, DeliveryState.FAILED))
        assertTrue(DeliveryPresentation.isUpgrade(DeliveryState.QUEUED, DeliveryState.FAILED))
        assertTrue(DeliveryPresentation.isUpgrade(DeliveryState.FAILED, DeliveryState.READ))
        // A retry that succeeds must be able to recover a failed message.
        assertTrue(DeliveryPresentation.isUpgrade(DeliveryState.FAILED, DeliveryState.SENT))
        assertTrue(DeliveryPresentation.isUpgrade(DeliveryState.FAILED, DeliveryState.DELIVERED))
        assertFalse(DeliveryPresentation.isUpgrade(DeliveryState.FAILED, DeliveryState.FAILED))
        assertFalse(DeliveryPresentation.isUpgrade(DeliveryState.SENT, DeliveryState.SENT))
    }

    @Test
    fun `delivery summary counts sent delivered read failed and explains channels`() {
        val messages = listOf(
            message(id = 1, delivery = DeliveryState.READ),
            message(id = 2, delivery = DeliveryState.DELIVERED),
            message(id = 3, delivery = DeliveryState.FAILED),
            message(id = 4, delivery = DeliveryState.SENT),
            message(id = 5, delivery = DeliveryState.DELIVERED, outgoing = false),
        )
        val summary = DeliveryPresentation.summary(messages)
        assertEquals(4, summary.sent)
        assertEquals(2, summary.delivered)
        assertEquals(1, summary.read)
        assertEquals(1, summary.failed)
        assertEquals(DeliveryState.SENT, summary.last)

        val dmText = DeliveryPresentation.summaryText(summary, isChannel = false, peerName = "Alice")
        assertTrue(dmText.contains("4 sent"))
        assertTrue(dmText.contains("2 delivered"))
        assertTrue(dmText.contains("1 read"))
        assertTrue(dmText.contains("1 failed"))

        val channelText = DeliveryPresentation.summaryText(summary, isChannel = true, peerName = "family")
        assertTrue(channelText.contains("no per-person receipts", ignoreCase = true))

        assertEquals(
            "No messages sent yet.",
            DeliveryPresentation.summaryText(DeliveryPresentation.summary(emptyList()), false, "Alice"),
        )
    }

    // ------------------------------------------------------------ peer link stability

    @Test
    fun `link quality is derived from rssi and last-heard age`() {
        val now = 10_000_000L
        assertEquals(LinkQuality.STRONG, PeerLinkPresentation.quality(-60, now - 10_000, now))
        assertEquals(LinkQuality.STRONG, PeerLinkPresentation.quality(-80, now - 10_000, now))
        assertEquals(LinkQuality.OK, PeerLinkPresentation.quality(-90, now - 10_000, now))
        assertEquals(LinkQuality.OK, PeerLinkPresentation.quality(-100, now - 10_000, now))
        assertEquals(LinkQuality.WEAK, PeerLinkPresentation.quality(-101, now - 10_000, now))
        assertEquals(LinkQuality.UNKNOWN, PeerLinkPresentation.quality(null, now - 10_000, now))
        assertEquals(LinkQuality.OFFLINE, PeerLinkPresentation.quality(-60, 0L, now))
        assertEquals(
            LinkQuality.OFFLINE,
            PeerLinkPresentation.quality(-60, now - PeerLinkPresentation.OFFLINE_AFTER_MS - 1, now),
        )
        assertEquals(
            LinkQuality.STRONG,
            PeerLinkPresentation.quality(-60, now - PeerLinkPresentation.OFFLINE_AFTER_MS, now),
        )
    }

    @Test
    fun `route and last-heard labels use beginner language`() {
        assertEquals("Direct link (0 hops)", PeerLinkPresentation.routeLabel(0))
        assertEquals("Via 1 relay", PeerLinkPresentation.routeLabel(1))
        assertEquals("Via 2 relays", PeerLinkPresentation.routeLabel(2))

        val now = 10_000_000_000L
        assertEquals("Not heard yet", PeerLinkPresentation.lastHeardLabel(0L, now))
        assertEquals("Heard just now", PeerLinkPresentation.lastHeardLabel(now - 20_000, now))
        assertEquals("Heard 5 min ago", PeerLinkPresentation.lastHeardLabel(now - 5 * 60_000, now))
        assertEquals("Heard 2 h ago", PeerLinkPresentation.lastHeardLabel(now - 2 * 3_600_000, now))
        assertEquals("Heard 3 d ago", PeerLinkPresentation.lastHeardLabel(now - 3 * 86_400_000, now))
    }

    @Test
    fun `signal and radio summary copy never leaks raw enum names`() {
        assertEquals("RSSI —", PeerLinkPresentation.signalLabel(null, null))
        assertEquals("RSSI -87 dBm · SNR 4.5 dB", PeerLinkPresentation.signalLabel(-87, 4.5f))

        assertEquals(
            "Radio ready · BLE · answering",
            PeerLinkPresentation.radioSummary(radio(RadioPhase.READY, RadioHealth.HEALTHY, "BLE")),
        )
        assertEquals(
            "Radio ready · BLE · not answering (stale link)",
            PeerLinkPresentation.radioSummary(radio(RadioPhase.READY, RadioHealth.STALE, "BLE")),
        )
        assertTrue(PeerLinkPresentation.radioSummary(radio(RadioPhase.SCANNING, RadioHealth.UNKNOWN)).contains("connecting"))
        assertTrue(PeerLinkPresentation.radioSummary(radio(RadioPhase.ERROR, RadioHealth.UNKNOWN)).contains("attention"))
        assertTrue(PeerLinkPresentation.radioSummary(radio(RadioPhase.NONE, RadioHealth.UNKNOWN)).contains("No radio"))
    }

    // ------------------------------------------------------------ helpers

    private fun message(
        id: Long,
        delivery: DeliveryState,
        outgoing: Boolean = true,
    ) = Message(
        id = id,
        peerId = "6d2dfb7be7948c4b",
        fromId = if (outgoing) "68102c09d9c0aeca" else "6d2dfb7be7948c4b",
        toId = "6d2dfb7be7948c4b",
        kind = MessageKind.TEXT,
        text = "hi $id",
        sentAtMs = 1_000L + id,
        outgoing = outgoing,
        delivery = delivery,
    )

    private fun radio(phase: RadioPhase, health: RadioHealth, transport: String = "BLE") = RadioStatus(
        connected = phase == RadioPhase.READY,
        transportName = transport,
        phase = phase,
        health = health,
        lastError = if (phase == RadioPhase.ERROR) "Bluetooth is turned off." else null,
    )
}
