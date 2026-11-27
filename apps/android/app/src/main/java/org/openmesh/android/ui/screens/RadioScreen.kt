package org.openmesh.android.ui.screens

import android.Manifest
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Bluetooth
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Error
import androidx.compose.material.icons.filled.Usb
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import org.openmesh.android.data.model.AppSettings
import org.openmesh.android.data.model.RadioStatus
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.service.MeshService
import org.openmesh.android.ui.components.SectionTitle
import org.openmesh.android.ui.components.radioToneColor
import org.openmesh.android.ui.radio.RadioPresentation
import org.openmesh.android.ui.radio.RadioStep
import org.openmesh.android.ui.radio.RetryTransport
import org.openmesh.android.ui.radio.StepState
import org.openmesh.android.ui.theme.MeshError
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshOutline
import org.openmesh.android.ui.theme.MeshSecondary
import org.openmesh.android.ui.theme.MeshSurfaceHigh
import org.openmesh.android.ui.theme.MeshOnline

/**
 * Radio link screen: state-driven companion card (phase, step rail, persistent error), signal,
 * counters and settings summary.
 *
 * The top card is deliberately plain language (docs/design/ux-help-system.md §6); protocol terms
 * appear only in the counters/tips below.
 */
@Composable
fun RadioScreen(
    repository: MeshRepository,
    radio: RadioStatus,
    settings: AppSettings,
    onBack: () -> Unit,
    onOpenHelp: (String) -> Unit = {},
) {
    val context = LocalContext.current
    val stats by repository.stats.collectAsState()
    val engine by repository.engineStatus.collectAsState()
    var pendingAction by remember { mutableStateOf<String?>(null) }
    var permissionNote by remember { mutableStateOf<String?>(null) }
    val demoMode = settings.demoMode

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

    fun retryConnect() {
        connect(
            when (RadioPresentation.retryTransport(radio.transportName)) {
                RetryTransport.USB -> MeshService.ACTION_CONNECT_USB
                RetryTransport.BLE -> MeshService.ACTION_CONNECT_BLE
            },
        )
    }

    // Elapsed-seconds ticker while an attempt is in progress (text, not just the spinner).
    var nowMs by remember { mutableLongStateOf(System.currentTimeMillis()) }
    val running = !demoMode && RadioPresentation.isRunning(radio.phase)
    LaunchedEffect(running) {
        while (running) {
            nowMs = System.currentTimeMillis()
            delay(1_000)
        }
    }

    val copy = RadioPresentation.phaseCopy(radio, demoMode)
    val toneColor = radioToneColor(copy.tone)
    val errorCard = RadioPresentation.errorCard(radio)

    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back") }
            Column {
                Text("Radio", style = MaterialTheme.typography.headlineSmall)
                Text(
                    "The radio carries your messages over the air. This screen shows what it is doing.",
                    color = MeshOnSurfaceVariant,
                )
            }
        }
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            SectionTitle("Radio link")

            // State card: phase title + text, step rail, device details, actions. The card itself
            // carries an accessibility state description; the spinner is never the only indicator.
            Card(
                colors = CardDefaults.cardColors(containerColor = MeshSurfaceHigh),
                modifier = Modifier
                    .fillMaxWidth()
                    .semantics {
                        contentDescription = copy.stateDescription
                        stateDescription = copy.stateDescription
                    },
            ) {
                Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        if (copy.showSpinner) {
                            CircularProgressIndicator(
                                modifier = Modifier.size(22.dp),
                                color = toneColor,
                                strokeWidth = 2.dp,
                            )
                            Spacer(Modifier.width(10.dp))
                        } else {
                            Box(Modifier.size(10.dp).clip(CircleShape).background(toneColor))
                            Spacer(Modifier.width(10.dp))
                        }
                        Column(Modifier.weight(1f)) {
                            Text(copy.title, style = MaterialTheme.typography.titleMedium)
                            Text(
                                copy.subtitle,
                                style = MaterialTheme.typography.bodyMedium,
                                color = if (copy.tone == org.openmesh.android.ui.radio.RadioTone.ERROR) {
                                    MeshError
                                } else {
                                    MeshOnSurfaceVariant
                                },
                            )
                        }
                        if (running) {
                            val seconds = RadioPresentation.elapsedSeconds(radio, nowMs)
                            Text("${seconds}s", style = MaterialTheme.typography.labelMedium, color = toneColor)
                        }
                    }

                    val rail = RadioPresentation.stepRail(radio, demoMode)
                    if (rail.isNotEmpty()) {
                        StepRail(rail)
                    }

                    if (!demoMode && radio.phase != org.openmesh.android.data.transport.RadioPhase.CONNECTING &&
                        (radio.deviceName != null || radio.deviceAddress != null || radio.rssi != null)
                    ) {
                        val details = buildList {
                            radio.deviceName?.let { add(it) }
                            radio.deviceAddress?.let { add(it) }
                            radio.rssi?.let { add("signal $it dBm") }
                            if (radio.devicesFound > 1) add("${radio.devicesFound} radios found")
                        }
                        Text(
                            details.joinToString(" · "),
                            style = MaterialTheme.typography.bodySmall,
                            color = MeshOnSurfaceVariant,
                        )
                    }

                    permissionNote?.let { Text(it, color = MaterialTheme.colorScheme.error) }

                    when {
                        demoMode -> Text(
                            "Demo mode is on, so no radio is used. Turn it off in Settings → Experience to connect a real radio.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MeshOnSurfaceVariant,
                        )
                        running -> {
                            OutlinedButton(onClick = { MeshService.disconnect(context) }, modifier = Modifier.fillMaxWidth()) {
                                Text("Cancel")
                            }
                        }
                        radio.phase == org.openmesh.android.data.transport.RadioPhase.READY -> {
                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                if (radio.health == org.openmesh.android.data.transport.RadioHealth.STALE) {
                                    Button(onClick = { retryConnect() }, modifier = Modifier.weight(1f)) {
                                        Text("Reconnect")
                                    }
                                }
                                OutlinedButton(
                                    onClick = { MeshService.disconnect(context) },
                                    modifier = Modifier.weight(1f),
                                ) { Text("Disconnect") }
                            }
                        }
                        else -> {
                            // Idle or error: offer both transports plus a direct retry on the last one.
                            if (errorCard != null) {
                                Button(onClick = { retryConnect() }, modifier = Modifier.fillMaxWidth()) {
                                    Text("Try again")
                                }
                            }
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
            }

            // Persistent error card: stays until the next connect attempt starts, with hints and help.
            errorCard?.let { error ->
                Card(
                    colors = CardDefaults.cardColors(containerColor = MeshError.copy(alpha = 0.12f)),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text(error.title, style = MaterialTheme.typography.titleMedium, color = MeshError)
                        Text(error.message, style = MaterialTheme.typography.bodyMedium)
                        Text(error.hint, style = MaterialTheme.typography.bodyMedium, color = MeshOnSurfaceVariant)
                        TextButton(onClick = { onOpenHelp("troubleshooting") }) { Text("Open troubleshooting help") }
                    }
                }
            }

            if (radio.phase == org.openmesh.android.data.transport.RadioPhase.READY && !demoMode) {
                SectionTitle("Link health")
                Card(colors = CardDefaults.cardColors(containerColor = MeshSurfaceHigh)) {
                    Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text(
                            if (radio.health == org.openmesh.android.data.transport.RadioHealth.HEALTHY) {
                                "Connected and answering"
                            } else {
                                "Connected but not answering — tap Reconnect"
                            },
                            style = MaterialTheme.typography.titleMedium,
                            color = radioToneColor(copy.tone),
                        )
                        Text(
                            "Firmware ${radio.firmwareVersion ?: "—"} · node ${radio.nodeId?.take(8) ?: "—"}",
                            style = MaterialTheme.typography.bodyMedium,
                        )
                        Text(
                            "${radio.transportName} · ${RadioPresentation.signalSummary(radio)}",
                            style = MaterialTheme.typography.bodySmall,
                            color = MeshOnSurfaceVariant,
                        )
                        TextButton(onClick = { onOpenHelp("radio-and-region") }) { Text("How the radio and region work") }
                    }
                }
            }

            SectionTitle("Signal and counters")
            Card(colors = CardDefaults.cardColors(containerColor = MeshSurfaceHigh)) {
                Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text("Radio tx queue ${radio.txQueue} · airtime ${"%.1f".format(radio.airtimePct)}% of budget")
                    Text("Waiting to send (queued) ${radio.queuedMessages}")
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
                "• Keep the radio switched on and within a few metres while connecting.\n" +
                    "• USB needs a data cable; accept the USB prompt when it appears.\n" +
                    "• Every node must use the same profile and region. Changing them briefly disconnects peers.\n" +
                    "• Messages you send with no radio are queued and sent when the radio connects.",
                style = MaterialTheme.typography.bodyMedium,
                color = MeshOnSurfaceVariant,
            )
            Spacer(Modifier.height(16.dp))
        }
    }
}

/** Scan → Connect → Handshake → Ready rail; each step has a shape AND a text label. */
@Composable
private fun StepRail(steps: List<RadioStep>) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        steps.forEachIndexed { index, step ->
            val color = when (step.state) {
                StepState.DONE -> MeshOnline
                StepState.ACTIVE -> MeshSecondary
                StepState.FAILED -> MeshError
                StepState.PENDING -> MeshOutline
            }
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Box(
                    Modifier
                        .size(18.dp)
                        .clip(CircleShape)
                        .background(if (step.state == StepState.PENDING) Color.Transparent else color.copy(alpha = 0.16f))
                        .border(1.dp, color, CircleShape),
                    contentAlignment = Alignment.Center,
                ) {
                    when (step.state) {
                        StepState.DONE -> Icon(Icons.Filled.Check, contentDescription = null, tint = color, modifier = Modifier.size(11.dp))
                        StepState.FAILED -> Icon(Icons.Filled.Error, contentDescription = null, tint = color, modifier = Modifier.size(11.dp))
                        StepState.ACTIVE -> Box(Modifier.size(6.dp).clip(CircleShape).background(color))
                        StepState.PENDING -> Unit
                    }
                }
                Spacer(Modifier.height(3.dp))
                Text(
                    step.label,
                    style = MaterialTheme.typography.labelSmall,
                    color = if (step.state == StepState.PENDING) MeshOnSurfaceVariant else color,
                )
            }
            if (index != steps.lastIndex) {
                Box(
                    Modifier
                        .weight(1f)
                        .padding(horizontal = 4.dp)
                        .height(2.dp)
                        .background(MeshOutline),
                )
            }
        }
    }
}
