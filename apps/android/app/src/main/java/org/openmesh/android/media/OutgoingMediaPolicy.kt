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
    /** Number of automatic retries already used for this attempt chain (manual retries reset it). */
    val autoRetries: Int = 0,
    /** When the next automatic retry is due; null while no backoff is scheduled. */
    val nextAutoRetryAtMs: Long? = null,
    /**
     * Row was restored from disk on app start (not sent in this process): automatic retries are
     * forbidden until the user explicitly taps Retry (prevents stale uploads contaminating tests
     * or flooding the companion after a restart).
     */
    val manualOnly: Boolean = false,
)

enum class OutgoingMediaUiState { SENDING, RETRYING, STALLED, AUTO_RETRY_WAIT }

data class OutgoingMediaUi(
    val state: OutgoingMediaUiState,
    val progress: Float?,
    val elapsedMs: Long,
    val stalledForMs: Long,
    val attempts: Int,
    val autoRetries: Int = 0,
    /** Milliseconds until the next automatic attempt while [state] is AUTO_RETRY_WAIT. */
    val retryInMs: Long = 0L,
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

    /** Automatic retry backoff: 30 s, 60 s, 120 s, … capped at 15 min. */
    const val AUTO_RETRY_BASE_MS: Long = 30_000L

    /** Never wait longer than this between automatic attempts. */
    const val AUTO_RETRY_MAX_MS: Long = 15 * 60_000L

    /** After this many automatic attempts the upload needs the user's manual Retry. */
    const val MAX_AUTO_RETRIES: Int = 3

    /** A transfer is considered in flight if any resource activity happened this recently. */
    const val TRANSFER_IN_FLIGHT_WINDOW_MS: Long = 15_000L

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
        val stalled = idle > threshold
        val retryAt = snapshot.nextAutoRetryAtMs
        val waitingForAutoRetry = stalled &&
            snapshot.autoRetries < MAX_AUTO_RETRIES &&
            retryAt != null &&
            retryAt > nowMs
        val state = when {
            waitingForAutoRetry -> OutgoingMediaUiState.AUTO_RETRY_WAIT
            stalled -> OutgoingMediaUiState.STALLED
            snapshot.attempts > 1 && elapsed <= RETRYING_WINDOW_MS -> OutgoingMediaUiState.RETRYING
            else -> OutgoingMediaUiState.SENDING
        }
        return OutgoingMediaUi(
            state = state,
            progress = snapshot.progress,
            elapsedMs = elapsed,
            stalledForMs = if (stalled) idle else 0L,
            attempts = snapshot.attempts,
            autoRetries = snapshot.autoRetries,
            retryInMs = if (waitingForAutoRetry) retryAt!! - nowMs else 0L,
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
        OutgoingMediaUiState.STALLED -> "Stalled — tap Retry"
        OutgoingMediaUiState.AUTO_RETRY_WAIT -> "Retrying in ${retryInSeconds(ui)} s"
        OutgoingMediaUiState.RETRYING -> "Retrying…"
        OutgoingMediaUiState.SENDING ->
            progressPercent(ui.progress)?.let { "Uploading… $it%" } ?: "Uploading…"
    }

    fun retryInSeconds(ui: OutgoingMediaUi): Long = ((ui.retryInMs + 999L) / 1000L).coerceAtLeast(1L)

    /** Delay before the next automatic attempt, given how many automatic attempts were used. */
    fun autoRetryDelayMs(autoRetries: Int): Long {
        val exponent = autoRetries.coerceIn(0, 20)
        var delay = AUTO_RETRY_BASE_MS
        repeat(exponent) {
            delay = if (delay >= AUTO_RETRY_MAX_MS / 2) AUTO_RETRY_MAX_MS else delay * 2
        }
        return delay.coerceAtMost(AUTO_RETRY_MAX_MS)
    }

    /** Schedule the next automatic attempt after the current stall. */
    fun scheduleAutoRetry(snapshot: OutgoingMediaSnapshot, nowMs: Long): OutgoingMediaSnapshot =
        snapshot.copy(nextAutoRetryAtMs = nowMs + autoRetryDelayMs(snapshot.autoRetries))

    fun autoRetryAllowed(snapshot: OutgoingMediaSnapshot): Boolean =
        snapshot.autoRetries < MAX_AUTO_RETRIES && !snapshot.manualOnly

    fun isAutoRetryDue(snapshot: OutgoingMediaSnapshot, nowMs: Long): Boolean =
        autoRetryAllowed(snapshot) && snapshot.nextAutoRetryAtMs != null && nowMs >= snapshot.nextAutoRetryAtMs

    /**
     * Only one automatic retry may run at a time, the link must be READY and no other transfer may
     * be in flight (resource activity within [TRANSFER_IN_FLIGHT_WINDOW_MS]) so an automatic retry
     * can never flood the companion TX path.
     */
    fun canStartAutoRetry(
        radioReady: Boolean,
        transferInFlight: Boolean,
        anotherAutoRetryRunning: Boolean,
    ): Boolean = radioReady && !transferInFlight && !anotherAutoRetryRunning

    /** Pick the single due upload to auto-retry this tick (oldest activity first). */
    fun pickAutoRetryCandidate(
        snapshots: Map<Long, OutgoingMediaSnapshot>,
        nowMs: Long,
    ): Long? = snapshots.entries
        .filter { isAutoRetryDue(it.value, nowMs) }
        .minByOrNull { it.value.lastActivityMs }
        ?.key

    /** Mark an automatic attempt as started (increments counters + retry timestamps). */
    fun markAutoRetryStarted(snapshot: OutgoingMediaSnapshot, nowMs: Long): OutgoingMediaSnapshot =
        retried(snapshot, nowMs).copy(
            autoRetries = snapshot.autoRetries + 1,
            nextAutoRetryAtMs = null,
        )

    /** Manual retry resets the automatic budget and opts a restored row back into live tracking. */
    fun markManualRetry(snapshot: OutgoingMediaSnapshot, nowMs: Long): OutgoingMediaSnapshot =
        retried(
            snapshot.copy(autoRetries = 0, nextAutoRetryAtMs = null, manualOnly = false),
            nowMs,
        )

    fun isActive(delivery: DeliveryState): Boolean = delivery == DeliveryState.QUEUED ||
        delivery == DeliveryState.SENT

    /** Cancel is only meaningful while the send can still consume airtime. */
    fun canCancel(delivery: DeliveryState): Boolean = delivery == DeliveryState.QUEUED ||
        delivery == DeliveryState.SENT || delivery == DeliveryState.FAILED

    /**
     * Cancel transition: stop scheduling automatic retries and drop any progress. The caller removes
     * the upload tracker once the core's cancelled event settles the delivery state.
     */
    fun cancelled(snapshot: OutgoingMediaSnapshot): OutgoingMediaSnapshot = snapshot.copy(
        progress = null,
        nextAutoRetryAtMs = null,
    )

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
    fun stalledExplanation(isChannel: Boolean): String {
        val channelNote = if (isChannel) " Channel messages have no per-person receipt. " else " "
        return "This upload has made no progress for a while. Retry starts a fresh attempt with the " +
            "same file; the message stays in place and is not duplicated." + channelNote +
            "After $MAX_AUTO_RETRIES automatic attempts it waits for you to tap Retry."
    }

    fun autoRetryExplanation(ui: OutgoingMediaUi): String =
        "No progress yet. A background retry will start in ${retryInSeconds(ui)} s " +
            "(attempt ${ui.autoRetries + 1} of $MAX_AUTO_RETRIES); you can retry now instead."

    fun autoRetryExhaustedExplanation(): String =
        "Automatic retries are used up. Tap Retry to start a fresh attempt with the same file."
}
