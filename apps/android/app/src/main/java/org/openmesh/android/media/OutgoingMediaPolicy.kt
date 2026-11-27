package org.openmesh.android.media

import org.openmesh.android.data.model.DeliveryState
import org.openmesh.android.data.model.Message
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/**
 * Pure state machine for outgoing attachment uploads (progress, slow hint, stall + retry).
 *
 * Android-free so the timing/copy decisions are unit-tested on the JVM
 * (`app/src/test/java/org/openmesh/android/media/OutgoingMediaPolicyTest.kt`). The repository feeds
 * snapshots from ResourceProgress / delivery events and a 1 s ticker turns them into [OutgoingMediaUi].
 */
data class OutgoingMediaSnapshot(
    /** Channel messages cannot report a per-recipient receipt; copy differs slightly. */
    val isChannel: Boolean,
    val mime: String,
    val sizeBytes: Int,
    /** When this upload attempt started (reset on retry). */
    val sentAtMs: Long,
    /** Last ResourceProgress/delivery event for this message (reset on retry). */
    val lastActivityMs: Long,
    /** Expected wall-clock transfer time from the profile/region + duty cycle estimate. */
    val expectedWindowMs: Long,
    /** 0..1 when the core reports progress; null means indeterminate (no chunk events yet). */
    val progress: Float?,
    val attempts: Int = 1,
)

enum class OutgoingMediaUiState { SENDING, RETRYING, STALLED }

data class OutgoingMediaUi(
    val state: OutgoingMediaUiState,
    val progress: Float?,
    val elapsedMs: Long,
    val stalledForMs: Long,
    val attempts: Int,
)

object OutgoingMediaPolicy {
    /** Never call an upload stalled before this, even for tiny voice notes. */
    const val MIN_STALL_MS: Long = 45_000L

    /** Never make the user wait longer than this for a stall verdict on huge media. */
    const val MAX_STALL_MS: Long = 180_000L

    /** "No progress for ~2x the expected window" per the task. */
    const val STALL_WINDOW_FACTOR: Int = 2

    /** A retried upload is labelled "Retrying…" for this long before falling back to normal copy. */
    const val RETRYING_WINDOW_MS: Long = 10_000L

    /** After this long with no completion show the "radio uploads can take minutes" hint. */
    const val SLOW_HINT_AFTER_MS: Long = 8_000L

    fun stallThresholdMs(expectedWindowMs: Long): Long =
        min(MAX_STALL_MS, max(MIN_STALL_MS, expectedWindowMs.coerceAtLeast(0L) * STALL_WINDOW_FACTOR))

    fun initial(
        isChannel: Boolean,
        mime: String,
        sizeBytes: Int,
        sentAtMs: Long,
        expectedWindowMs: Long,
    ): OutgoingMediaSnapshot = OutgoingMediaSnapshot(
        isChannel = isChannel,
        mime = mime,
        sizeBytes = sizeBytes,
        sentAtMs = sentAtMs,
        lastActivityMs = sentAtMs,
        expectedWindowMs = expectedWindowMs,
        progress = null,
        attempts = 1,
    )

    /** Snapshot for a message loaded from the database (SENT but no in-memory progress yet). */
    fun fromMessage(message: Message, expectedWindowMs: Long): OutgoingMediaSnapshot? {
        if (!message.outgoing || message.kind != org.openmesh.android.data.model.MessageKind.ATTACHMENT) return null
        if (message.delivery != DeliveryState.SENT) return null
        val attachment = message.attachment ?: return null
        return initial(
            isChannel = false, // caller decides from the thread it came from
            mime = attachment.mime,
            sizeBytes = attachment.sizeBytes,
            sentAtMs = message.sentAtMs,
            expectedWindowMs = expectedWindowMs,
        )
    }

    fun uiState(snapshot: OutgoingMediaSnapshot, nowMs: Long): OutgoingMediaUi {
        val elapsed = (nowMs - snapshot.sentAtMs).coerceAtLeast(0L)
        val idle = (nowMs - snapshot.lastActivityMs).coerceAtLeast(0L)
        val threshold = stallThresholdMs(snapshot.expectedWindowMs)
        val state = when {
            idle > threshold -> OutgoingMediaUiState.STALLED
            snapshot.attempts > 1 && elapsed <= RETRYING_WINDOW_MS -> OutgoingMediaUiState.RETRYING
            else -> OutgoingMediaUiState.SENDING
        }
        return OutgoingMediaUi(
            state = state,
            progress = snapshot.progress,
            elapsedMs = elapsed,
            stalledForMs = if (state == OutgoingMediaUiState.STALLED) idle else 0L,
            attempts = snapshot.attempts,
        )
    }

    /** Reset timestamps and bump the attempt counter when the user retries an upload. */
    fun retried(snapshot: OutgoingMediaSnapshot, nowMs: Long): OutgoingMediaSnapshot = snapshot.copy(
        sentAtMs = nowMs,
        lastActivityMs = nowMs,
        progress = null,
        attempts = snapshot.attempts + 1,
    )

    fun progressPercent(progress: Float?): Int? =
        progress?.let { (it.coerceIn(0f, 1f) * 100f).roundToInt() }

    /** Short status shown beside the tick while an attachment is still uploading. */
    fun statusText(ui: OutgoingMediaUi): String = when (ui.state) {
        OutgoingMediaUiState.STALLED -> "Upload stalled — retry"
        OutgoingMediaUiState.RETRYING -> "Retrying…"
        OutgoingMediaUiState.SENDING ->
            progressPercent(ui.progress)?.let { "Uploading… $it%" } ?: "Uploading…"
    }

    fun isActive(delivery: DeliveryState): Boolean = delivery == DeliveryState.QUEUED ||
        delivery == DeliveryState.SENT

    fun slowHintVisible(ui: OutgoingMediaUi): Boolean =
        ui.state == OutgoingMediaUiState.SENDING && ui.elapsedMs >= SLOW_HINT_AFTER_MS

    /** Thread-level hint while at least one media upload is slow (never shown once stalled). */
    fun slowHintText(isChannel: Boolean): String = if (isChannel) {
        "Radio uploads can take minutes — this media continues in the background. " +
            "If it stalls you can retry from the message."
    } else {
        "Large media can take minutes on radio — it continues in the background. " +
            "If it stalls you can retry from the message."
    }

    /** Full explanation for the message-status dialog when the upload itself is stalled. */
    fun stalledExplanation(isChannel: Boolean): String = if (isChannel) {
        "This upload has made no progress for a while. Retry starts a fresh attempt with the same " +
            "file; the message stays in place and is not duplicated."
    } else {
        "This upload has made no progress for a while. Retry starts a fresh attempt with the same " +
            "file; the message stays in place and is not duplicated."
    }
}
