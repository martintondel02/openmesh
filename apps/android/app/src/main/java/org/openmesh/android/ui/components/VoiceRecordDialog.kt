package org.openmesh.android.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import org.openmesh.android.media.VoicePlayer
import org.openmesh.android.media.VoiceRecorder
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshPrimary

/**
 * Records a voice note with MediaRecorder (AAC-LC mono 16 kHz ~12 kbps, MP4) capped to the
 * profile's duration, lets the user review/play it, then returns the bytes for the engine call.
 */
@Composable
fun VoiceRecordDialog(
    maxSeconds: Int,
    onRecord: (RecordedVoice) -> Unit,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    val recorder = remember { VoiceRecorder(context) }
    val player = remember { VoicePlayer(context) }
    var seconds by remember { mutableIntStateOf(0) }
    var recording by remember { mutableStateOf(false) }
    var recorded by remember { mutableStateOf<RecordedVoice?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var playing by remember { mutableStateOf(false) }

    LaunchedEffect(recording) {
        if (!recording) return@LaunchedEffect
        while (recording && seconds < maxSeconds) {
            delay(1000)
            seconds += 1
        }
        if (recording && seconds >= maxSeconds) {
            runCatching { recorder.stop() }.onSuccess {
                recorded = RecordedVoice(it.file.readBytes(), it.durationMs, it.file)
                recording = false
            }.onFailure { error = it.message }
        }
    }

    AlertDialog(
        onDismissRequest = {
            if (recording) runCatching { recorder.cancel() }
            player.stop()
            onDismiss()
        },
        title = { Text("Voice note") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(
                    "AAC-LC mono 16 kHz, ~12 kbps, capped at ${maxSeconds}s for this profile. " +
                        "The next step shows the airtime estimate before sending.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MeshOnSurfaceVariant,
                )
                Text(
                    if (recording) "Recording… ${seconds}s / ${maxSeconds}s"
                    else recorded?.let { "Recorded ${it.durationMs / 1000}s · ${it.bytes.size / 1024} KiB" }
                        ?: "Ready to record",
                    style = MaterialTheme.typography.titleMedium,
                )
                if (recording) {
                    Row(Modifier.fillMaxWidth().height(24.dp), verticalAlignment = Alignment.CenterVertically) {
                        Text("●", color = MeshPrimary)
                        Text("  keep speaking; it stops automatically", style = MaterialTheme.typography.bodySmall)
                    }
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        },
        confirmButton = {
            when {
                recorded != null -> Button(onClick = { onRecord(recorded!!) }) { Text("Use recording") }
                recording -> Button(onClick = {
                    runCatching { recorder.stop() }.onSuccess {
                        recorded = RecordedVoice(it.file.readBytes(), it.durationMs, it.file)
                    }.onFailure { error = it.message }
                    recording = false
                }) {
                    Icon(Icons.Filled.Stop, contentDescription = null)
                    Text(" Stop")
                }
                else -> Button(onClick = {
                    runCatching {
                        recorder.start("MEDIUM_FAST")
                        seconds = 0
                        recording = true
                    }.onFailure { error = it.message }
                }) {
                    Icon(Icons.Filled.Mic, contentDescription = null)
                    Text(" Record")
                }
            }
        },
        dismissButton = {
            Row {
                if (recorded != null) {
                    TextButton(onClick = {
                        playing = !playing
                        if (playing) {
                            recorded?.file?.let { file -> player.play(file) { playing = false } }
                        } else {
                            player.stop()
                        }
                    }) {
                        Icon(Icons.Filled.PlayArrow, contentDescription = null)
                        Text(if (playing) "Playing" else "Preview")
                    }
                }
                TextButton(onClick = {
                    if (recording) runCatching { recorder.cancel() }
                    player.stop()
                    onDismiss()
                }) { Text("Cancel") }
            }
        },
    )
}

data class RecordedVoice(val bytes: ByteArray, val durationMs: Int, val file: java.io.File? = null)
