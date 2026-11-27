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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.BatteryStd
import androidx.compose.material.icons.filled.CellTower
import androidx.compose.material.icons.filled.DeviceHub
import androidx.compose.material.icons.filled.GpsFixed
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.SolarPower
import androidx.compose.material.icons.filled.Storage
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
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
import org.openmesh.android.data.model.NodeInfo
import org.openmesh.android.data.model.NodeClass
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.ui.components.MeshCard
import org.openmesh.android.ui.components.NodeIdText
import org.openmesh.android.ui.components.SectionTitle
import org.openmesh.android.ui.components.StatusPill
import org.openmesh.android.ui.components.relativeTime
import org.openmesh.android.ui.radio.RadioPresentation
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshPrimary
import org.openmesh.android.ui.theme.MeshSecondary
import org.openmesh.android.ui.theme.MeshSurfaceHigh
import org.openmesh.android.ui.theme.MeshTertiary

/** Nodes tab: every node heard, with role, caps, hop count, signal and battery. */
@Composable
fun NodesScreen(repository: MeshRepository) {
    val nodes by repository.nodes.collectAsState()
    val radio by repository.radio.collectAsState()
    val settings by repository.settings.collectAsState()
    var query by remember { mutableStateOf("") }
    var selected by remember { mutableStateOf<NodeInfo?>(null) }

    val filtered = nodes
        .filter { query.isBlank() || it.name.contains(query, ignoreCase = true) || it.nodeId.contains(query, ignoreCase = true) }
        .sortedWith(compareBy({ it.hops }, { it.name.lowercase() }))

    Column(Modifier.fillMaxSize()) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp)) {
            Text("Nodes", style = MaterialTheme.typography.headlineSmall)
            Text(
                "${nodes.size} known · ${nodes.count { it.isContact }} contacts · " +
                    RadioPresentation.nodesSubtitle(radio, settings.demoMode),
                style = MaterialTheme.typography.bodyMedium,
                color = MeshOnSurfaceVariant,
            )
        }
        LazyColumn(
            Modifier.fillMaxSize(),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            item {
                OutlinedTextField(
                    value = query,
                    onValueChange = { query = it },
                    leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null) },
                    placeholder = { Text("Filter by name or id") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            item { SectionTitle("Mesh view") }
            item {
                MeshCard {
                    Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            StatusPill("${nodes.size} nodes", MeshPrimary)
                            StatusPill("${nodes.maxOfOrNull { it.hops } ?: 0} max hops", MeshSecondary)
                            StatusPill("${nodes.count { it.caps.contains("STORE") }} store", MeshTertiary)
                        }
                        Text(
                            "Managed flooding: every relay retransmits once, subject to TTL and duplicate " +
                                "suppression. Store-capable nodes hold encrypted envelopes for offline peers.",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MeshOnSurfaceVariant,
                        )
                    }
                }
            }
            item { SectionTitle("Known nodes") }
            items(filtered, key = { it.nodeId }) { node ->
                NodeRow(node, onClick = { selected = node })
            }
        }
    }

    selected?.let { node ->
        AlertDialog(
            onDismissRequest = { selected = null },
            title = { Text(node.name) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    NodeIdText(node.nodeId)
                    Text("Role: ${node.nodeClass.label}")
                    Text("Caps: ${node.caps.ifEmpty { setOf("none") }.joinToString(", ")}")
                    Text("Hops: ${node.hops} · last seen ${relativeTime(node.lastSeenMs)}")
                    node.rssi?.let { Text("RSSI $it dBm" + (node.snr?.let { snr -> " · SNR $snr dB" } ?: "")) }
                    node.batteryPct?.let { Text("Battery $it %") }
                    node.position?.let { Text("%.5f, %.5f".format(it.lat, it.lon)) }
                    if (node.isContact) Text("Saved contact", color = MeshPrimary)
                }
            },
            confirmButton = { TextButton(onClick = { selected = null }) { Text("Close") } },
        )
    }
}

@Composable
private fun NodeRow(node: NodeInfo, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(MeshSurfaceHigh.copy(alpha = 0.55f))
            .clickable(onClick = onClick)
            .padding(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .size(40.dp)
                .clip(CircleShape)
                .background(roleColor(node.nodeClass).copy(alpha = 0.16f)),
            contentAlignment = Alignment.Center,
        ) {
            Icon(roleIcon(node.nodeClass), contentDescription = null, tint = roleColor(node.nodeClass))
        }
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    node.name,
                    style = MaterialTheme.typography.titleMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                Text(
                    relativeTime(node.lastSeenMs),
                    style = MaterialTheme.typography.labelMedium,
                    color = MeshOnSurfaceVariant,
                )
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    "${node.nodeClass.label} · ${node.hops} hop(s)",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MeshOnSurfaceVariant,
                )
                Spacer(Modifier.width(8.dp))
                node.rssi?.let {
                    Icon(Icons.Filled.CellTower, contentDescription = null, modifier = Modifier.size(14.dp), tint = MeshOnSurfaceVariant)
                    Text("$it dBm", style = MaterialTheme.typography.labelMedium, color = MeshOnSurfaceVariant)
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                if (node.caps.contains("STORE")) CapDot("STORE", Icons.Filled.Storage)
                if (node.caps.contains("SOLAR")) CapDot("SOLAR", Icons.Filled.SolarPower)
                if (node.caps.contains("GPS")) CapDot("GPS", Icons.Filled.GpsFixed)
                node.batteryPct?.let { CapDot("$it %", Icons.Filled.BatteryStd) }
            }
        }
    }
}

@Composable
private fun CapDot(label: String, icon: androidx.compose.ui.graphics.vector.ImageVector) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, contentDescription = null, modifier = Modifier.size(12.dp), tint = MeshOnSurfaceVariant)
        Spacer(Modifier.width(2.dp))
        Text(label, style = MaterialTheme.typography.labelSmall, color = MeshOnSurfaceVariant)
    }
}

private fun roleIcon(role: NodeClass) = when (role) {
    NodeClass.REPEATER, NodeClass.GATEWAY -> Icons.Filled.DeviceHub
    NodeClass.TRACKER -> Icons.Filled.GpsFixed
    NodeClass.ROOM_SERVER -> Icons.Filled.Storage
    NodeClass.CLIENT -> Icons.Filled.CellTower
}

private fun roleColor(role: NodeClass) = when (role) {
    NodeClass.REPEATER -> MeshTertiary
    NodeClass.GATEWAY -> MeshSecondary
    NodeClass.TRACKER -> MeshPrimary
    NodeClass.ROOM_SERVER -> MeshTertiary
    NodeClass.CLIENT -> MeshPrimary
}
