package org.openmesh.android.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import org.openmesh.android.data.model.NodeInfo
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.ui.components.MeshCard
import org.openmesh.android.ui.components.NodeIdText
import org.openmesh.android.ui.components.relativeTime
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshPrimary
import org.openmesh.android.ui.theme.MeshSurfaceHigh
import org.osmdroid.config.Configuration
import org.osmdroid.tileprovider.tilesource.TileSourceFactory
import org.osmdroid.util.GeoPoint
import org.osmdroid.views.MapView
import org.osmdroid.views.overlay.Marker

/**
 * Map tab: osmdroid with one marker per node that has announced a position.
 *
 * osmdroid is used instead of MapLibre native because it needs no API key and works with
 * the standard OSM tile servers; the README documents the MapLibre alternative and the
 * offline-tile considerations. The mesh itself needs no internet — only map tiles do.
 */
@Composable
fun MapScreen(repository: MeshRepository) {
    val nodes by repository.nodes.collectAsState()
    val withPosition = nodes.filter { it.position != null }
    var selected by remember { mutableStateOf<NodeInfo?>(null) }
    val context = LocalContext.current

    val mapView = remember {
        // OSM tile usage policy requires a meaningful user agent.
        Configuration.getInstance().userAgentValue = context.packageName
        MapView(context).apply {
            setTileSource(TileSourceFactory.MAPNIK)
            setMultiTouchControls(true)
            controller.setZoom(11.0)
            controller.setCenter(GeoPoint(59.9137, 10.7522))
        }
    }
    val markers = remember { mutableListOf<Marker>() }

    DisposableEffect(Unit) {
        mapView.onResume()
        onDispose { mapView.onPause() }
    }

    Box(Modifier.fillMaxSize()) {
        AndroidView(
            factory = { mapView },
            modifier = Modifier.fillMaxSize(),
            update = { view ->
                markers.forEach { view.overlays.remove(it) }
                markers.clear()
                withPosition.forEach { node ->
                    val position = node.position ?: return@forEach
                    val marker = Marker(view).apply {
                        this.position = GeoPoint(position.lat, position.lon, position.altM?.toDouble() ?: 0.0)
                        title = node.name
                        snippet = "${node.nodeClass.label} · ${node.hops} hop(s)"
                        setAnchor(Marker.ANCHOR_CENTER, Marker.ANCHOR_BOTTOM)
                        setOnMarkerClickListener { _, _ ->
                            selected = node
                            true
                        }
                    }
                    view.overlays.add(marker)
                    markers.add(marker)
                }
                if (withPosition.isNotEmpty() && markers.isNotEmpty()) {
                    selected?.position?.let {
                        view.controller.setCenter(GeoPoint(it.lat, it.lon))
                    }
                }
                view.invalidate()
            },
        )

        if (withPosition.isEmpty()) {
            MeshCard(
                Modifier
                    .align(Alignment.TopCenter)
                    .padding(16.dp),
            ) {
                Column(Modifier.padding(14.dp)) {
                    Text("No positions yet", style = MaterialTheme.typography.titleMedium)
                    Text(
                        "Position sharing is opt-in per node. When nodes announce a fix they appear here; " +
                            "the map is the only feature that uses internet (for map tiles).",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MeshOnSurfaceVariant,
                    )
                }
            }
        }

        Text(
            "© OpenStreetMap contributors",
            style = MaterialTheme.typography.labelSmall,
            color = MeshOnSurfaceVariant,
            modifier = Modifier
                .align(Alignment.BottomEnd)
                .padding(8.dp)
                .clip(RoundedCornerShape(4.dp))
                .background(MeshSurfaceHigh.copy(alpha = 0.8f))
                .padding(horizontal = 6.dp, vertical = 2.dp),
        )

        selected?.let { node ->
            MeshCard(
                Modifier
                    .align(Alignment.BottomCenter)
                    .padding(12.dp),
            ) {
                Row(Modifier.padding(14.dp), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(node.name, style = MaterialTheme.typography.titleMedium)
                        Text(
                            "${node.nodeClass.label} · ${node.hops} hop(s) · seen ${relativeTime(node.lastSeenMs)}",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MeshOnSurfaceVariant,
                        )
                        NodeIdText(node.nodeId)
                    }
                    Spacer(Modifier.width(8.dp))
                    Column(horizontalAlignment = Alignment.End) {
                        node.rssi?.let {
                            Text("$it dBm", style = MaterialTheme.typography.labelMedium, color = MeshPrimary)
                        }
                        node.batteryPct?.let {
                            Text("$it %", style = MaterialTheme.typography.labelMedium, color = MeshPrimary)
                        }
                    }
                }
            }
        }
    }
}
