package org.openmesh.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Cancel
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.DoneAll
import androidx.compose.material.icons.filled.Error
import androidx.compose.material.icons.filled.Inventory2
import androidx.compose.material.icons.filled.MarkEmailRead
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import org.openmesh.android.data.model.DeliveryState
import org.openmesh.android.data.model.RadioStatus
import org.openmesh.android.data.repository.MediaEstimate
import org.openmesh.android.ui.radio.RadioPresentation
import org.openmesh.android.ui.radio.RadioTone
import org.openmesh.android.ui.theme.MeshError
import org.openmesh.android.ui.theme.MeshOnline
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshOutline
import org.openmesh.android.ui.theme.MeshPrimary
import org.openmesh.android.ui.theme.MeshSecondary
import org.openmesh.android.ui.theme.MeshSurface
import org.openmesh.android.ui.theme.MeshSurfaceHigh
import org.openmesh.android.ui.theme.MeshTertiary

/** Relative timestamp used in chat lists ("now", "4m", "2h", "3d"). */
fun relativeTime(ms: Long): String {
    if (ms <= 0L) return ""
    val diff = (System.currentTimeMillis() - ms).coerceAtLeast(0L)
    return when {
        diff < 60_000 -> "now"
        diff < 3_600_000 -> "${diff / 60_000}m"
        diff < 86_400_000 -> "${diff / 3_600_000}h"
        diff < 7 * 86_400_000L -> "${diff / 86_400_000}d"
        else -> SimpleDateFormat("dd MMM", Locale.getDefault()).format(Date(ms))
    }
}

@Composable
fun StatusDot(online: Boolean, modifier: Modifier = Modifier) {
    Box(
        modifier
            .size(8.dp)
            .clip(CircleShape)
            .background(if (online) MeshOnline else MeshOnSurfaceVariant),
    )
}

@Composable
fun StatusPill(text: String, color: Color = MeshPrimary, modifier: Modifier = Modifier) {
    Row(
        modifier = modifier
            .clip(RoundedCornerShape(50))
            .background(color.copy(alpha = 0.12f))
            .padding(horizontal = 10.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Box(
            Modifier
                .size(6.dp)
                .clip(CircleShape)
                .background(color),
        )
        Text(
            text = text.uppercase(Locale.ROOT),
            style = MaterialTheme.typography.labelSmall,
            color = color,
            maxLines = 1,
        )
    }
}

/** Theme color for a radio status tone (dot, spinner, error accents). */
fun radioToneColor(tone: RadioTone): Color = when (tone) {
    RadioTone.NEUTRAL -> MeshOnSurfaceVariant
    RadioTone.PROGRESS -> MeshSecondary
    RadioTone.POSITIVE -> MeshOnline
    RadioTone.WARNING -> MeshTertiary
    RadioTone.ERROR -> MeshError
}

/**
 * One-line radio status used at the top of the chat list and settings.
 *
 * Phase-aware: says what the link is doing (scanning/connecting/handshake/ready/error) instead of
 * only "connected / no radio", and shows how many messages are waiting locally.
 */
@Composable
fun RadioBanner(radio: RadioStatus, demoMode: Boolean, onClick: () -> Unit) {
    val copy = RadioPresentation.bannerCopy(radio, demoMode)
    val tone = radioToneColor(copy.tone)
    val running = !demoMode && RadioPresentation.isRunning(radio.phase)
    Card(
        onClick = onClick,
        colors = CardDefaults.cardColors(containerColor = MeshSurface),
        shape = RoundedCornerShape(14.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(
            Modifier.padding(horizontal = 14.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.size(8.dp).clip(CircleShape).background(tone))
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    copy.title,
                    style = MaterialTheme.typography.titleMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    copy.subtitle,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MeshOnSurfaceVariant,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            if (radio.queuedMessages > 0) {
                StatusPill(RadioPresentation.queuedChipLabel(radio.queuedMessages), MeshTertiary)
            } else if (running) {
                Spacer(Modifier.width(8.dp))
                CircularProgressIndicator(
                    modifier = Modifier.size(18.dp),
                    color = tone,
                    strokeWidth = 2.dp,
                )
            }
        }
    }
}

@Composable
fun SectionTitle(text: String, modifier: Modifier = Modifier) {
    Text(
        text = text.uppercase(Locale.ROOT),
        style = MaterialTheme.typography.labelMedium,
        color = MeshOnSurfaceVariant,
        modifier = modifier.padding(start = 4.dp, bottom = 8.dp),
    )
}

@Composable
fun MeshCard(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MeshSurface),
        shape = RoundedCornerShape(16.dp),
        modifier = modifier.fillMaxWidth(),
    ) { content() }
}

@Composable
fun EmptyState(title: String, detail: String, modifier: Modifier = Modifier) {
    Column(
        modifier = modifier.fillMaxWidth().padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(title, style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(6.dp))
        Text(detail, style = MaterialTheme.typography.bodyMedium, color = MeshOnSurfaceVariant)
    }
}

@Composable
fun DeliveryIcon(state: DeliveryState) {
    when (state) {
        DeliveryState.QUEUED -> Icon(Icons.Filled.Schedule, "queued", tint = MeshOnSurfaceVariant, modifier = Modifier.size(14.dp))
        DeliveryState.SENT -> Icon(Icons.Filled.Check, "sent", tint = MeshOnSurfaceVariant, modifier = Modifier.size(14.dp))
        DeliveryState.DELIVERED -> Icon(Icons.Filled.DoneAll, "delivered", tint = MeshPrimary, modifier = Modifier.size(14.dp))
        DeliveryState.READ -> Icon(Icons.Filled.MarkEmailRead, "read", tint = MeshPrimary, modifier = Modifier.size(14.dp))
        DeliveryState.STORED_AT_RELAY -> Icon(Icons.Filled.Inventory2, "stored at relay", tint = MeshTertiary, modifier = Modifier.size(14.dp))
        DeliveryState.FAILED -> Icon(Icons.Filled.Error, "failed", tint = MeshError, modifier = Modifier.size(14.dp))
        DeliveryState.CANCELLED -> Icon(Icons.Filled.Cancel, "cancelled", tint = MeshOnSurfaceVariant, modifier = Modifier.size(14.dp))
    }
}

/**
 * Pre-send airtime confirmation. This dialog is the product contract from
 * docs/research/lora-capacity-and-media.md §7.2: never let a user send media without seeing
 * what it costs on the current profile/region.
 */
@Composable
fun AirtimeEstimateDialog(
    estimate: MediaEstimate,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
) {
    val pct = (estimate.fractionOfHourlyBudget * 100)
    val overBudget = pct > 100f
    val heavy = pct > 25f
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Send ${estimate.kind.name.lowercase()}?") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("${(estimate.bytes / 1024.0).toString().take(4)} KiB · ${estimate.chunks} chunks")
                Text("≈ ${"%.1f".format(estimate.airtimeSeconds)} s of airtime on ${estimate.profile.label}")
                Text(
                    "At ${estimate.region.label} (${estimate.region.dutyCyclePct?.let { "$it %" } ?: "fair use"}) " +
                        "this takes ≈ ${formatDuration(estimate.wallClockMs)} to send.",
                )
                if (estimate.region.dutyCyclePct != null) {
                    Text(
                        "Uses ≈ ${pct.toInt()} % of one hour's airtime allowance.",
                        color = if (overBudget) MeshError else if (heavy) MeshTertiary else MeshPrimary,
                        fontWeight = FontWeight.Medium,
                    )
                }
                if (overBudget) {
                    Text(
                        "This exceeds a 1-hour allowance: the transfer will be spread over hours on a " +
                            "1 % duty-cycle band. Consider a smaller thumbnail.",
                        color = MeshError,
                    )
                }
                Text(
                    "Relays in range also transmit this, spending their own airtime.",
                    color = MeshOnSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        },
        confirmButton = {
            TextButton(onClick = onConfirm) {
                Text(if (overBudget) "Send anyway" else "Send")
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

fun formatDuration(ms: Long): String {
    val totalSeconds = (ms / 1000).coerceAtLeast(0)
    return when {
        totalSeconds < 60 -> "${totalSeconds}s"
        totalSeconds < 3600 -> "${totalSeconds / 60}m ${totalSeconds % 60}s"
        else -> "${totalSeconds / 3600}h ${(totalSeconds % 3600) / 60}m"
    }
}

@Composable
fun NodeIdText(nodeId: String, modifier: Modifier = Modifier) {
    Text(
        text = nodeId,
        style = MaterialTheme.typography.labelSmall.copy(fontFamily = FontFamily.Monospace),
        color = MeshOnSurfaceVariant,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = modifier,
    )
}

@Composable
fun DividerLine() {
    Box(
        Modifier
            .fillMaxWidth()
            .height(1.dp)
            .background(MeshOutline.copy(alpha = 0.6f)),
    )
}

@Suppress("unused")
private val unusedSurface = MeshSurfaceHigh
