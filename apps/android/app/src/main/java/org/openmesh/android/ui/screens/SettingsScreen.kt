package org.openmesh.android.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Bluetooth
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Link
import androidx.compose.material.icons.filled.Usb
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.openmesh.android.data.model.RadioStatus
import org.openmesh.android.data.repository.MediaKind
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.service.CompanionService
import org.openmesh.android.ui.components.DividerLine
import org.openmesh.android.ui.components.MeshCard
import org.openmesh.android.ui.components.NodeIdText
import org.openmesh.android.ui.components.SectionTitle
import org.openmesh.android.ui.components.StatusPill
import org.openmesh.android.ui.components.formatDuration
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshPrimary
import org.openmesh.android.ui.theme.MeshTertiary
import org.openmesh.android.util.LoraProfile
import org.openmesh.android.util.Region

/** Settings: identity, radio connection, media/airtime, mesh behaviour, about. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun SettingsScreen(
    repository: MeshRepository,
    radio: RadioStatus,
    onBack: () -> Unit,
) {
    val settings by repository.settings.collectAsState()
    val engine by repository.engineStatus.collectAsState()
    val context = LocalContext.current
    var nodeName by remember(settings.nodeName) { mutableStateOf(settings.nodeName) }

    val profile = LoraProfile.fromName(settings.profile)
    val capBytes = repository.mediaCapBytes()
    val imageEstimate = repository.estimateMedia(5 * 1024, MediaKind.IMAGE)
    val voiceEstimate = repository.estimateMedia(7_000, MediaKind.VOICE)

    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onBack) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
            }
            Text("Settings", style = MaterialTheme.typography.headlineSmall)
        }

        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            SectionTitle("Identity")
            MeshCard {
                Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(
                        value = nodeName,
                        onValueChange = {
                            nodeName = it.take(32)
                            repository.updateSettings { current -> current.copy(nodeName = nodeName) }
                        },
                        label = { Text("Node name (announce)") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    NodeIdText(engine.nodeIdHex ?: radio.nodeId ?: "identity not loaded")
                    Text(
                        "Identity: Ed25519 signing key + X25519 DH key derived from one 32-byte seed. " +
                            "The Android app will hold it in the Keystore through openmesh-ffi (M2); " +
                            "the radio never holds your user keys.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MeshOnSurfaceVariant,
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        StatusPill(if (engine.running) "core: demo" else "core: not linked", MeshTertiary)
                        StatusPill("protocol OMP/1", MeshPrimary)
                    }
                }
            }

            SectionTitle("Radio")
            MeshCard {
                Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text(
                                if (radio.connected) "Connected · ${radio.transportName}" else "No companion link",
                                style = MaterialTheme.typography.titleMedium,
                            )
                            Text(
                                radio.firmwareVersion?.let { "firmware $it" } ?: "Connect a radio over USB or BLE",
                                style = MaterialTheme.typography.bodyMedium,
                                color = MeshOnSurfaceVariant,
                            )
                        }
                        if (radio.connected) {
                            OutlinedButton(onClick = {
                                CompanionService.stop(context)
                            }) { Text("Disconnect") }
                        }
                    }
                    if (!radio.connected) {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Button(
                                onClick = {
                                    CompanionService.start(context, CompanionService.ACTION_CONNECT_USB)
                                },
                                modifier = Modifier.weight(1f),
                            ) {
                                Icon(Icons.Filled.Usb, contentDescription = null, modifier = Modifier.size(18.dp))
                                Spacer(Modifier.width(6.dp))
                                Text("USB")
                            }
                            Button(
                                onClick = {
                                    CompanionService.start(context, CompanionService.ACTION_CONNECT_BLE)
                                },
                                modifier = Modifier.weight(1f),
                            ) {
                                Icon(Icons.Filled.Bluetooth, contentDescription = null, modifier = Modifier.size(18.dp))
                                Spacer(Modifier.width(6.dp))
                                Text("BLE")
                            }
                        }
                    }

                    DividerLine()
                    Text("Profile", style = MaterialTheme.typography.labelMedium)
                    androidx.compose.foundation.layout.FlowRow(
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        LoraProfile.entries.forEach { entry ->
                            FilterChip(
                                selected = settings.profile == entry.name,
                                onClick = { repository.updateSettings { it.copy(profile = entry.name) } },
                                label = { Text(entry.label) },
                            )
                        }
                    }
                    Text("Region (duty cycle)", style = MaterialTheme.typography.labelMedium)
                    androidx.compose.foundation.layout.FlowRow(
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        Region.entries.forEach { entry ->
                            FilterChip(
                                selected = settings.region == entry.name,
                                onClick = { repository.updateSettings { it.copy(region = entry.name) } },
                                label = { Text(entry.name) },
                            )
                        }
                    }
                    SwitchRow(
                        title = "Share position",
                        subtitle = "Announce GPS fixes to chosen contacts/channels (opt-in).",
                        checked = settings.positionSharing,
                        onCheckedChange = { value -> repository.updateSettings { it.copy(positionSharing = value) } },
                    )
                    SwitchRow(
                        title = "Store and forward (relays)",
                        subtitle = "Store-capable repeaters may hold encrypted envelopes for offline peers for 72 h.",
                        checked = settings.storeForward,
                        onCheckedChange = { value -> repository.updateSettings { it.copy(storeForward = value) } },
                    )
                }
            }

            SectionTitle("Airtime and media")
            MeshCard {
                Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(
                        "${profile.label}: ${profile.maxFrameAirtimeMs} ms for a full 240-byte frame.",
                        style = MaterialTheme.typography.titleMedium,
                    )
                    Text(
                        "Attachment cap on this profile: ${capBytes / 1024.0} KiB. " +
                            "5 KiB image ≈ ${(" %.1f".format(imageEstimate.airtimeSeconds)).trim()} s airtime; " +
                            "8 s Opus voice ≈ ${"%.1f".format(voiceEstimate.airtimeSeconds)} s.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MeshOnSurfaceVariant,
                    )
                    if (settings.region.let { Region.fromName(it).dutyCyclePct } != null) {
                        Text(
                            "At ${Region.fromName(settings.region).label} this means ≈ " +
                                "${formatDuration(imageEstimate.wallClockMs)} of wall clock for the image " +
                                "(the dialog shows the exact number before every send).",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MeshTertiary,
                        )
                    }
                    SwitchRow(
                        title = "Show airtime estimates",
                        subtitle = "Required by the protocol for media on 1 % duty-cycle bands.",
                        checked = settings.showAirtimeEstimates,
                        onCheckedChange = { value -> repository.updateSettings { it.copy(showAirtimeEstimates = value) } },
                    )
                }
            }

            SectionTitle("Mesh")
            MeshCard {
                Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(
                        "Announce every ${settings.announceIntervalSec / 60} min · TTL 5 (clients) / 8 (announces)",
                        style = MaterialTheme.typography.bodyLarge,
                    )
                    SwitchRow(
                        title = "Demo mode",
                        subtitle = "Show seeded conversations and simulate delivery. Turn off when a radio " +
                            "and the linked core are available; without the FFI core, composing messages " +
                            "reports an explicit error instead of pretending to send.",
                        checked = settings.demoMode,
                        onCheckedChange = { value -> repository.setDemoMode(value) },
                    )
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Filled.Link, contentDescription = null, tint = MeshOnSurfaceVariant)
                        Spacer(Modifier.width(8.dp))
                        Column {
                            Text("Bridge API (desktop/web)", style = MaterialTheme.typography.bodyLarge)
                            Text(
                                "http://127.0.0.1:8787 · ws://127.0.0.1:8787/api/ws",
                                style = MaterialTheme.typography.labelSmall,
                                color = MeshOnSurfaceVariant,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                    engine.error?.let {
                        Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium)
                    }
                }
            }

            SectionTitle("About")
            MeshCard {
                Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Filled.Info, contentDescription = null, tint = MeshOnSurfaceVariant)
                        Spacer(Modifier.width(8.dp))
                        Text("OpenMesh Android 0.1.0", style = MaterialTheme.typography.titleMedium)
                    }
                    Text(
                        "AGPL-3.0-or-later. This is a scaffold: the companion transport, framing and " +
                            "repository are real; OMP frame construction/parsing arrives via openmesh-ffi in M2. " +
                            "OpenMesh is not Reticulum and contains no Reticulum-licensed code.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MeshOnSurfaceVariant,
                    )
                    Text(
                        "Source: https://github.com/martintondel02/openmesh",
                        style = MaterialTheme.typography.labelSmall,
                        color = MeshOnSurfaceVariant,
                    )
                }
            }
            Spacer(Modifier.height(24.dp))
        }
    }
}

@Composable
private fun SwitchRow(
    title: String,
    subtitle: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            Text(subtitle, style = MaterialTheme.typography.bodyMedium, color = MeshOnSurfaceVariant)
        }
        Switch(checked = checked, onCheckedChange = onCheckedChange)
    }
}
