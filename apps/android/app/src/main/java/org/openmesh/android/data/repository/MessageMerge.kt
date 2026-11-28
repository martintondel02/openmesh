package org.openmesh.android.data.repository

import org.openmesh.android.data.model.DeliveryState
import org.openmesh.android.data.model.Message
import org.openmesh.android.data.model.MessageKind

/**
 * Additive merge for two reports of the same engine message id.
 *
 * The companion/FFI stream can describe one logical message with several events, in any order:
 *  * `GroupMessage` — the text/caption event (maps to [MessageKind.TEXT]),
 *  * one resource for an oversized group-message envelope (`application/cbor`; ignored by the
 *    repository — it is a transport artifact, never a user attachment),
 *  * one resource for the actual attachment payload (an image or audio mime, …).
 *
 * The old "replace the row" upsert dropped whichever half arrived first (attachment metadata, or
 * the text), and an own message echoed back by the mesh could flip an outgoing bubble to incoming.
 * This merge never loses information already held, and prefers richer/newer values when both
 * copies carry them. Pure Kotlin so it is unit-tested on the JVM (`MessageMergeTest`).
 */
object MessageMerge {

    fun merge(existing: Message?, incoming: Message): Message {
        if (existing == null || existing.id != incoming.id) return incoming
        val attachment = incoming.attachment ?: existing.attachment
        val outgoing = existing.outgoing || incoming.outgoing
        val text = incoming.text?.takeIf { it.isNotBlank() } ?: existing.text
        val sentAtMs = listOf(existing.sentAtMs, incoming.sentAtMs)
            .filter { it > 0L }
            .minOrNull()
            ?: incoming.sentAtMs
        return incoming.copy(
            peerId = incoming.peerId.ifBlank { existing.peerId },
            // For an echo of our own message keep the local sender id.
            fromId = if (existing.outgoing) existing.fromId else incoming.fromId.ifBlank { existing.fromId },
            toId = incoming.toId.ifBlank { existing.toId },
            kind = if (attachment != null) MessageKind.ATTACHMENT else incoming.kind,
            text = text,
            sentAtMs = sentAtMs,
            receivedAtMs = incoming.receivedAtMs ?: existing.receivedAtMs,
            outgoing = outgoing,
            delivery = mergeDelivery(existing.delivery, incoming.delivery),
            read = existing.read || incoming.read,
            attachment = attachment,
            position = incoming.position ?: existing.position,
            viaStore = existing.viaStore || incoming.viaStore,
            bridged = existing.bridged || incoming.bridged,
        )
    }

    /** Progress never goes backwards; a delivered copy wins over queued/sent, FAILED is sticky. */
    fun mergeDelivery(existing: DeliveryState, incoming: DeliveryState): DeliveryState = when {
        // The user cancelled this send; a late retransmit/ack must not resurrect it.
        existing == DeliveryState.CANCELLED || incoming == DeliveryState.CANCELLED -> DeliveryState.CANCELLED
        incoming == DeliveryState.DELIVERED || existing == DeliveryState.DELIVERED -> DeliveryState.DELIVERED
        existing == DeliveryState.FAILED -> DeliveryState.FAILED
        incoming == DeliveryState.FAILED -> DeliveryState.FAILED
        existing == DeliveryState.STORED_AT_RELAY || incoming == DeliveryState.STORED_AT_RELAY ->
            DeliveryState.STORED_AT_RELAY
        existing == DeliveryState.SENT || incoming == DeliveryState.SENT -> DeliveryState.SENT
        else -> DeliveryState.QUEUED
    }
}
