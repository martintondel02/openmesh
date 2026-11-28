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
        autoRetries: Int = 0,
        nextAutoRetryAtMs: Long? = null,
        manualOnly: Boolean = false,
    ) = OutgoingMediaSnapshot(
        isChannel = isChannel,
        mime = mime,
        sizeBytes = sizeBytes,
        sentAtMs = sentAtMs,
        lastActivityMs = lastActivityMs,
        expectedWindowMs = expectedWindowMs,
        progress = progress,
        attempts = attempts,
        autoRetries = autoRetries,
        nextAutoRetryAtMs = nextAutoRetryAtMs,
        manualOnly = manualOnly,
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
        assertEquals("Stalled — tap Retry", OutgoingMediaPolicy.statusText(ui))
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

    @Test
    fun `automatic retry backoff starts at 30s doubles and caps at 15min`() {
        assertEquals(30_000, OutgoingMediaPolicy.autoRetryDelayMs(0))
        assertEquals(60_000, OutgoingMediaPolicy.autoRetryDelayMs(1))
        assertEquals(120_000, OutgoingMediaPolicy.autoRetryDelayMs(2))
        assertEquals(240_000, OutgoingMediaPolicy.autoRetryDelayMs(3))
        assertEquals(480_000, OutgoingMediaPolicy.autoRetryDelayMs(4))
        assertEquals(OutgoingMediaPolicy.AUTO_RETRY_MAX_MS, OutgoingMediaPolicy.autoRetryDelayMs(5))
        assertEquals(OutgoingMediaPolicy.AUTO_RETRY_MAX_MS, OutgoingMediaPolicy.autoRetryDelayMs(99))
    }

    @Test
    fun `a stalled upload waits for its auto retry with a visible countdown`() {
        val stalled = snapshot(
            sentAtMs = now - 120_000,
            lastActivityMs = now - 60_000,
            expectedWindowMs = 5_000,
        )
        val scheduled = OutgoingMediaPolicy.scheduleAutoRetry(stalled, now)
        assertEquals(now + 30_000, scheduled.nextAutoRetryAtMs)

        val ui = OutgoingMediaPolicy.uiState(scheduled, now)
        assertEquals(OutgoingMediaUiState.AUTO_RETRY_WAIT, ui.state)
        assertEquals(30_000, ui.retryInMs)
        assertEquals("Retrying in 30 s", OutgoingMediaPolicy.statusText(ui))
        assertTrue(OutgoingMediaPolicy.autoRetryExplanation(ui).contains("attempt 1 of 3"))

        val later = OutgoingMediaPolicy.uiState(scheduled, now + 10_000)
        assertEquals(20_000, later.retryInMs)
        assertEquals("Retrying in 20 s", OutgoingMediaPolicy.statusText(later))
    }

    @Test
    fun `after three automatic attempts the upload waits for the manual retry`() {
        val exhausted = snapshot(
            sentAtMs = now - 600_000,
            lastActivityMs = now - 240_000,
            expectedWindowMs = 5_000,
            autoRetries = OutgoingMediaPolicy.MAX_AUTO_RETRIES,
            nextAutoRetryAtMs = null,
        )
        val ui = OutgoingMediaPolicy.uiState(exhausted, now)
        assertEquals(OutgoingMediaUiState.STALLED, ui.state)
        assertEquals("Stalled — tap Retry", OutgoingMediaPolicy.statusText(ui))
        assertFalse(OutgoingMediaPolicy.autoRetryAllowed(exhausted))
        assertTrue(OutgoingMediaPolicy.autoRetryExhaustedExplanation().contains("Tap Retry"))
    }

    @Test
    fun `auto retry is due only after the backoff and only while budget remains`() {
        val scheduled = snapshot(
            sentAtMs = now - 120_000,
            lastActivityMs = now - 60_000,
            expectedWindowMs = 5_000,
        ).let { OutgoingMediaPolicy.scheduleAutoRetry(it, now) }

        assertFalse(OutgoingMediaPolicy.isAutoRetryDue(scheduled, now))
        assertTrue(OutgoingMediaPolicy.isAutoRetryDue(scheduled, now + 30_000))
        assertFalse(
            OutgoingMediaPolicy.isAutoRetryDue(
                scheduled.copy(autoRetries = OutgoingMediaPolicy.MAX_AUTO_RETRIES),
                now + 1_000_000,
            ),
        )
        assertFalse(OutgoingMediaPolicy.isAutoRetryDue(scheduled.copy(nextAutoRetryAtMs = null), now + 1_000_000))
    }

    @Test
    fun `one automatic retry is picked at a time and only when the link is safe`() {
        val older = snapshot(sentAtMs = now - 200_000, lastActivityMs = now - 100_000, expectedWindowMs = 4_000)
            .let { OutgoingMediaPolicy.scheduleAutoRetry(it, now - 40_000) }
        val newer = snapshot(sentAtMs = now - 150_000, lastActivityMs = now - 90_000, expectedWindowMs = 4_000)
            .let { OutgoingMediaPolicy.scheduleAutoRetry(it, now - 10_000) }
        val map = mapOf(1L to older, 2L to newer)
        assertEquals(1L, OutgoingMediaPolicy.pickAutoRetryCandidate(map, now))
        assertNull(OutgoingMediaPolicy.pickAutoRetryCandidate(map, now - 15_000))

        assertTrue(OutgoingMediaPolicy.canStartAutoRetry(radioReady = true, transferInFlight = false, anotherAutoRetryRunning = false))
        assertFalse(OutgoingMediaPolicy.canStartAutoRetry(radioReady = false, transferInFlight = false, anotherAutoRetryRunning = false))
        assertFalse(OutgoingMediaPolicy.canStartAutoRetry(radioReady = true, transferInFlight = true, anotherAutoRetryRunning = false))
        assertFalse(OutgoingMediaPolicy.canStartAutoRetry(radioReady = true, transferInFlight = false, anotherAutoRetryRunning = true))
    }

    @Test
    fun `starting an auto retry increments counters and resets the attempt timers`() {
        val scheduled = snapshot(
            sentAtMs = now - 120_000,
            lastActivityMs = now - 60_000,
            expectedWindowMs = 5_000,
            attempts = 2,
        ).let { OutgoingMediaPolicy.scheduleAutoRetry(it, now) }

        val started = OutgoingMediaPolicy.markAutoRetryStarted(scheduled, now + 30_000)
        assertEquals(1, started.autoRetries)
        assertEquals(3, started.attempts)
        assertNull(started.nextAutoRetryAtMs)
        assertEquals(now + 30_000, started.sentAtMs)
        assertEquals(now + 30_000, started.lastActivityMs)
        assertEquals(OutgoingMediaUiState.RETRYING, OutgoingMediaPolicy.uiState(started, now + 31_000).state)

        val manual = OutgoingMediaPolicy.markManualRetry(started, now + 40_000)
        assertEquals(0, manual.autoRetries)
        assertNull(manual.nextAutoRetryAtMs)
        assertEquals(4, manual.attempts)
    }

    @Test
    fun `auto retry wait expires into the stalled state when the attempt is not started`() {
        val scheduled = snapshot(
            sentAtMs = now - 120_000,
            lastActivityMs = now - 60_000,
            expectedWindowMs = 5_000,
        ).let { OutgoingMediaPolicy.scheduleAutoRetry(it, now) }
        val ui = OutgoingMediaPolicy.uiState(scheduled, now + 31_000)
        assertEquals(OutgoingMediaUiState.STALLED, ui.state)
        assertEquals("Stalled — tap Retry", OutgoingMediaPolicy.statusText(ui))
    }

    @Test
    fun `cancel is allowed only while the send can still cost airtime`() {
        assertTrue(OutgoingMediaPolicy.canCancel(DeliveryState.QUEUED))
        assertTrue(OutgoingMediaPolicy.canCancel(DeliveryState.SENT))
        assertTrue(OutgoingMediaPolicy.canCancel(DeliveryState.FAILED))
        assertFalse(OutgoingMediaPolicy.canCancel(DeliveryState.DELIVERED))
        assertFalse(OutgoingMediaPolicy.canCancel(DeliveryState.READ))
        assertFalse(OutgoingMediaPolicy.canCancel(DeliveryState.CANCELLED))
    }

    @Test
    fun `cancel transition stops scheduled automatic retries`() {
        val scheduled = snapshot(
            sentAtMs = now - 120_000,
            lastActivityMs = now - 60_000,
            expectedWindowMs = 5_000,
            progress = 0.4f,
        ).let { OutgoingMediaPolicy.scheduleAutoRetry(it, now) }
        assertEquals(OutgoingMediaUiState.AUTO_RETRY_WAIT, OutgoingMediaPolicy.uiState(scheduled, now).state)

        val cancelled = OutgoingMediaPolicy.cancelled(scheduled)
        assertNull(cancelled.nextAutoRetryAtMs)
        assertNull(cancelled.progress)
        assertEquals(scheduled.autoRetries, cancelled.autoRetries)
        // No auto retry can be due after cancellation (tracker is removed by the repository).
        assertFalse(OutgoingMediaPolicy.isAutoRetryDue(cancelled, now + 10 * 60_000))
    }

    @Test
    fun `rows restored from disk are manual-retry-only`() {
        val restored = snapshot(
            sentAtMs = now - 600_000,
            lastActivityMs = now - 300_000,
            expectedWindowMs = 5_000,
            manualOnly = true,
        )
        // Stalled, but never auto-retried or scheduled by the governor.
        assertEquals(OutgoingMediaUiState.STALLED, OutgoingMediaPolicy.uiState(restored, now).state)
        assertFalse(OutgoingMediaPolicy.autoRetryAllowed(restored))
        assertFalse(OutgoingMediaPolicy.isAutoRetryDue(restored, now + 24 * 3600_000L))
        assertNull(OutgoingMediaPolicy.pickAutoRetryCandidate(mapOf(7L to restored), now))

        // Live uploads in this process stay eligible.
        assertTrue(OutgoingMediaPolicy.autoRetryAllowed(snapshot(manualOnly = false)))
    }

    @Test
    fun `manual retry opts a restored row back into live tracking`() {
        val restored = snapshot(
            sentAtMs = now - 600_000,
            lastActivityMs = now - 300_000,
            expectedWindowMs = 5_000,
            manualOnly = true,
        )
        val retried = OutgoingMediaPolicy.markManualRetry(restored, now)
        assertFalse(retried.manualOnly)
        assertEquals(0, retried.autoRetries)
        assertTrue(OutgoingMediaPolicy.autoRetryAllowed(retried))
        assertNull(retried.nextAutoRetryAtMs)
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
