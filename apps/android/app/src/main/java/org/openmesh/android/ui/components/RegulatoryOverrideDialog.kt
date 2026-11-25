package org.openmesh.android.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.openmesh.android.data.regulatory.OverrideDraft
import org.openmesh.android.data.regulatory.RegionLimits
import org.openmesh.android.data.regulatory.RegulatoryOverride
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshSurfaceHigh

/**
 * Advanced-only modal for the regulatory override (docs/design/regulatory-and-regions.md §3).
 * Enabling any value above the legal limit requires ticking "I understand" AND typing OVERRIDE;
 * safe reductions (lower duty/power, region frequency) can be saved without the typed phrase.
 */
@Composable
fun RegulatoryOverrideDialog(
    repository: MeshRepository,
    region: String,
    onDismiss: () -> Unit,
) {
    val limit = RegionLimits.forRegion(region)
    val scope = rememberCoroutineScope()
    var dutyText by remember { mutableStateOf(limit.dutyPct?.toString() ?: "") }
    var powerText by remember { mutableStateOf(minOf(limit.maxPowerDbm, limit.hardwareMaxDbm).toString()) }
    var frequencyText by remember { mutableStateOf(limit.centerKhz.toString()) }
    var reason by remember { mutableStateOf("") }
    var understand by remember { mutableStateOf(false) }
    var typed by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }

    val draft = OverrideDraft(dutyText, powerText, frequencyText, reason, understand, typed)
    val preview = draft.toOverride(region)
    val exceeds = preview.exceedsLegal(region)

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Regulatory override (emergency use)") },
        text = {
            Column(
                Modifier.verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.errorContainer)) {
                    Text(
                        RegulatoryOverride.WARNING,
                        modifier = Modifier.padding(12.dp),
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                Text(limit.legalLine, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
                Text(
                    "Values are EIRP ceilings; a high-gain antenna can exceed them. Verify your local rules — this is not legal advice.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MeshOnSurfaceVariant,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(
                        value = dutyText,
                        onValueChange = { dutyText = it.filter(Char::isDigit).take(3) },
                        label = { Text("Duty cycle %") },
                        supportingText = { Text("0 = silent; legal ${limit.dutyLabel}") },
                        modifier = Modifier.weight(1f),
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = powerText,
                        onValueChange = { powerText = it.filter { c -> c.isDigit() || c == '-' }.take(4) },
                        label = { Text("TX power dBm") },
                        supportingText = { Text("Hardware cap ${limit.hardwareMaxDbm} dBm") },
                        modifier = Modifier.weight(1f),
                        singleLine = true,
                    )
                }
                OutlinedTextField(
                    value = frequencyText,
                    onValueChange = { frequencyText = it.filter(Char::isDigit).take(6) },
                    label = { Text("Frequency kHz (optional)") },
                    supportingText = { Text("Region centre ${limit.centerKhz} kHz · 150 000–960 000 kHz · antenna warning applies") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                OutlinedTextField(
                    value = reason,
                    onValueChange = { reason = it.take(120) },
                    label = { Text("Reason (optional, ≤120 chars)") },
                    supportingText = { Text("Recorded in the activity log; never put secrets here.") },
                    modifier = Modifier.fillMaxWidth(),
                )
                if (exceeds) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(checked = understand, onCheckedChange = { understand = it })
                        Text(
                            "I understand this may violate local law and void certification.",
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                    OutlinedTextField(
                        value = typed,
                        onValueChange = { typed = it.take(12) },
                        label = { Text("Type OVERRIDE to confirm") },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                    )
                } else {
                    Text(
                        "This is a safe reduction (duty/power at or below legal, region frequency); no typed confirmation required.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MeshOnSurfaceVariant,
                    )
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        },
        confirmButton = {
            Button(
                enabled = !busy,
                onClick = {
                    val validation = draft.validate(region, limit.hardwareMaxDbm)
                    if (!validation.ok) {
                        error = validation.error
                        return@Button
                    }
                    busy = true
                    error = null
                    scope.launch {
                        repository.setRegulatoryOverride(validation.override)
                            .onSuccess { onDismiss() }
                            .onFailure { error = it.message ?: "The override was rejected." }
                        busy = false
                    }
                },
            ) { Text(if (busy) "Saving…" else "Enable override") }
        },
        dismissButton = { TextButton(onClick = onDismiss, enabled = !busy) { Text("Cancel") } },
    )
}
