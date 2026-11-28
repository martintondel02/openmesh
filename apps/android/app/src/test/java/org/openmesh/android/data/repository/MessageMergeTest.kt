package org.openmesh.android.data.repository

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.openmesh.android.data.model.Attachment
import org.openmesh.android.data.model.DeliveryState
import org.openmesh.android.data.model.Message
import org.openmesh.android.data.model.MessageKind

/**
 * JVM tests for the order-independent inbound-message merge. Covers the real-world event orders
 * seen from the companion/FFI stream: GroupMessage text before or after the attachment payload
 * resource, oversized-envelope events without attachment data, and loopback echoes of own frames.
 */
class MessageMergeTest {

    private fun message(
        id: Long = 67017298,
        kind: MessageKind = MessageKind.TEXT,
        text: String? = null,
        attachment: Attachment? = null,
        outgoing: Boolean = false,
        fromId: String = "52195e4f1a4201a3",
        sentAtMs: Long = 1_000,
        receivedAtMs: Long? = 1_100,
        delivery: DeliveryState = DeliveryState.DELIVERED,
        read: Boolean = false,
        viaStore: Boolean = false,
        bridged: Boolean = false,
    ) = Message(
        id = id,
        peerId = "c63c92c033aac7b9",
        fromId = fromId,
        toId = "c63c92c033aac7b9",
        kind = kind,
        text = text,
        sentAtMs = sentAtMs,
        receivedAtMs = receivedAtMs,
        outgoing = outgoing,
        delivery = delivery,
        read = read,
        attachment = attachment,
        viaStore = viaStore,
        bridged = bridged,
    )

    private fun attachment(
        mime: String = "audio/c2-1300",
        size: Int = 308,
    ) = Attachment(id = "att_67017298", mime = mime, sizeBytes = size, durationMs = null, name = null, progress = 1f)

    @Test
    fun `text event arriving after the payload keeps the attachment`() {
        val payloadFirst = message(
            kind = MessageKind.ATTACHMENT,
            attachment = attachment(),
            sentAtMs = 1_000,
        )
        val textAfter = message(kind = MessageKind.TEXT, text = "bridge voice for phone", sentAtMs = 1_000)
        val merged = MessageMerge.merge(payloadFirst, textAfter)
        assertEquals(MessageKind.ATTACHMENT, merged.kind)
        assertEquals("audio/c2-1300", merged.attachment?.mime)
        assertEquals(308, merged.attachment?.sizeBytes)
        assertEquals("bridge voice for phone", merged.text)
    }

    @Test
    fun `payload event arriving after the text keeps the caption and adds the attachment`() {
        val textFirst = message(kind = MessageKind.TEXT, text = "bridge image for phone", attachment = null)
        val payloadAfter = message(
            kind = MessageKind.ATTACHMENT,
            text = null,
            attachment = attachment(mime = "image/webp", size = 6018),
            sentAtMs = 1_050,
        )
        val merged = MessageMerge.merge(textFirst, payloadAfter)
        assertEquals(MessageKind.ATTACHMENT, merged.kind)
        assertEquals("image/webp", merged.attachment?.mime)
        assertEquals(6018, merged.attachment?.sizeBytes)
        assertEquals("bridge image for phone", merged.text)
    }

    @Test
    fun `incoming richer attachment metadata replaces stale envelope metadata`() {
        val envelopeRow = message(
            kind = MessageKind.ATTACHMENT,
            attachment = attachment(mime = "application/cbor", size = 401),
        )
        val payload = message(
            kind = MessageKind.ATTACHMENT,
            attachment = attachment(mime = "audio/c2-1300", size = 308),
            sentAtMs = 1_000,
        )
        val merged = MessageMerge.merge(envelopeRow, payload)
        assertEquals("audio/c2-1300", merged.attachment?.mime)
        assertEquals(308, merged.attachment?.sizeBytes)
    }

    @Test
    fun `a later text-only event can never clear an attachment`() {
        val withAttachment = message(kind = MessageKind.ATTACHMENT, attachment = attachment())
        val textOnly = message(kind = MessageKind.TEXT, text = "", attachment = null)
        val merged = MessageMerge.merge(withAttachment, textOnly)
        assertEquals(MessageKind.ATTACHMENT, merged.kind)
        assertEquals("audio/c2-1300", merged.attachment?.mime)
        assertEquals("bridge voice for phone", MessageMerge.merge(
            message(kind = MessageKind.ATTACHMENT, attachment = attachment(), text = "bridge voice for phone"),
            textOnly,
        ).text)
    }

    @Test
    fun `loopback echo of an own message stays outgoing and keeps the local sender`() {
        val sent = message(
            outgoing = true,
            fromId = "68102c09d9c0aeca",
            delivery = DeliveryState.SENT,
            sentAtMs = 5_000,
            receivedAtMs = null,
        )
        val echo = message(
            outgoing = false,
            fromId = "6d2dfb7be7948c4b",
            delivery = DeliveryState.DELIVERED,
            sentAtMs = 5_100,
        )
        val merged = MessageMerge.merge(sent, echo)
        assertTrue(merged.outgoing)
        assertEquals("68102c09d9c0aeca", merged.fromId)
        assertEquals(DeliveryState.DELIVERED, merged.delivery)
        assertEquals(5_000L, merged.sentAtMs)
    }

    @Test
    fun `earliest positive timestamp wins and read is sticky`() {
        val readEarlier = message(sentAtMs = 7_000, read = true)
        val unreadLater = message(sentAtMs = 9_000, read = false)
        val merged = MessageMerge.merge(readEarlier, unreadLater)
        assertEquals(7_000L, merged.sentAtMs)
        assertTrue(merged.read)
    }

    @Test
    fun `delivery merge is monotonic and failed is sticky`() {
        assertEquals(DeliveryState.DELIVERED, MessageMerge.mergeDelivery(DeliveryState.SENT, DeliveryState.DELIVERED))
        assertEquals(DeliveryState.DELIVERED, MessageMerge.mergeDelivery(DeliveryState.QUEUED, DeliveryState.DELIVERED))
        assertEquals(DeliveryState.SENT, MessageMerge.mergeDelivery(DeliveryState.QUEUED, DeliveryState.SENT))
        assertEquals(DeliveryState.FAILED, MessageMerge.mergeDelivery(DeliveryState.FAILED, DeliveryState.SENT))
        assertEquals(
            DeliveryState.CANCELLED,
            MessageMerge.mergeDelivery(DeliveryState.SENT, DeliveryState.CANCELLED),
        )
        assertEquals(
            DeliveryState.CANCELLED,
            MessageMerge.mergeDelivery(DeliveryState.CANCELLED, DeliveryState.DELIVERED),
        )
    }

    @Test
    fun `store and bridge provenance flags are preserved`() {
        val stored = message(viaStore = true, bridged = false)
        val replayed = message(viaStore = false, bridged = true, sentAtMs = 1_500)
        val merged = MessageMerge.merge(stored, replayed)
        assertTrue(merged.viaStore)
        assertTrue(merged.bridged)
    }

    @Test
    fun `first copy is returned when there is nothing to merge`() {
        val incoming = message(text = "hello")
        assertEquals(incoming, MessageMerge.merge(null, incoming))
        val other = message(id = 1234, text = "other id")
        assertEquals(other, MessageMerge.merge(incoming, other))
        assertNull(MessageMerge.merge(null, incoming).attachment)
        assertFalse(MessageMerge.merge(incoming, message(text = "x")).outgoing)
    }
}
