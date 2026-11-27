package org.openmesh.android.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.openmesh.android.data.model.Message
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.ui.people.DeliveryPresentation
import org.openmesh.android.ui.people.LinkQuality
import org.openmesh.android.ui.people.NamePolicy
import org.openmesh.android.ui.people.PeerLinkPresentation
import org.openmesh.android.ui.radio.RadioTone
import org.openmesh.android.ui.theme.MeshError
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshPrimary
import org.openmesh.android.ui.theme.MeshTertiary

/**
 * Peer / link-stability panel, reachable from a chat header and from the Nodes list.
 *
 * Shows who the peer is (announced name + short id), how they are reachable (role, hops,
 * RSSI/SNR, last heard, quality), whether they are a pinned contact, the phone's own radio link
 * state and the delivery/read summary for this conversation. Channel variant explains the
 * "no per-person receipt" semantics instead of pretending to know who read a group message.
 */
@Composable
fun PeerDetailDialog(
    repository: MeshRepository,
    peerId: String,
    isChannel: Boolean,
    onDismiss: () -> Unit,
) {
    val nodes by repository.nodes.collectAsState()
    val contacts by repository.contacts.collectAsState()
    val channels by repository.channels.collectAsState()
    val radio by repository.radio.collectAsState()
    val directMessages by repository.messages.collectAsState()
    val channelMessages by repository.channelMessages.collectAsState()

    val node = nodes.firstOrNull { it.nodeId == peerId }
    val contact = contacts.firstOrNull { it.nodeId == peerId }
    val channel = channels.firstOrNull { it.id == peerId }
    val name = NamePolicy.displayName(
        peerId = peerId,
        announcedName = node?.name,
        inviteName = contact?.name,
        channelName = if (isChannel) channel?.name else null,
    )
    val shortId = NamePolicy.shortId(peerId)
    val thread: List<Message> = if (isChannel) {
        channelMessages[peerId].orEmpty()
    } else {
        directMessages[peerId].orEmpty()
    }
    val summary = DeliveryPresentation.summary(thread)
    val now = remember(peerId) { System.currentTimeMillis() }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = {
            Column {
                Text(if (isChannel) "#$name" else name)
                Text(
                    "Short id $shortId · unique and permanent",
                    style = MaterialTheme.typography.bodySmall,
                    color = MeshOnSurfaceVariant,
                )
            }
        },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                if (isChannel) {
                    Text("Channel · shared-key messaging", style = MaterialTheme.typography.bodyMedium)
                    Text(
                        if (channel?.pskSet == true) {
                            "Messages are encrypted with the channel key; every member can read them."
                        } else {
                            "This channel has no key set."
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = MeshOnSurfaceVariant,
                    )
                } else {
                    val quality = PeerLinkPresentation.quality(node?.rssi, node?.lastSeenMs ?: 0L, now)
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text("Link quality", style = MaterialTheme.typography.labelMedium)
                        StatusPill(
                            quality.label,
                            when (quality) {
                                LinkQuality.STRONG -> MeshPrimary
                                LinkQuality.OK -> MeshTertiary
                                LinkQuality.WEAK, LinkQuality.OFFLINE -> MeshError
                                LinkQuality.UNKNOWN -> MeshOnSurfaceVariant
                            },
                        )
                    }
                    Text("Role: ${node?.nodeClass?.label ?: "unknown"}")
                    Text("Route: ${PeerLinkPresentation.routeLabel(node?.hops ?: 0)}")
                    Text(PeerLinkPresentation.signalLabel(node?.rssi, node?.snr))
                    Text(PeerLinkPresentation.lastHeardLabel(node?.lastSeenMs ?: 0L, now))
                    Text(
                        if (contact != null) {
                            "Pinned contact" + if (contact.verified) " · invite verified" else ""
                        } else {
                            "Not a saved contact — name comes from their announce"
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = MeshOnSurfaceVariant,
                    )
                }

                Text(
                    "Your radio: ${PeerLinkPresentation.radioSummary(radio)}",
                    style = MaterialTheme.typography.bodySmall,
                    color = MeshOnSurfaceVariant,
                )

                Text(
                    DeliveryPresentation.summaryText(summary, isChannel = isChannel, peerName = name),
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (summary.failed > 0) MeshError else MeshOnSurfaceVariant,
                )
                summary.last?.let { state ->
                    Text(
                        "Last message: ${DeliveryPresentation.label(state)} — " +
                            DeliveryPresentation.explanation(state, isChannel, name),
                        style = MaterialTheme.typography.bodySmall,
                        color = MeshOnSurfaceVariant,
                    )
                }
                NodeIdText(peerId)
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Close") } },
    )
}

/** Small helper used by screens that need the same tone colours for status pills. */
fun linkQualityTone(quality: LinkQuality): RadioTone = when (quality) {
    LinkQuality.STRONG -> RadioTone.POSITIVE
    LinkQuality.OK -> RadioTone.PROGRESS
    LinkQuality.WEAK -> RadioTone.WARNING
    LinkQuality.OFFLINE -> RadioTone.ERROR
    LinkQuality.UNKNOWN -> RadioTone.NEUTRAL
}
