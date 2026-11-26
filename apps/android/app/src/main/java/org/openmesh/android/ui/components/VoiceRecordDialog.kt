package org.openmesh.android.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.openmesh.android.data.repository.MediaKind
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.media.FfiVoiceCodec
import org.openmesh.android.media.MediaTierPolicy
import org.openmesh.android.media.PcmRecorder
import org.openmesh.android.media.VoiceRecorder
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshPrimary

/**
 * Voice recorder with Codec2 tiers (tiny 1.3k / medium 3.2k via AudioRecord + FFI) and the platform
 * Opus/AAC high tier (MediaRecorder), v0.6.0 media tiers. Shows bytes + airtime before sending.
 */
@Composable
fun VoiceTierRecordDialog(
    repository: MeshRepository,
    peerId: String,
    isChannel: Boolean,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val codec = remember { FfiVoiceCodec() }
    var tierId by remember { mutableStateOf(repository.defaultVoiceTier()) }
    val tiers = repository.mediaTiers
    val tier = MediaTierPolicy.voiceTier(tiers, tierId)
    val effectiveCap = repository.effectiveVoiceMaxSeconds(tierId)
    var recording by remember { mutableStateOf(false) }
    var seconds by remember { mutableIntStateOf(0) }
    var result by remember { mutableStateOf<RecordedVoice?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }

    val pcmRecorder = remember { PcmRecorder(context) }
    val highRecorder = remember { VoiceRecorder(context) }

    LaunchedEffect(recording) {
        if (!recording) return@LaunchedEffect
        while (recording && seconds < effectiveCap) {
            delay(1_000)
            seconds++
        }
        if (recording && seconds >= effectiveCap) {
            stopRecording(
                tierId = tierId,
                high = tierId == "high",
                tier = tier,
                effectiveCap = effectiveCap,
                pcmRecorder = pcmRecorder,
                highRecorder = highRecorder,
                repository = repository,
                codec = codec,
                onResult = { result = it; error = null },
                onError = { error = it; result = null },
                setBusy = { busy = it },
                scope = scope,
            )
            recording = false
        }
    }

    val estimate = result?.bytes?.let { repository.estimateMedia(it.size, MediaKind.VOICE) }
    val overCap = (result?.bytes?.size ?: 0) > repository.mediaCapBytes()

    AlertDialog(
        onDismissRequest = {
            if (recording) {
                runCatching { pcmRecorder.cancel() }
                runCatching { highRecorder.cancel() }
            }
            onDismiss()
        },
        title = { Text("Voice note · choose tier") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    tiers.voiceOrder.forEach { id ->
                        val info = tiers.voice.getValue(id)
                        FilterChip(
                            selected = tierId == id,
                            enabled = !recording,
                            onClick = { tierId = id; result = null; error = null; seconds = 0 },
                            label = {
                                Text(
                                    when (id) {
                                        "tiny" -> "Tiny 1.3k"
                                        "medium" -> "Medium 3.2k"
                                        else -> "High Opus"
                                    },
                                )
                            },
                        )
                    }
                }
                Text(
                    "Cap for ${tier.mime.substringBefore(';')}: ${effectiveCap}s " +
                        "(tier max ${tier.maxDurationS}s, profile cap ${repository.mediaCapBytes() / 1024} KiB).",
                    style = MaterialTheme.typography.bodySmall,
                    color = MeshOnSurfaceVariant,
                )
                Text(
                    if (recording) "Recording… ${seconds}s / ${effectiveCap}s"
                    else result?.let { "Recorded ${it.durationMs / 1000}s · ${"%.1f".format(it.bytes.size / 1024.0)} KiB · ${it.mime}" }
                        ?: "Ready — ${tier.codec} ${tier.mode}",
                    style = MaterialTheme.typography.titleSmall,
                )
                if (recording) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("●", color = MeshPrimary)
                        Text("  recording; stops automatically at the cap", style = MaterialTheme.typography.bodySmall)
                    }
                }
                if (estimate != null) {
                    Text(
                        "≈ ${"%.1f".format(estimate.airtimeSeconds)} s airtime · " +
                            "${"%.1f".format(estimate.fractionOfHourlyBudget * 100)} % of the hourly duty budget",
                        color = MeshPrimary,
                    )
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        },
        confirmButton = {
            when {
                result != null -> Button(
                    enabled = !busy && !overCap,
                    onClick = {
                        val data = result ?: return@Button
                        repository.sendAttachmentBytes(
                            peerId,
                            "voice-${data.durationMs / 1000}s.${if (data.mime.contains("ogg") || data.mime.startsWith("audio/c2")) "c2" else "m4a"}",
                            data.mime,
                            data.bytes,
                            data.durationMs.toLong(),
                            isChannel,
                        )
                        onDismiss()
                    },
                ) { Text(if (overCap) "Too large" else "Send voice note") }
                busy -> Button(onClick = {}, enabled = false) { Text("Encoding…") }
                recording -> Button(onClick = {
                    stopRecording(
                        tierId = tierId,
                        high = tierId == "high",
                        tier = tier,
                        effectiveCap = effectiveCap,
                        pcmRecorder = pcmRecorder,
                        highRecorder = highRecorder,
                        repository = repository,
                        codec = codec,
                        onResult = { result = it; error = null },
                        onError = { error = it; result = null },
                        setBusy = { busy = it },
                        scope = scope,
                    )
                    recording = false
                }) {
                    Icon(Icons.Filled.Stop, contentDescription = null)
                    Text(" Stop")
                }
                else -> Button(onClick = {
                    error = null
                    result = null
                    seconds = 0
                    runCatching {
                        if (tierId == "high") highRecorder.start(repository.settings.value.profile) else pcmRecorder.start(effectiveCap)
                        recording = true
                    }.onFailure { error = it.message ?: "Could not start recording" }
                }) {
                    Icon(Icons.Filled.Mic, contentDescription = null)
                    Text(" Record")
                }
            }
        },
        dismissButton = {
            Row {
                if (recording) {
                    OutlinedButton(onClick = {
                        runCatching { pcmRecorder.cancel() }
                        runCatching { highRecorder.cancel() }
                        recording = false
                        seconds = 0
                    }) { Text("Cancel recording") }
                }
                TextButton(onClick = onDismiss) { Text("Close") }
            }
        },
    )
}

private fun stopRecording(
    tierId: String,
    high: Boolean,
    tier: org.openmesh.android.media.VoiceTier,
    effectiveCap: Int,
    pcmRecorder: PcmRecorder,
    highRecorder: VoiceRecorder,
    repository: MeshRepository,
    codec: FfiVoiceCodec,
    onResult: (RecordedVoice) -> Unit,
    onError: (String) -> Unit,
    setBusy: (Boolean) -> Unit,
    scope: kotlinx.coroutines.CoroutineScope,
) {
    setBusy(true)
    if (high) {
        runCatching { highRecorder.stop() }
            .onSuccess { recording ->
                val bytes = recording.file.readBytes()
                onResult(RecordedVoice(bytes, recording.durationMs, highRecorder.lastMime))
            }
            .onFailure { onError(it.message ?: "Recording failed") }
        setBusy(false)
    } else {
        val pcm = runCatching { pcmRecorder.stop() }
            .getOrElse { onError(it.message ?: "Recording failed"); setBusy(false); return }
        scope.launch {
            runCatching {
                withContext(Dispatchers.Default) { codec.encode(tierId, pcm.pcm) }
            }.onSuccess { payload ->
                onResult(RecordedVoice(payload, pcm.durationMs, tier.mime))
            }.onFailure { onError(it.message ?: "Codec2 encoding failed") }
            setBusy(false)
        }
    }
}

data class RecordedVoice(val bytes: ByteArray, val durationMs: Int, val mime: String)
