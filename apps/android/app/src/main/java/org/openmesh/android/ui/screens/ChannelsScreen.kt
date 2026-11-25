package org.openmesh.android.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.ui.components.InviteQr
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshSurfaceHigh

/** Channels: create with invite QR, join via camera/paste, open a channel thread. */
@Composable
fun ChannelsScreen(
    repository: MeshRepository,
    onBack: () -> Unit,
    onOpenThread: (channelId: String) -> Unit,
    onScan: () -> Unit,
) {
    val channels by repository.channels.collectAsState()
    val scope = rememberCoroutineScope()
    var showCreate by remember { mutableStateOf(false) }
    var showJoin by remember { mutableStateOf(false) }
    var name by remember { mutableStateOf("") }
    var joinText by remember { mutableStateOf("") }
    var invite by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }

    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back") }
            Column(Modifier.weight(1f)) {
                Text("Channels", style = MaterialTheme.typography.headlineSmall)
                Text("Private group chats · shared key, signed messages", color = MeshOnSurfaceVariant)
            }
            IconButton(onClick = onScan) { Icon(Icons.Filled.QrCodeScanner, contentDescription = "Scan channel invite") }
            IconButton(onClick = { showCreate = true }) { Icon(Icons.Filled.Add, contentDescription = "Create channel") }
        }
        Row(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = { showJoin = true }, modifier = Modifier.weight(1f)) { Text("Join with invite") }
            Button(onClick = { showCreate = true }, modifier = Modifier.weight(1f)) { Text("Create channel") }
        }
        LazyColumn(
            Modifier.fillMaxSize().padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            items(channels, key = { it.id }) { channel ->
                Card(
                    onClick = { onOpenThread(channel.id) },
                    colors = CardDefaults.cardColors(containerColor = MeshSurfaceHigh),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Row(Modifier.padding(14.dp), verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text("#${channel.name}", style = MaterialTheme.typography.titleMedium)
                            Text("Channel ${channel.id.take(8)}… · key set", style = MaterialTheme.typography.bodySmall, color = MeshOnSurfaceVariant)
                        }
                        if (channel.unreadCount > 0) Text("${channel.unreadCount} new", color = MaterialTheme.colorScheme.primary)
                    }
                }
            }
            if (channels.isEmpty()) {
                item {
                    Text(
                        "No channels yet. Create one and share its QR, or join with an invite. Channels are the easiest way to bring a group onto the mesh.",
                        color = MeshOnSurfaceVariant,
                    )
                }
            }
        }
    }

    if (showCreate) {
        AlertDialog(
            onDismissRequest = { showCreate = false },
            title = { Text("Create channel") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("The channel key is shown once. Anyone with the invite can read and write.", style = MaterialTheme.typography.bodySmall, color = MeshOnSurfaceVariant)
                    OutlinedTextField(value = name, onValueChange = { name = it.take(32) }, label = { Text("Name") }, singleLine = true)
                    error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    scope.launch {
                        error = null
                        val result = repository.createChannel(name.ifBlank { "family" })
                        result.onSuccess {
                            showCreate = false
                            invite = repository.channelInvite(it.channelIdHex) ?: it.inviteUrl
                        }.onFailure { error = it.message ?: "Could not create channel" }
                    }
                }) { Text("Create") }
            },
            dismissButton = { TextButton(onClick = { showCreate = false }) { Text("Cancel") } },
        )
    }

    if (showJoin) {
        AlertDialog(
            onDismissRequest = { showJoin = false },
            title = { Text("Join channel") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(
                        value = joinText,
                        onValueChange = { joinText = it },
                        label = { Text("openmesh://channel?… invite") },
                        minLines = 2,
                    )
                    TextButton(onClick = { onScan() }) { Text("Scan QR instead") }
                    error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    repository.importInvite(joinText)
                        .onSuccess { showJoin = false }
                        .onFailure { error = it.message ?: "Invalid invite" }
                }) { Text("Join") }
            },
            dismissButton = { TextButton(onClick = { showJoin = false }) { Text("Cancel") } },
        )
    }

    invite?.let { url ->
        AlertDialog(
            onDismissRequest = { invite = null },
            title = { Text("Channel invite") },
            text = {
                Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    InviteQr(url)
                    Text("Share this QR or the invite link over a trusted channel. The invite contains the channel key.", style = MaterialTheme.typography.bodySmall, color = MeshOnSurfaceVariant)
                }
            },
            confirmButton = { TextButton(onClick = { invite = null }) { Text("Done") } },
        )
    }
}
