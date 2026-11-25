package org.openmesh.android.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Bluetooth
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Groups
import androidx.compose.material.icons.filled.Hub
import androidx.compose.material.icons.filled.LocationSearching
import androidx.compose.material.icons.filled.Router
import androidx.compose.material.icons.filled.Shield
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.openmesh.android.data.presets.BeginnerAnswers
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.ui.components.InviteQr
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshPrimary
import org.openmesh.android.ui.theme.MeshSurfaceHigh
import org.openmesh.android.ui.theme.MeshTertiary

private data class RoleOption(
    val id: String,
    val title: String,
    val blurb: String,
    val icon: androidx.compose.ui.graphics.vector.ImageVector,
)

private val roles = listOf(
    RoleOption("client", "Phone or handheld", "Send and receive from this device. The usual choice.", Icons.Filled.Bluetooth),
    RoleOption("tracker", "Tracker", "Battery-friendly position beacon for a bike, boat or pet.", Icons.Filled.LocationSearching),
    RoleOption("repeater", "Repeater", "Solar/roof relay that extends range and holds mail for offline nodes.", Icons.Filled.Router),
    RoleOption("gateway", "Gateway", "Bridges the local mesh to a backbone or another network.", Icons.Filled.Hub),
)

/**
 * Three-step beginner onboarding (docs/design/ux-help-system.md §3): what is this box → where are
 * you → secure my node. Applies the shared preset locally and creates the private channel invite.
 */
@Composable
fun OnboardingScreen(
    repository: MeshRepository,
    onFinished: () -> Unit,
) {
    var step by remember { mutableIntStateOf(0) }
    var role by remember { mutableStateOf("client") }
    var country by remember { mutableStateOf("NO") }
    var name by remember { mutableStateOf("openmesh-android") }
    var keepPrivate by remember { mutableStateOf(true) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val settings by repository.settings.collectAsState()
    val lastInvite by repository.lastChannelInvite.collectAsState()
    LaunchedEffect(Unit) {
        repository.uiEvents.collect { event ->
            when (event) {
                is org.openmesh.android.data.repository.UiEvent.Error -> error = event.text
                is org.openmesh.android.data.repository.UiEvent.Snackbar -> error = event.text
                else -> Unit
            }
        }
    }
    val presets = remember { runCatching { repository.presets() }.getOrNull() }
    val recommended = presets?.roleFor(role)

    Column(
        Modifier.fillMaxSize().statusBarsPadding().navigationBarsPadding().verticalScroll(rememberScrollState()).padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Filled.Shield, contentDescription = null, tint = MeshPrimary, modifier = Modifier.size(28.dp))
            Spacer(Modifier.width(10.dp))
            Column {
                Text("Set up your node", style = MaterialTheme.typography.headlineSmall)
                Text("Three steps · about two minutes", color = MeshOnSurfaceVariant)
            }
        }
        LinearProgressIndicator(
            progress = { (step + 1) / 3f },
            modifier = Modifier.fillMaxWidth(),
        )

        when (step) {
            0 -> {
                Text("What is this device?", style = MaterialTheme.typography.titleLarge)
                Text(
                    "This chooses sensible radio settings. You can change everything later in Settings.",
                    color = MeshOnSurfaceVariant,
                )
                roles.forEach { option ->
                    Card(
                        onClick = { role = option.id },
                        modifier = Modifier.fillMaxWidth(),
                        colors = androidx.compose.material3.CardDefaults.cardColors(
                            containerColor = if (role == option.id) MeshPrimary.copy(alpha = 0.18f) else MeshSurfaceHigh,
                        ),
                    ) {
                        Row(Modifier.padding(14.dp), verticalAlignment = Alignment.CenterVertically) {
                            Icon(option.icon, contentDescription = null, tint = MeshPrimary)
                            Spacer(Modifier.width(12.dp))
                            Column(Modifier.weight(1f)) {
                                Text(option.title, fontWeight = FontWeight.SemiBold)
                                Text(option.blurb, style = MaterialTheme.typography.bodySmall, color = MeshOnSurfaceVariant)
                            }
                            if (role == option.id) {
                                Icon(Icons.Filled.CheckCircle, contentDescription = "Selected", tint = MeshTertiary)
                            }
                        }
                    }
                }
                Button(onClick = { step = 1 }, modifier = Modifier.fillMaxWidth()) { Text("Continue") }
            }

            1 -> {
                Text("Where are you?", style = MaterialTheme.typography.titleLarge)
                Text(
                    "Your country decides the legal radio band. It is only stored on this device.",
                    color = MeshOnSurfaceVariant,
                )
                Text("Country code", style = MaterialTheme.typography.labelMedium)
                OutlinedTextField(
                    value = country,
                    onValueChange = { country = it.uppercase().take(2) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Text(
                    "Region: ${presets?.regionFor(country) ?: "EU868"}",
                    color = MeshPrimary,
                )
                Text("Node name", style = MaterialTheme.typography.labelMedium)
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it.take(32) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text("Keep my location private", fontWeight = FontWeight.SemiBold)
                        Text(
                            "Position sharing stays off until you explicitly send it.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MeshOnSurfaceVariant,
                        )
                    }
                    Switch(checked = keepPrivate, onCheckedChange = { keepPrivate = it })
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedButton(onClick = { step = 0 }) { Text("Back") }
                    Button(onClick = { step = 2 }, modifier = Modifier.weight(1f)) { Text("Continue") }
                }
            }

            else -> if (settings.setupCompleted) {
                Card(Modifier.fillMaxWidth()) {
                    Column(
                        Modifier.padding(16.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        Icon(Icons.Filled.CheckCircle, contentDescription = null, tint = MeshTertiary, modifier = Modifier.size(40.dp))
                        Text("You're set up", style = MaterialTheme.typography.titleLarge)
                        Text(
                            "${settings.region} · ${settings.profile} · setup_completed=true",
                            color = MeshOnSurfaceVariant,
                        )
                        if (lastInvite != null) {
                            Text("Invite someone to your private channel:", textAlign = TextAlign.Center)
                            InviteQr(content = lastInvite!!, size = 220.dp)
                            Text(
                                "Share this QR or the invite over a trusted channel. Anyone with it can join the channel.",
                                style = MaterialTheme.typography.bodySmall,
                                color = MeshOnSurfaceVariant,
                                textAlign = TextAlign.Center,
                            )
                        } else {
                            Text(
                                "The node is ready, but the private channel could not be created yet.",
                                color = MaterialTheme.colorScheme.error,
                                textAlign = TextAlign.Center,
                            )
                            OutlinedButton(
                                enabled = !busy,
                                onClick = {
                                    busy = true
                                    error = null
                                    scope.launch {
                                        repository.retryDefaultChannel()
                                            .onFailure { error = it.message ?: "Channel creation failed" }
                                        busy = false
                                    }
                                },
                            ) { Text(if (busy) "Creating…" else "Retry channel invite") }
                        }
                        error?.let { Text(it, color = MaterialTheme.colorScheme.error, textAlign = TextAlign.Center) }
                    }
                }
                Button(onClick = onFinished, modifier = Modifier.fillMaxWidth()) { Text("Start messaging") }
            } else {
                Text("Secure my node", style = MaterialTheme.typography.titleLarge)
                Text(
                    "This applies the shared beginner preset: region, profile, privacy and a private channel.",
                    color = MeshOnSurfaceVariant,
                )
                Card(Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text("Will be applied", fontWeight = FontWeight.SemiBold)
                        Text("Role: $role", color = MeshOnSurfaceVariant)
                        Text("Region: ${presets?.regionFor(country) ?: "EU868"} · profile ${recommended?.profile ?: "MEDIUM_FAST"}", color = MeshOnSurfaceVariant)
                        Text(
                            "Store & forward: ${if (recommended?.storeForward == true) "on" else "off"} · " +
                                "announce ${(recommended?.announceIntervalS ?: 1800) / 60} min",
                            color = MeshOnSurfaceVariant,
                        )
                        Text(
                            "Security: token-auth backbone, loopback API, no open backbone mode.",
                            color = MeshOnSurfaceVariant,
                        )
                    }
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedButton(onClick = { step = 1 }, enabled = !busy) { Text("Back") }
                    Button(
                        enabled = !busy,
                        modifier = Modifier.weight(1f),
                        onClick = {
                            busy = true
                            error = null
                            scope.launch {
                                repository.applyBeginnerPreset(
                                    BeginnerAnswers(
                                        role = role,
                                        country = country,
                                        nodeName = name.ifBlank { "openmesh-android" },
                                        privacy = if (keepPrivate) "private" else "shared",
                                    ),
                                ).onFailure {
                                    error = it.message ?: "Setup failed"
                                }
                                busy = false
                            }
                        },
                    ) { Text(if (busy) "Securing…" else "Secure my node") }
                }
            }
        }
        Text(
            "OpenMesh has no account server. Your identity lives on this device — back it up later in Settings.",
            style = MaterialTheme.typography.bodySmall,
            color = MeshOnSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier.fillMaxWidth().background(Color.Transparent),
        )
    }
}
