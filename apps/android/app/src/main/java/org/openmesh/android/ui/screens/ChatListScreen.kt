package org.openmesh.android.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Public
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.HelpOutline
import androidx.compose.material3.IconButton
import androidx.compose.material.icons.filled.Tag
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.openmesh.android.data.model.Conversation
import org.openmesh.android.data.model.PublicMesh
import org.openmesh.android.data.model.RadioStatus
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.ui.components.DeliveryIcon
import org.openmesh.android.ui.components.EmptyState
import org.openmesh.android.ui.components.RadioBanner
import org.openmesh.android.ui.components.SectionTitle
import org.openmesh.android.ui.components.StatusPill
import org.openmesh.android.ui.components.relativeTime
import org.openmesh.android.ui.radio.RadioPresentation
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshTertiary
import org.openmesh.android.ui.theme.MeshPrimary
import org.openmesh.android.ui.theme.MeshSurfaceHigh

/** Chats tab: direct conversations first, then channels, with a radio status banner. */
@Composable
fun ChatListScreen(
    repository: MeshRepository,
    radio: RadioStatus,
    onOpenThread: (peerId: String, channel: Boolean) -> Unit,
    onOpenRadio: () -> Unit,
    onOpenChannels: () -> Unit = {},
    onOpenHelp: () -> Unit = {},
) {
    val conversations by repository.conversations.collectAsState()
    val settings by repository.settings.collectAsState()
    val channels by repository.channels.collectAsState()
    val contacts by repository.contacts.collectAsState()
    var showNewChat by remember { mutableStateOf(false) }

    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text("OpenMesh", style = MaterialTheme.typography.headlineSmall)
                Text(
                    "Encrypted mesh messaging · offline by design",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MeshOnSurfaceVariant,
                )
            }
            IconButton(onClick = onOpenChannels) {
                Icon(Icons.Filled.Tag, contentDescription = "Channels", tint = MeshPrimary)
            }
            IconButton(onClick = onOpenHelp) {
                Icon(Icons.Filled.HelpOutline, contentDescription = "Help", tint = MeshPrimary)
            }
            Button(onClick = { showNewChat = true }) {
                Icon(Icons.Filled.Edit, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
                Text("New")
            }
        }

        LazyColumn(
            Modifier.fillMaxSize(),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(
                start = 16.dp, end = 16.dp, bottom = 24.dp,
            ),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            item {
                RadioBanner(radio = radio, demoMode = settings.demoMode, onClick = onOpenRadio)
            }
            if (!settings.demoMode && radio.queuedMessages > 0) {
                item {
                    QueuedMessagesCard(
                        count = radio.queuedMessages,
                        onOpenRadio = onOpenRadio,
                    )
                }
            }
            item { Spacer(Modifier.height(4.dp)) }
            item { SectionTitle("Conversations") }
            if (conversations.isEmpty()) {
                item {
                    EmptyState(
                        "No conversations yet",
                        "Scan a contact QR or pick a contact to start. Messages are encrypted end-to-end " +
                            "between nodes; relays only see ciphertext.",
                    )
                }
            }
            items(conversations, key = { it.peerId }) { conversation ->
                ConversationRow(conversation, onClick = { onOpenThread(conversation.peerId, false) })
            }
            item { Spacer(Modifier.height(8.dp)) }
            item { SectionTitle("Channels") }
            items(PublicMesh.pinFirst(channels), key = { it.id }) { channel ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clip(MaterialTheme.shapes.medium)
                        .background(MeshSurfaceHigh.copy(alpha = 0.5f))
                        .clickable { onOpenThread(channel.id, true) }
                        .padding(horizontal = 12.dp, vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(
                        if (channel.isPublic) Icons.Filled.Public else Icons.Filled.Tag,
                        contentDescription = null,
                        tint = if (channel.isPublic) MeshTertiary else MeshPrimary,
                    )
                    Spacer(Modifier.width(12.dp))
                    Column(Modifier.weight(1f)) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text("#${channel.name}", style = MaterialTheme.typography.titleMedium)
                            if (channel.isPublic) {
                                Spacer(Modifier.width(8.dp))
                                StatusPill("PUBLIC", MeshTertiary)
                            }
                        }
                        Text(
                            when {
                                channel.isPublic -> PublicMesh.shortWarning()
                                channel.pskSet -> "Shared key channel"
                                else -> "No key set"
                            },
                            style = MaterialTheme.typography.bodyMedium,
                            color = MeshOnSurfaceVariant,
                        )
                    }
                    if (channel.unreadCount > 0) {
                        UnreadBadge(channel.unreadCount)
                    }
                }
            }
            item { Spacer(Modifier.height(8.dp)) }
            item {
                Text(
                    "Airtime estimate is shown before sending media. On 1 % duty-cycle bands a " +
                        "5 KiB image can take minutes of channel time — the dialog tells you.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MeshOnSurfaceVariant,
                    modifier = Modifier.padding(4.dp),
                )
            }
        }
    }

    if (showNewChat) {
        AlertDialog(
            onDismissRequest = { showNewChat = false },
            title = { Text("Start a conversation") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    if (contacts.isEmpty()) {
                        Text("No contacts yet. Scan a contact QR from the Contacts tab.")
                    }
                    contacts.forEach { contact ->
                        TextButton(onClick = {
                            showNewChat = false
                            onOpenThread(contact.nodeId, false)
                        }) {
                            Column(Modifier.fillMaxWidth()) {
                                Text(
                                    repository.displayNameFor(contact.nodeId),
                                    style = MaterialTheme.typography.titleMedium,
                                )
                                Text(
                                    contact.nodeId,
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MeshOnSurfaceVariant,
                                )
                            }
                        }
                    }
                }
            },
            confirmButton = { TextButton(onClick = { showNewChat = false }) { Text("Close") } },
        )
    }
}

/** Inline reminder that messages are waiting for a radio link (chat list and thread). */
@Composable
fun QueuedMessagesCard(count: Int, onOpenRadio: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clip(MaterialTheme.shapes.medium)
            .background(MeshTertiary.copy(alpha = 0.12f))
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(
                RadioPresentation.queuedCardText(count),
                style = MaterialTheme.typography.bodyMedium,
                color = MeshTertiary,
            )
        }
        TextButton(onClick = onOpenRadio) { Text("Connect") }
    }
}

@Composable
private fun ConversationRow(conversation: Conversation, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clip(MaterialTheme.shapes.medium)
            .background(MeshSurfaceHigh.copy(alpha = 0.5f))
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .size(42.dp)
                .clip(CircleShape)
                .background(MeshPrimary.copy(alpha = 0.15f)),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                conversation.peerName.take(1).uppercase(),
                style = MaterialTheme.typography.titleMedium,
                color = MeshPrimary,
            )
        }
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    conversation.peerName,
                    style = MaterialTheme.typography.titleMedium,
                    modifier = Modifier.weight(1f),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    relativeTime(conversation.lastMessage?.sentAtMs ?: 0L),
                    style = MaterialTheme.typography.labelMedium,
                    color = MeshOnSurfaceVariant,
                )
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                val message = conversation.lastMessage
                if (message?.outgoing == true) {
                    DeliveryIcon(message.delivery)
                    Spacer(Modifier.width(4.dp))
                }
                Text(
                    message?.text ?: "No messages yet",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MeshOnSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                if (conversation.unreadCount > 0) UnreadBadge(conversation.unreadCount)
            }
        }
    }
}

@Composable
private fun UnreadBadge(count: Int) {
    Surface(color = MeshPrimary, shape = CircleShape) {
        Text(
            count.toString(),
            style = MaterialTheme.typography.labelMedium,
            color = Color.Black,
            modifier = Modifier.padding(horizontal = 8.dp, vertical = 2.dp),
        )
    }
}
