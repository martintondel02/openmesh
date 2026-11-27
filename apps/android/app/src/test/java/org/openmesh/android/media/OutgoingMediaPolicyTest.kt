package org.openmesh.android.media

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
 * JVM tests for the outgoing-attachment upload state machine: progress, slow hint, stall verdict
 * and retry transitions.
 */
class OutgoingMediaPolicyTest {

    private val now = 1_000_000_000L

    private fun snapshot(
        sentAtMs: Long = now - 1_000,
        lastActivityMs: Long = now - 1_000,
        expectedWindowMs: Long = 30_000,
        progress: Float? = null,
        attempts: Int = 1,
        isChannel: Boolean = false,
        mime: String = "image/webp",
        sizeBytes: Int = 5_000,
    ) = OutgoingMediaSnapshot(
        isChannel = isChannel,
        mime = mime,
        sizeBytes = sizeBytes,
        sentAtMs = sentAtMs,
        lastActivityMs = lastActivityMs,
        expectedWindowMs = expectedWindowMs,
        progress = progress,
        attempts = attempts,
    )

    @Test
    fun `fresh upload is sending and shown as indeterminate`() {
        val ui = OutgoingMediaPolicy.uiState(snapshot(), now)
        assertEquals(OutgoingMediaUiState.SENDING, ui.state)
        assertNull(ui.progress)
        assertEquals("Uploading…", OutgoingMediaPolicy.statusText(ui))
        assertEquals(1, ui.attempts)
        assertTrue(ui.elapsedMs >= 1_000)
    }

    @Test
    fun `progress events drive the percentage and the progress bar`() {
        val snap = snapshot(progress = 3f / 8f, lastActivityMs = now)
        val ui = OutgoingMediaPolicy.uiState(snap, now)
        assertEquals(OutgoingMediaUiState.SENDING, ui.state)
        assertEquals(3f / 8f, ui.progress!!, 0.0001f)
        assertEquals(38, OutgoingMediaPolicy.progressPercent(ui.progress))
        assertEquals("Uploading… 38%", OutgoingMediaPolicy.statusText(ui))
        assertEquals(100, OutgoingMediaPolicy.progressPercent(1.5f))
        assertEquals(0, OutgoingMediaPolicy.progressPercent(-0.2f))
        assertNull(OutgoingMediaPolicy.progressPercent(null))
    }

    @Test
    fun `stall threshold is two expected windows clamped to 45s-180s`() {
        assertEquals(OutgoingMediaPolicy.MIN_STALL_MS, OutgoingMediaPolicy.stallThresholdMs(5_000))
        assertEquals(OutgoingMediaPolicy.MIN_STALL_MS, OutgoingMediaPolicy.stallThresholdMs(22_500))
        assertEquals(80_000, OutgoingMediaPolicy.stallThresholdMs(40_000))
        assertEquals(OutgoingMediaPolicy.MAX_STALL_MS, OutgoingMediaPolicy.stallThresholdMs(120_000))
        assertEquals(OutgoingMediaPolicy.MIN_STALL_MS, OutgoingMediaPolicy.stallThresholdMs(0))
    }

    @Test
    fun `no progress for longer than the threshold flips to stalled`() {
        val stalled = snapshot(
            sentAtMs = now - 60_000,
            lastActivityMs = now - 46_000,
            expectedWindowMs = 5_000, // threshold 45 s
        )
        val ui = OutgoingMediaPolicy.uiState(stalled, now)
        assertEquals(OutgoingMediaUiState.STALLED, ui.state)
        assertEquals(46_000, ui.stalledForMs)
        assertEquals("Upload stalled — retry", OutgoingMediaPolicy.statusText(ui))
        assertFalse(OutgoingMediaPolicy.slowHintVisible(ui))
    }

    @Test
    fun `recent activity keeps an old upload out of the stalled state`() {
        val active = snapshot(
            sentAtMs = now - 5 * 60_000,
            lastActivityMs = now - 10_000,
            expectedWindowMs = 5_000,
            progress = 0.5f,
        )
        val ui = OutgoingMediaPolicy.uiState(active, now)
        assertEquals(OutgoingMediaUiState.SENDING, ui.state)
        assertEquals(0L, ui.stalledForMs)
        assertTrue(OutgoingMediaPolicy.slowHintVisible(ui))
    }

    @Test
    fun `a retried upload is labelled retrying for the first ten seconds only`() {
        val justRetried = snapshot(
            sentAtMs = now - 2_000,
            lastActivityMs = now - 2_000,
            attempts = 2,
        )
        val ui = OutgoingMediaPolicy.uiState(justRetried, now)
        assertEquals(OutgoingMediaUiState.RETRYING, ui.state)
        assertEquals("Retrying…", OutgoingMediaPolicy.statusText(ui))

        val retriedLongAgo = snapshot(
            sentAtMs = now - OutgoingMediaPolicy.RETRYING_WINDOW_MS - 1,
            lastActivityMs = now - OutgoingMediaPolicy.RETRYING_WINDOW_MS - 1,
            attempts = 2,
        )
        assertEquals(OutgoingMediaUiState.SENDING, OutgoingMediaPolicy.uiState(retriedLongAgo, now).state)
    }

    @Test
    fun `retry resets timestamps progress and increments the attempt count`() {
        val old = snapshot(
            sentAtMs = now - 90_000,
            lastActivityMs = now - 60_000,
            progress = 0.25f,
            attempts = 1,
        )
        val retried = OutgoingMediaPolicy.retried(old, now)
        assertEquals(now, retried.sentAtMs)
        assertEquals(now, retried.lastActivityMs)
        assertNull(retried.progress)
        assertEquals(2, retried.attempts)
        assertEquals(old.mime, retried.mime)
        assertEquals(old.sizeBytes, retried.sizeBytes)
        assertEquals(OutgoingMediaUiState.RETRYING, OutgoingMediaPolicy.uiState(retried, now).state)
    }

    @Test
    fun `slow hint appears after eight seconds of a non-stalled upload`() {
        val ui = OutgoingMediaPolicy.uiState(
            snapshot(sentAtMs = now - 9_000, lastActivityMs = now - 2_000),
            now,
        )
        assertTrue(OutgoingMediaPolicy.slowHintVisible(ui))
        assertTrue(OutgoingMediaPolicy.slowHintText(isChannel = false).contains("background"))
        assertTrue(OutgoingMediaPolicy.slowHintText(isChannel = true).contains("Radio uploads"))

        val early = OutgoingMediaPolicy.uiState(
            snapshot(sentAtMs = now - 3_000, lastActivityMs = now - 2_000),
            now,
        )
        assertFalse(OutgoingMediaPolicy.slowHintVisible(early))
    }

    @Test
    fun `stalled explanation promises one bubble and a fresh attempt`() {
        val text = OutgoingMediaPolicy.stalledExplanation(isChannel = true)
        assertTrue(text.contains("Retry", ignoreCase = true))
        assertTrue(text.contains("not duplicated", ignoreCase = true))
    }

    @Test
    fun `only queued and sent deliveries count as active uploads`() {
        assertTrue(OutgoingMediaPolicy.isActive(DeliveryState.QUEUED))
        assertTrue(OutgoingMediaPolicy.isActive(DeliveryState.SENT))
        assertFalse(OutgoingMediaPolicy.isActive(DeliveryState.DELIVERED))
        assertFalse(OutgoingMediaPolicy.isActive(DeliveryState.READ))
        assertFalse(OutgoingMediaPolicy.isActive(DeliveryState.FAILED))
    }

    @Test
    fun `fromMessage only seeds outgoing sent attachments`() {
        val sent = message(delivery = DeliveryState.SENT, outgoing = true, kind = MessageKind.ATTACHMENT)
        val fromSent = OutgoingMediaPolicy.fromMessage(sent, expectedWindowMs = 30_000)
        assertEquals(30_000, fromSent!!.expectedWindowMs)
        assertEquals(1, fromSent.attempts)
        assertNull(fromSent.progress)

        assertNull(OutgoingMediaPolicy.fromMessage(message(DeliveryState.DELIVERED, true, MessageKind.ATTACHMENT), 1))
        assertNull(OutgoingMediaPolicy.fromMessage(message(DeliveryState.SENT, false, MessageKind.ATTACHMENT), 1))
        assertNull(OutgoingMediaPolicy.fromMessage(message(DeliveryState.SENT, true, MessageKind.TEXT), 1))
    }

    private fun message(delivery: DeliveryState, outgoing: Boolean, kind: MessageKind) = Message(
        id = 42L,
        peerId = "6d2dfb7be7948c4b",
        fromId = if (outgoing) "68102c09d9c0aeca" else "6d2dfb7be7948c4b",
        toId = "6d2dfb7be7948c4b",
        kind = kind,
        text = "media",
        sentAtMs = now - 10_000,
        outgoing = outgoing,
        delivery = delivery,
        attachment = if (kind == MessageKind.ATTACHMENT) {
            Attachment(id = "att_42", mime = "image/webp", sizeBytes = 5_000, progress = 1f)
        } else {
            null
        },
    )
}
