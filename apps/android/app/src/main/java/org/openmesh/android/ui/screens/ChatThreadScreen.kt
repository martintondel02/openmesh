package org.openmesh.android.ui.screens

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
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
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.AttachFile
import androidx.compose.material.icons.filled.Image
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.PhotoCamera
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Send
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.openmesh.android.data.model.DeliveryState
import org.openmesh.android.data.model.Message
import org.openmesh.android.data.model.MessageKind
import org.openmesh.android.data.repository.MediaKind
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.media.ImageCompressor
import org.openmesh.android.ui.components.AirtimeEstimateDialog
import org.openmesh.android.ui.components.DeliveryIcon
import org.openmesh.android.ui.components.formatDuration
import org.openmesh.android.ui.components.relativeTime
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshOutline
import org.openmesh.android.ui.theme.MeshPrimary
import org.openmesh.android.ui.theme.MeshSurfaceHigh
import org.openmesh.android.ui.theme.MeshTertiary

@Composable
fun ChatThreadScreen(
    repository: MeshRepository,
    peerId: String,
    isChannel: Boolean,
    onBack: () -> Unit,
    onOpenHelp: (String) -> Unit = {},
) {
    val context = LocalContext.current
    val directMessages by repository.messages.collectAsState()
    val channelMessages by repository.channelMessages.collectAsState()
    val thread = if (isChannel) channelMessages[peerId].orEmpty() else directMessages[peerId].orEmpty()
    val settings by repository.settings.collectAsState()
    val listState = rememberLazyListState()

    var draft by remember { mutableStateOf("") }
    var pickedImage by remember { mutableStateOf<Uri?>(null) }
    var showVoiceDialog by remember { mutableStateOf(false) }

    LaunchedEffect(thread.size) {
        if (thread.isNotEmpty()) listState.animateScrollToItem(thread.lastIndex)
    }
    LaunchedEffect(peerId, isChannel) {
        repository.markThreadRead(peerId, isChannel)
    }

    var cameraUri by remember { mutableStateOf<Uri?>(null) }
    val imagePicker = rememberLauncherForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        if (uri != null) pickedImage = uri
    }
    val cameraLauncher = rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { success ->
        if (success) pickedImage = cameraUri
    }

    Column(Modifier.fillMaxSize().imePadding()) {
        Row(
            Modifier
                .fillMaxWidth()
                .background(MeshSurfaceHigh.copy(alpha = 0.35f))
                .padding(horizontal = 4.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onBack) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
            }
            Column(Modifier.weight(1f)) {
                Text(
                    if (isChannel) "#${repository.displayNameFor(peerId)}" else repository.displayNameFor(peerId),
                    style = MaterialTheme.typography.titleLarge,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    if (isChannel) "Channel · ${settings.profile.replace('_', ' ').lowercase()} · ${settings.region}"
                    else "End-to-end encrypted · ${settings.profile.replace('_', ' ').lowercase()} · ${settings.region}",
                    style = MaterialTheme.typography.labelMedium,
                    color = MeshOnSurfaceVariant,
                )
            }
        }

        LazyColumn(
            state = listState,
            modifier = Modifier.weight(1f).fillMaxWidth(),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            items(thread, key = { it.id }) { message -> MessageBubble(message, isChannel, repository) }
            if (thread.isEmpty()) {
                item {
                    Text(
                        "No messages yet. Say hello — it will be sealed once and relayed by any node in range.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MeshOnSurfaceVariant,
                        modifier = Modifier.padding(16.dp),
                    )
                }
            }
        }

        Row(
            Modifier
                .fillMaxWidth()
                .background(MeshSurfaceHigh.copy(alpha = 0.5f))
                .navigationBarsPadding()
                .padding(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = { imagePicker.launch("image/*") }) {
                Icon(Icons.Filled.Image, contentDescription = "Attach image", tint = MeshPrimary)
            }
            IconButton(onClick = {
                val dir = java.io.File(context.cacheDir, "captures").apply { mkdirs() }
                val file = java.io.File(dir, "capture-${System.currentTimeMillis()}.jpg")
                cameraUri = androidx.core.content.FileProvider.getUriForFile(
                    context,
                    "${context.packageName}.fileprovider",
                    file,
                )
                cameraLauncher.launch(cameraUri!!)
            }) {
                Icon(Icons.Filled.PhotoCamera, contentDescription = "Take photo", tint = MeshPrimary)
            }
            IconButton(onClick = { showVoiceDialog = true }) {
                Icon(Icons.Filled.Mic, contentDescription = "Record voice note", tint = MeshPrimary)
            }
            OutlinedTextField(
                value = draft,
                onValueChange = { draft = it },
                modifier = Modifier.weight(1f),
                placeholder = { Text("Message") },
                maxLines = 4,
                shape = RoundedCornerShape(20.dp),
            )
            Spacer(Modifier.width(6.dp))
            IconButton(
                onClick = {
                    if (draft.isNotBlank()) {
                        repository.sendText(peerId, draft, isChannel)
                        draft = ""
                    }
                },
                enabled = draft.isNotBlank(),
            ) {
                Icon(Icons.Filled.Send, contentDescription = "Send", tint = MeshPrimary)
            }
        }
    }

    pickedImage?.let { uri ->
        org.openmesh.android.ui.components.ImageTierSendDialog(
            repository = repository,
            peerId = peerId,
            isChannel = isChannel,
            uri = uri,
            onDismiss = { pickedImage = null },
        )
    }

    if (showVoiceDialog) {
        org.openmesh.android.ui.components.VoiceTierRecordDialog(
            repository = repository,
            peerId = peerId,
            isChannel = isChannel,
            onDismiss = { showVoiceDialog = false },
        )
    }
}


@Composable
private fun MessageBubble(message: Message, isChannel: Boolean, repository: MeshRepository) {
    val outgoing = message.outgoing
    val bubbleColor = if (outgoing) MeshPrimary.copy(alpha = 0.16f) else MeshSurfaceHigh
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = if (outgoing) Arrangement.End else Arrangement.Start,
    ) {
        Column(
            Modifier
                .fillMaxWidth(0.82f)
                .clip(RoundedCornerShape(14.dp))
                .background(bubbleColor)
                .padding(horizontal = 12.dp, vertical = 8.dp),
        ) {
            if (!outgoing && isChannel) {
                Text(
                    message.fromId.take(8),
                    style = MaterialTheme.typography.labelSmall,
                    color = MeshTertiary,
                )
            }
            when (message.kind) {
                MessageKind.ATTACHMENT -> AttachmentBubble(message, repository)
                MessageKind.POSITION -> Column {
                    Text("Position", style = MaterialTheme.typography.titleMedium)
                    message.position?.let {
                        Text(
                            "%.5f, %.5f".format(it.lat, it.lon) +
                                (it.altM?.let { alt -> " · ${alt} m" } ?: ""),
                            style = MaterialTheme.typography.labelMedium,
                            color = MeshOnSurfaceVariant,
                        )
                    }
                }
                MessageKind.TELEMETRY -> Column {
                    Text("Telemetry", style = MaterialTheme.typography.titleMedium)
                    Text(
                        message.text ?: "Battery and airtime statistics",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MeshOnSurfaceVariant,
                    )
                }
                MessageKind.TEXT -> Text(message.text ?: "")
            }
            Spacer(Modifier.height(4.dp))
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.End,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (message.viaStore) {
                    Text(
                        "via store",
                        style = MaterialTheme.typography.labelSmall,
                        color = MeshTertiary,
                    )
                    Spacer(Modifier.width(6.dp))
                }
                if (message.bridged) {
                    Text("bridge", style = MaterialTheme.typography.labelSmall, color = MeshTertiary)
                    Spacer(Modifier.width(6.dp))
                }
                Text(
                    relativeTime(message.sentAtMs),
                    style = MaterialTheme.typography.labelSmall,
                    color = MeshOnSurfaceVariant,
                )
                if (outgoing) {
                    Spacer(Modifier.width(4.dp))
                    DeliveryIcon(message.delivery)
                }
            }
        }
    }
}

@Composable
private fun AttachmentBubble(message: Message, repository: MeshRepository) {
    val attachment = message.attachment ?: return
    val context = androidx.compose.ui.platform.LocalContext.current
    val player = remember { org.openmesh.android.media.AttachmentPlayer(context) }
    var playing by remember { mutableStateOf(false) }
    val isAudio = attachment.mime.startsWith("audio/")
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .then(
                if (isAudio) {
                    Modifier.clickable {
                        val file = repository.attachmentFile(attachment.id)
                        if (file != null) {
                            playing = !playing
                            if (playing) {
                                player.play(file, attachment.mime) { playing = false }
                            } else {
                                player.stop()
                            }
                        }
                    }
                } else {
                    Modifier
                },
            ),
    ) {
        Box(
            Modifier
                .size(44.dp)
                .clip(RoundedCornerShape(10.dp))
                .background(MeshOutline.copy(alpha = 0.5f)),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                if (attachment.mime.startsWith("audio")) Icons.Filled.PlayArrow else Icons.Filled.Image,
                contentDescription = null,
                tint = MeshPrimary,
            )
        }
        Spacer(Modifier.width(10.dp))
        Column(Modifier.weight(1f)) {
            Text(
                attachment.name ?: message.text ?: "Attachment",
                style = MaterialTheme.typography.titleMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                org.openmesh.android.media.MediaTierPolicy.attachmentLabel(
                    attachment.mime,
                    attachment.durationMs,
                    attachment.sizeBytes,
                ) + if (playing) " · playing…" else "",
                style = MaterialTheme.typography.labelMedium,
                color = if (isAudio) MeshPrimary else MeshOnSurfaceVariant,
            )
        }
    }
    if (message.delivery == DeliveryState.FAILED) {
        Text(
            "Transfer failed — long-press the message to retry.",
            style = MaterialTheme.typography.labelMedium,
            color = MeshOnSurfaceVariant,
        )
    }
}

@Composable
private fun VoiceRecorderDialog(maxSeconds: Int, onRecord: (Int) -> Unit, onDismiss: () -> Unit) {
    var seconds by remember { mutableFloatStateOf(8f) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Voice note") },
        text = {
            Column {
                Text("Duration: ${seconds.toInt()} s")
                Slider(
                    value = seconds,
                    onValueChange = { seconds = it },
                    valueRange = 2f..maxSeconds.toFloat(),
                )
                Text(
                    "Recording uses Opus (~7 kbps). The next dialog shows the airtime cost before sending; " +
                        "on slow profiles a much shorter clip is recommended.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MeshOnSurfaceVariant,
                )
            }
        },
        confirmButton = {
            TextButton(onClick = { onRecord(seconds.toInt()) }) { Text("Review cost") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}
