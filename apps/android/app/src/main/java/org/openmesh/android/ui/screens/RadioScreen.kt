package org.openmesh.android.ui.screens

import android.Manifest
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Bluetooth
import androidx.compose.material.icons.filled.Usb
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
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
import androidx.compose.ui.unit.dp
import org.openmesh.android.data.model.AppSettings
import org.openmesh.android.data.model.RadioStatus
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.service.MeshService
import org.openmesh.android.ui.components.SectionTitle
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshPrimary
import org.openmesh.android.ui.theme.MeshSurfaceHigh

/** Radio link screen: connect BLE/USB companion, show signal, stats and settings summary. */
@Composable
fun RadioScreen(
    repository: MeshRepository,
    radio: RadioStatus,
    settings: AppSettings,
    onBack: () -> Unit,
) {
    val context = LocalContext.current
    val stats by repository.stats.collectAsState()
    val engine by repository.engineStatus.collectAsState()
    var pendingAction by remember { mutableStateOf<String?>(null) }
    var permissionNote by remember { mutableStateOf<String?>(null) }

    // Android 12+ needs runtime Bluetooth permissions before scanning; ask here, then connect.
    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) { grants ->
        val action = pendingAction
        pendingAction = null
        if (action == null) return@rememberLauncherForActivityResult
        val granted = grants[Manifest.permission.BLUETOOTH_SCAN] != false &&
            grants[Manifest.permission.BLUETOOTH_CONNECT] != false
        if (granted) {
            permissionNote = null
            MeshService.start(context, action)
        } else {
            permissionNote = "Bluetooth permission is required to scan for and connect the radio."
        }
    }
    fun connect(action: String) {
        if (action == MeshService.ACTION_CONNECT_BLE && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            pendingAction = action
            permissionLauncher.launch(
                arrayOf(Manifest.permission.BLUETOOTH_SCAN, Manifest.permission.BLUETOOTH_CONNECT),
            )
        } else {
            permissionNote = null
            MeshService.start(context, action)
        }
    }

    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back") }
            Column {
                Text("Radio", style = MaterialTheme.typography.headlineSmall)
                Text("The radio is a dumb authenticated pipe for OMP frames", color = MeshOnSurfaceVariant)
            }
        }
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            SectionTitle("Companion link")
            Card(colors = CardDefaults.cardColors(containerColor = MeshSurfaceHigh)) {
                Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(
                        if (radio.connected) "Connected · ${radio.transportName}" else "No radio connected",
                        style = MaterialTheme.typography.titleMedium,
                    )
                    Text(
                        radio.firmwareVersion?.let { "Firmware $it · node ${radio.nodeId?.take(8) ?: "?"}" }
                            ?: "Connect a supported board over USB serial or BLE. The app runs the protocol; the radio relays frames.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MeshOnSurfaceVariant,
                    )
                    Text("Profile ${settings.profile} · region ${settings.region}", style = MaterialTheme.typography.bodySmall, color = MeshPrimary)
                    permissionNote?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                    if (radio.connected) {
                        OutlinedButton(onClick = { MeshService.disconnect(context) }) { Text("Disconnect") }
                    } else {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Button(onClick = { connect(MeshService.ACTION_CONNECT_USB) }, modifier = Modifier.weight(1f)) {
                                Icon(Icons.Filled.Usb, contentDescription = null, modifier = Modifier.size(18.dp))
                                Spacer(Modifier.width(6.dp))
                                Text("USB")
                            }
                            Button(onClick = { connect(MeshService.ACTION_CONNECT_BLE) }, modifier = Modifier.weight(1f)) {
                                Icon(Icons.Filled.Bluetooth, contentDescription = null, modifier = Modifier.size(18.dp))
                                Spacer(Modifier.width(6.dp))
                                Text("BLE")
                            }
                        }
                    }
                }
            }

            SectionTitle("Signal and counters")
            Card(colors = CardDefaults.cardColors(containerColor = MeshSurfaceHigh)) {
                Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text("RSSI ${radio.rssi?.let { "$it dBm" } ?: "—"} · SNR ${radio.snr?.let { "$it dB" } ?: "—"}")
                    Text("Air queue ${radio.txQueue} · airtime ${"%.1f".format(radio.airtimePct)}% of budget")
                    Text("Frames tx ${stats.txFrames} · rx ${stats.rxFrames} · forwarded ${stats.forwarded} · dropped ${stats.dropped}")
                    Text("Known nodes ${stats.nodes} · channels ${stats.channels} · airtime ${stats.airtimeMs} ms")
                    Text(
                        "Effective duty: ${stats.effectiveDutyPct?.let { "$it %" } ?: "fair use"} (region/override policy)",
                        color = if (settings.overrideEnabled) MaterialTheme.colorScheme.error else MeshOnSurfaceVariant,
                    )
                    Text("Engine ${if (engine.running) "running" else "stopped"} · node ${engine.nodeIdHex ?: "—"}", color = MeshOnSurfaceVariant)
                    Text(repository.regulatoryStatusLine(), color = if (settings.overrideEnabled) MaterialTheme.colorScheme.error else MeshOnSurfaceVariant)
                    Text(repository.legalLine(), style = MaterialTheme.typography.bodySmall, color = MeshOnSurfaceVariant)
                }
            }

            SectionTitle("Tips")
            Text(
                "• Pairing: hold the board's pairing button / use the flasher's BLE name prefix \"OpenMesh\".\n" +
                    "• USB: use a data cable; accept the USB permission prompt.\n" +
                    "• Profiles must match across the mesh; changing profile disconnects nodes on the old one.\n" +
                    "• Battery: the foreground service keeps the link alive; BLE and USB are both low-power.",
                style = MaterialTheme.typography.bodyMedium,
                color = MeshOnSurfaceVariant,
            )
        }
    }
}
