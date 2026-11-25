package org.openmesh.android.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.AutoAwesome
import androidx.compose.material.icons.filled.Cloud
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import android.content.Intent
import org.openmesh.android.MainActivity
import org.openmesh.android.service.MeshService
import org.openmesh.android.BuildConfig
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.ui.components.SectionTitle
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshPrimary
import org.openmesh.android.ui.theme.MeshSurfaceHigh
import org.openmesh.android.ui.theme.MeshTertiary

/** About, agents and backbone tips (v0.5.0). */
@Composable
fun AboutScreen(
    repository: MeshRepository,
    onBack: () -> Unit,
    onOpenHelp: (String) -> Unit,
) {
    val context = LocalContext.current
    val selfId = repository.selfNodeId().orEmpty()
    var showQaReset by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back") }
            Column(
                Modifier.pointerInput(Unit) {
                    detectTapGestures(onLongPress = {
                        if (BuildConfig.DEBUG) showQaReset = true
                    })
                },
            ) {
                Text("About & agents", style = MaterialTheme.typography.headlineSmall)
                Text("Version ${BuildConfig.VERSION_NAME}", color = MeshOnSurfaceVariant)
            }
        }
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            SectionTitle("AI agents (MCP)")
            Card(colors = CardDefaults.cardColors(containerColor = MeshSurfaceHigh)) {
                Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Filled.AutoAwesome, contentDescription = null, tint = MeshPrimary)
                        Text("  MCP server on your computer", style = MaterialTheme.typography.titleMedium)
                    }
                    Text(
                        "The Android app runs the protocol itself, so to let an AI agent operate a bridge, run `openmesh-bridge mcp` on the machine that hosts your bridge/bridge-mode node and connect Claude, Cursor or DSH. The agent gets read tools plus guarded write tools (send, config, backbone) and every write is attributed in the activity log.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MeshOnSurfaceVariant,
                    )
                    Text(
                        "Safety: start read-only (OPENMESH_MCP_READ_ONLY=1), keep the bridge API on loopback, and use confirm:true only for destructive actions.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MeshOnSurfaceVariant,
                    )
                    Text(
                        "Android has no local MCP server in v0.5.0; agents never get your Android identity seed.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MeshTertiary,
                    )
                }
            }

            SectionTitle("Run a backbone")
            Card(colors = CardDefaults.cardColors(containerColor = MeshSurfaceHigh)) {
                Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Filled.Cloud, contentDescription = null, tint = MeshPrimary)
                        Text("  Link distant meshes", style = MaterialTheme.typography.titleMedium)
                    }
                    Text(
                        "A small VPS or Raspberry Pi can relay between meshes over TCP using a shared token. Keep the API loopback-only, open only the OBB port (default 9443/tcp), and prefer WireGuard or Tailscale on hostile networks.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MeshOnSurfaceVariant,
                    )
                    Text(
                        "Exposure note: if you publish a backbone publicly, treat it like any internet service — restrict the port, rotate the token, monitor auth failures. Preferred: Openflare (outbound-only tunnel, no open inbound port — github.com/martintondel02/openflare) or WireGuard/Tailscale; see docs/guides/backbone-exposure.md in the repo. Never expose the panel/API.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MeshOnSurfaceVariant,
                    )
                }
            }

            SectionTitle("Security")
            Card(colors = CardDefaults.cardColors(containerColor = MeshSurfaceHigh)) {
                Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Filled.Lock, contentDescription = null, tint = MeshPrimary)
                        Text("  Your identity", style = MaterialTheme.typography.titleMedium)
                    }
                    Text("Node id: ${selfId.ifBlank { "not started" }}", fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace)
                    Text(
                        "The 32-byte identity seed is generated on this device and wrapped with an AndroidKeyStore AES-GCM key. It never leaves the phone in plaintext; there is no account recovery, so export/backup it from Settings.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MeshOnSurfaceVariant,
                    )
                }
            }

            SectionTitle("Open source")
            Text(
                "OpenMesh v0.5.0 · AGPL-3.0-only. Includes osmdroid, zxing, usb-serial-for-android and Nordic BLE (see NOTICE in the repository). The Rust protocol core is linked through UniFFI; no analytics, no cloud, no account.",
                style = MaterialTheme.typography.bodySmall,
                color = MeshOnSurfaceVariant,
            )
            Text("Help topics: onboarding, channels, media, agents, backbone…", style = MaterialTheme.typography.bodySmall, color = MeshOnSurfaceVariant)
        }
    }

    if (showQaReset) {
        AlertDialog(
            onDismissRequest = { showQaReset = false },
            title = { Text("QA: reset onboarding?") },
            text = {
                Text(
                    "Debug builds only. Clears the setup_completed flag (keeps identity, messages and settings) " +
                        "and restarts the app so the onboarding wizard can be re-tested without uninstalling.",
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    showQaReset = false
                    repository.resetOnboarding()
                    MeshService.stop(context)
                    context.startActivity(
                        Intent(context, MainActivity::class.java)
                            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK),
                    )
                    (context as? android.app.Activity)?.finish()
                }) { Text("Reset & restart") }
            },
            dismissButton = { TextButton(onClick = { showQaReset = false }) { Text("Cancel") } },
        )
    }
}
