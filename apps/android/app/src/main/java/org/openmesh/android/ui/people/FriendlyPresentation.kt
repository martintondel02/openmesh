package org.openmesh.android.ui.people

import org.openmesh.android.data.model.DeliveryState
import org.openmesh.android.data.model.Message
import org.openmesh.android.data.model.RadioStatus
import org.openmesh.android.data.transport.RadioHealth
import org.openmesh.android.data.transport.RadioPhase
import org.openmesh.android.ui.radio.RadioTone

/**
 * Pure presentation rules for the "people" surfaces: friendly display names, message delivery /
 * read status and peer link stability.
 *
 * Android/Compose-free so all copy and fallback decisions are unit-tested on the JVM
 * (`app/src/test/java/org/openmesh/android/ui/people/FriendlyPresentationTest.kt`). Copy follows
 * docs/design/ux-help-system.md §6 (plain language, explain the trade-off, no dead ends).
 */
object NamePolicy {
    /** Announce names are carried in one OMP frame; the core caps the name at 32 bytes (UTF-8). */
    const val MAX_NAME_BYTES: Int = 32

    data class Validation(val value: String? = null, val error: String? = null) {
        val ok: Boolean get() = error == null && value != null
    }

    /** Trim, reject empty/control characters and names that do not fit the announce frame. */
    fun validate(raw: String): Validation {
        val trimmed = raw.trim()
        if (trimmed.isEmpty()) return Validation(error = "Enter a display name.")
        if (trimmed.any { it.isISOControl() }) {
            return Validation(error = "Use plain text only — no line breaks or control characters.")
        }
        val bytes = trimmed.toByteArray(Charsets.UTF_8).size
        if (bytes > MAX_NAME_BYTES) {
            return Validation(
                error = "Keep the display name to $MAX_NAME_BYTES bytes or less " +
                    "(this one is $bytes bytes; emoji and accents use several bytes each).",
            )
        }
        return Validation(value = trimmed)
    }

    fun shortId(nodeId: String): String = nodeId.take(8)

    fun fallbackName(nodeId: String): String = "node ${shortId(nodeId)}"

    /**
     * Best name we can show for a peer.
     *
     * Order: channel name → the name the peer last announced → the locally pinned invite/contact
     * alias → "node <first8>". An announced rename therefore replaces a stale cached name; the
     * pinned invite name remains as a fallback only (there is no alias-editing UI).
     */
    fun displayName(
        peerId: String,
        announcedName: String?,
        inviteName: String? = null,
        channelName: String? = null,
    ): String {
        channelName?.takeIf { it.isNotBlank() }?.let { return it }
        announcedName?.takeIf { it.isNotBlank() }?.let { return it }
        inviteName?.takeIf { it.isNotBlank() }?.let { return it }
        return fallbackName(peerId)
    }

    /** "Ada Lovelace · 1a2b3c4d" — name and short id together wherever identity matters. */
    fun labelWithId(name: String, nodeId: String): String = "$name · ${shortId(nodeId)}"

    fun isFallbackName(name: String, nodeId: String): Boolean =
        name.isBlank() || name == fallbackName(nodeId)
}

/** Delivery/read presentation with explicit direct-message vs channel semantics. */
object DeliveryPresentation {
    fun label(state: DeliveryState): String = when (state) {
        DeliveryState.QUEUED -> "Queued"
        DeliveryState.SENT -> "Sent"
        DeliveryState.DELIVERED -> "Delivered"
        DeliveryState.READ -> "Read"
        DeliveryState.STORED_AT_RELAY -> "Stored at relay"
        DeliveryState.FAILED -> "Failed"
        DeliveryState.CANCELLED -> "Cancelled"
    }

    /** Short label shown next to the bubble icon (so status is never icon-only). */
    fun bubbleStatus(state: DeliveryState): String = when (state) {
        DeliveryState.QUEUED -> "Queued"
        DeliveryState.SENT -> "Sent"
        DeliveryState.DELIVERED -> "Delivered"
        DeliveryState.READ -> "Read"
        DeliveryState.STORED_AT_RELAY -> "At relay"
        DeliveryState.FAILED -> "Failed — tap to retry"
        DeliveryState.CANCELLED -> "Cancelled"
    }

    fun tone(state: DeliveryState): RadioTone = when (state) {
        DeliveryState.QUEUED -> RadioTone.NEUTRAL
        DeliveryState.SENT -> RadioTone.PROGRESS
        DeliveryState.DELIVERED -> RadioTone.POSITIVE
        DeliveryState.READ -> RadioTone.POSITIVE
        DeliveryState.STORED_AT_RELAY -> RadioTone.WARNING
        DeliveryState.FAILED -> RadioTone.ERROR
        DeliveryState.CANCELLED -> RadioTone.NEUTRAL
    }

    /**
     * Full, plain-language explanation for the status dialog. Channels are not sealed to one
     * recipient, so there is no per-person delivery/read receipt; direct messages are.
     */
    fun explanation(state: DeliveryState, isChannel: Boolean, peerName: String): String = when (state) {
        DeliveryState.QUEUED ->
            "Not sent yet. It will go out as soon as the radio link is ready — queued messages are " +
                "kept, not dropped."
        DeliveryState.SENT -> if (isChannel) {
            "Sent into this channel. Group messages have no per-person receipt: any node in range " +
                "relays them, so we cannot say who has seen it."
        } else {
            "Sent to $peerName's node. Waiting for the delivery confirmation."
        }
        DeliveryState.DELIVERED -> if (isChannel) {
            "Relayed into this channel. No per-person receipts exist for group messages."
        } else {
            "Delivered to $peerName's node. It is in their inbox and will be shown when they open " +
                "the chat; a read receipt arrives when they do."
        }
        DeliveryState.READ -> if (isChannel) {
            "Read receipts are not available for channel messages."
        } else {
            "$peerName has read this message."
        }
        DeliveryState.STORED_AT_RELAY ->
            "A store-and-forward relay is holding it until $peerName (or another relay) can pass it on."
        DeliveryState.FAILED ->
            "Sending failed. Open the message actions to retry, or check the radio link on the Radio screen."
        DeliveryState.CANCELLED ->
            "You cancelled this send — no more airtime is being used. The message stays in the " +
                "thread as a record; send it again if you still want it delivered."
    }

    /** Monotonic delivery order: progress never goes backwards on a status update. */
    fun rank(state: DeliveryState): Int = when (state) {
        DeliveryState.QUEUED -> 1
        DeliveryState.SENT -> 2
        DeliveryState.STORED_AT_RELAY -> 3
        DeliveryState.DELIVERED -> 4
        DeliveryState.READ -> 5
        DeliveryState.FAILED -> 0
        DeliveryState.CANCELLED -> 6
    }

    /** Whether a status update should replace the current one (never downgrade delivered/read). */
    fun isUpgrade(current: DeliveryState, next: DeliveryState): Boolean = when {
        current == next -> false
        next == DeliveryState.FAILED -> rank(current) < rank(DeliveryState.DELIVERED)
        // A retry (or a late acknowledgement after a failure) may recover the message.
        current == DeliveryState.FAILED -> true
        else -> rank(next) > rank(current)
    }

    data class Summary(
        val sent: Int,
        val delivered: Int,
        val read: Int,
        val failed: Int,
        val last: DeliveryState?,
    )

    fun summary(messages: List<Message>): Summary {
        val outgoing = messages.filter { it.outgoing }
        return Summary(
            sent = outgoing.size,
            delivered = outgoing.count { it.delivery == DeliveryState.DELIVERED || it.delivery == DeliveryState.READ },
            read = outgoing.count { it.delivery == DeliveryState.READ },
            failed = outgoing.count { it.delivery == DeliveryState.FAILED },
            last = outgoing.lastOrNull()?.delivery,
        )
    }

    fun summaryText(summary: Summary, isChannel: Boolean, peerName: String): String {
        if (summary.sent == 0) return "No messages sent yet."
        return if (isChannel) {
            buildString {
                append("${summary.sent} sent into this channel · no per-person receipts")
                if (summary.failed > 0) append(" · ${summary.failed} failed")
            }
        } else {
            buildString {
                append("${summary.sent} sent · ${summary.delivered} delivered · ${summary.read} read")
                if (summary.failed > 0) append(" · ${summary.failed} failed")
                append(" · to $peerName")
            }
        }
    }
}

/** Peer link stability vocabulary shared with the radio UX ("Strong / OK / Weak / Offline"). */
enum class LinkQuality(val label: String) {
    STRONG("Strong"),
    OK("OK"),
    WEAK("Weak"),
    OFFLINE("Offline"),
    UNKNOWN("No signal yet"),
}

object PeerLinkPresentation {
    /** A peer not heard for this long is treated as offline even if we have an old RSSI. */
    const val OFFLINE_AFTER_MS: Long = 5 * 60_000L

    fun quality(rssi: Int?, lastSeenMs: Long, nowMs: Long): LinkQuality {
        if (lastSeenMs <= 0L || nowMs - lastSeenMs > OFFLINE_AFTER_MS) return LinkQuality.OFFLINE
        if (rssi == null) return LinkQuality.UNKNOWN
        return when {
            rssi >= -80 -> LinkQuality.STRONG
            rssi >= -100 -> LinkQuality.OK
            else -> LinkQuality.WEAK
        }
    }

    /** Direct hop (0) vs relayed, in beginner words. */
    fun routeLabel(hops: Int): String = when {
        hops <= 0 -> "Direct link (0 hops)"
        hops == 1 -> "Via 1 relay"
        else -> "Via $hops relays"
    }

    fun lastHeardLabel(lastSeenMs: Long, nowMs: Long): String {
        if (lastSeenMs <= 0L) return "Not heard yet"
        val diff = (nowMs - lastSeenMs).coerceAtLeast(0L)
        return when {
            diff < 60_000L -> "Heard just now"
            diff < 3_600_000L -> "Heard ${diff / 60_000L} min ago"
            diff < 86_400_000L -> "Heard ${diff / 3_600_000L} h ago"
            else -> "Heard ${diff / 86_400_000L} d ago"
        }
    }

    fun signalLabel(rssi: Int?, snr: Float?): String = buildString {
        append("RSSI ${rssi?.let { "$it dBm" } ?: "—"}")
        if (snr != null) append(" · SNR $snr dB")
    }

    /** Compact radio state for the peer panel: what the phone's own link is doing right now. */
    fun radioSummary(radio: RadioStatus): String = when {
        radio.phase == RadioPhase.READY && radio.health == RadioHealth.HEALTHY ->
            "Radio ready · ${radio.transportName} · answering"
        radio.phase == RadioPhase.READY ->
            "Radio ready · ${radio.transportName} · not answering (stale link)"
        radio.phase == RadioPhase.ERROR ->
            "Radio needs attention — ${radio.lastError ?: "link error"}"
        radio.phase == RadioPhase.SCANNING || radio.phase == RadioPhase.CONNECTING ||
            radio.phase == RadioPhase.HANDSHAKING -> "Radio is connecting…"
        else -> "No radio connected — messages are queued"
    }
}
