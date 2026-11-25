package org.openmesh.android.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ContentPaste
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
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
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.openmesh.android.data.model.Contact
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.ui.components.EmptyState
import org.openmesh.android.ui.components.MeshCard
import org.openmesh.android.ui.components.NodeIdText
import org.openmesh.android.ui.components.SectionTitle
import org.openmesh.android.ui.components.StatusPill
import org.openmesh.android.ui.components.relativeTime
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshPrimary
import org.openmesh.android.ui.theme.MeshSecondary
import org.openmesh.android.ui.theme.MeshSurfaceHigh
import org.openmesh.android.ui.theme.MeshTertiary

/** Contacts tab: QR scan/import, verification state, bridged contacts. */
@Composable
fun ContactsScreen(repository: MeshRepository, onScan: () -> Unit) {
    val contacts by repository.contacts.collectAsState()
    var showPaste by remember { mutableStateOf(false) }
    var pasteText by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }

    Column(Modifier.fillMaxSize()) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp)) {
            Text("Contacts", style = MaterialTheme.typography.headlineSmall)
            Text(
                "Invites carry the node id and public identity; verification pins the key to the id.",
                style = MaterialTheme.typography.bodyMedium,
                color = MeshOnSurfaceVariant,
            )
            Spacer(Modifier.height(10.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(onClick = onScan) {
                    Icon(Icons.Filled.QrCodeScanner, contentDescription = null, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("Scan QR")
                }
                OutlinedButton(onClick = { showPaste = true }) {
                    Icon(Icons.Filled.ContentPaste, contentDescription = null, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("Paste link")
                }
            }
        }

        LazyColumn(
            Modifier.fillMaxSize(),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            item { SectionTitle("Saved contacts") }
            if (contacts.isEmpty()) {
                item {
                    EmptyState(
                        "No contacts yet",
                        "Scan a contact QR from another OpenMesh client, or paste an openmesh://contact link. " +
                            "You can still receive messages from unknown nodes; adding them as contacts pins their keys.",
                    )
                }
            }
            items(contacts, key = { it.nodeId }) { contact ->
                ContactRow(contact, onRemove = { repository.removeContact(contact.nodeId) })
            }
            item { Spacer(Modifier.height(8.dp)) }
            item {
                MeshCard {
                    Column(Modifier.padding(14.dp)) {
                        Text("What this QR contains", style = MaterialTheme.typography.titleMedium)
                        Text(
                            "openmesh://contact?node_id=…&name=…&identity=… — 8-byte self-certifying id plus " +
                                "the 72-byte Ed25519+X25519 public identity. The id is a hash of the keys, so a " +
                                "mismatched identity is rejected.",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MeshOnSurfaceVariant,
                        )
                    }
                }
            }
        }
    }

    if (showPaste) {
        AlertDialog(
            onDismissRequest = { showPaste = false },
            title = { Text("Paste invite link") },
            text = {
                Column {
                    OutlinedTextField(
                        value = pasteText,
                        onValueChange = { pasteText = it; error = null },
                        placeholder = { Text("openmesh://contact?…") },
                        modifier = Modifier.fillMaxWidth(),
                        maxLines = 3,
                    )
                    error?.let {
                        Spacer(Modifier.height(6.dp))
                        Text(it, color = MaterialTheme.colorScheme.error)
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    repository.importInvite(pasteText).onSuccess {
                        showPaste = false
                        pasteText = ""
                    }.onFailure { error = it.message ?: "Invalid invite" }
                }) { Text("Import") }
            },
            dismissButton = { TextButton(onClick = { showPaste = false }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun ContactRow(contact: Contact, onRemove: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(MeshSurfaceHigh.copy(alpha = 0.55f))
            .padding(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    contact.name,
                    style = MaterialTheme.typography.titleMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                if (contact.verified) {
                    Icon(Icons.Filled.Verified, contentDescription = "verified", tint = MeshPrimary, modifier = Modifier.size(16.dp))
                }
            }
            NodeIdText(contact.nodeId)
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                if (contact.bridged) StatusPill("bridged", MeshTertiary) else StatusPill("mesh", MeshSecondary)
                Text(
                    "last seen ${relativeTime(contact.lastSeenMs)}",
                    style = MaterialTheme.typography.labelMedium,
                    color = MeshOnSurfaceVariant,
                )
            }
        }
        IconButton(onClick = onRemove) {
            Icon(Icons.Filled.Delete, contentDescription = "Remove contact", tint = MeshOnSurfaceVariant)
        }
    }
}
