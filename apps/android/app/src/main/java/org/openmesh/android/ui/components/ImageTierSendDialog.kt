package org.openmesh.android.ui.components

import android.net.Uri
import android.os.Build
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.openmesh.android.data.repository.MediaKind
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.media.ImageCompressor
import org.openmesh.android.media.MediaFormats
import org.openmesh.android.media.MediaTierPolicy
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshPrimary

/**
 * Image tier chooser (Tiny 320 / Medium 640 / High 1280 / Original), compression + pre-send
 * preview with actual bytes, airtime and duty-budget share (v0.6.0 media tiers).
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ImageTierSendDialog(
    repository: MeshRepository,
    peerId: String,
    isChannel: Boolean,
    uri: Uri,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val settings by repository.settings.collectAsState()
    val presets = repository.mediaTiers
    val cap = repository.mediaCapBytes()
    val suggested = repository.suggestImageTier(cap)
    val defaultTier = repository.defaultImageTier()
    val defaultFits = defaultTier != "original" &&
        MediaTierPolicy.estimateImageBytes(MediaTierPolicy.imageTier(presets, defaultTier)) <= cap
    var selected by remember { mutableStateOf(if (defaultFits) defaultTier else suggested) }
    var result by remember { mutableStateOf<ImageCompressor.Result?>(null) }
    var mime by remember { mutableStateOf(MediaFormats.preferredImageMime(Build.VERSION.SDK_INT)) }
    var dimensions by remember { mutableStateOf<Pair<Int, Int>?>(null) }
    var preparing by remember { mutableStateOf(false) }
    var originalConfirmed by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val originalAllowed = MediaTierPolicy.originalAllowed(settings.profile)

    suspend fun prepare(id: String) {
        preparing = true
        error = null
        result = null
        dimensions = null
        withContext(Dispatchers.IO) {
            if (id == "original") {
                val (raw, rawMime) = ImageCompressor.readOriginal(context, uri)
                    ?: return@withContext ImageCompressor.Result.Failure("Could not read the selected image.")
                mime = rawMime
                ImageCompressor.Result.Success(raw, rawMime, 0, 0)
            } else {
                val tier = MediaTierPolicy.imageTier(presets, id)
                // Preset target (now image/avif); empty = platform preference. The encoder reports
                // the real MIME afterwards, so a WebP fallback is labelled honestly.
                mime = tier.mime.ifBlank { MediaFormats.preferredImageMime(Build.VERSION.SDK_INT) }
                ImageCompressor.compressTier(
                    context,
                    uri,
                    maxDim = tier.maxDim ?: 1280,
                    quality = tier.quality ?: 0.7,
                    capBytes = cap,
                    avifEncoder = ImageCompressor.AvifEncoder { rgba, width, height, quality, speed ->
                        repository.encodeAvif(rgba, width, height, quality, speed)
                    },
                    avifSpeed = MediaFormats.avifSpeedForTier(id),
                )
            }
        }.also { outcome ->
            when (outcome) {
                is ImageCompressor.Result.Success -> {
                    result = outcome
                    mime = outcome.mime
                    dimensions = outcome.width to outcome.height
                }
                is ImageCompressor.Result.Failure -> error = outcome.reason
            }
        }
        preparing = false
    }

    LaunchedEffect(selected) { prepare(selected) }

    val bytes = (result as? ImageCompressor.Result.Success)?.bytes
    val actualBytes = bytes?.size ?: 0
    val estimate = if (actualBytes > 0) repository.estimateMedia(actualBytes, MediaKind.IMAGE) else null
    val overCap = actualBytes > cap
    // Original always needs an explicit confirmation when it is selectable at all.
    val originalBlocked = selected == "original" && (!originalAllowed || !originalConfirmed)

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Send image · choose tier") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    presets.imageOrder.forEach { id ->
                        val tier = presets.images.getValue(id)
                        val disabled = id == "original" && !originalAllowed
                        FilterChip(
                            selected = selected == id,
                            enabled = !disabled,
                            modifier = Modifier.padding(end = 2.dp),
                            onClick = {
                                selected = id
                                originalConfirmed = false
                            },
                            label = {
                                Text(
                                    when (id) {
                                        "original" -> "Original"
                                        else -> id.replaceFirstChar { it.uppercase() } + " ${tier.maxDim}"
                                    },
                                )
                            },
                        )
                    }
                }
                Text(
                    "Tier sizes are targets; the preview shows the measured bytes. Profile cap: ${cap / 1024} KiB.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MeshOnSurfaceVariant,
                )
                if (!originalAllowed) {
                    Text(
                        "Original is disabled on long-range profiles (${settings.profile}); use a tier so the image fits the link.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error,
                    )
                }
                when {
                    preparing -> Text("Compressing…")
                    bytes != null -> {
                        Text(
                            "Actual size: ${"%.1f".format(actualBytes / 1024.0)} KiB (${mime.substringBefore(';')})",
                            style = MaterialTheme.typography.titleSmall,
                        )
                        estimate?.let {
                            Text(
                                "≈ ${"%.1f".format(it.airtimeSeconds)} s airtime at ${it.profile.name} · " +
                                    "${"%.1f".format(it.fractionOfHourlyBudget * 100)} % of the hourly duty budget",
                                color = MeshPrimary,
                            )
                        }
                        if (overCap) {
                            Text(
                                "This exceeds the ${cap / 1024} KiB attachment cap; the next smaller tier is $suggested.",
                                color = MaterialTheme.colorScheme.error,
                                style = MaterialTheme.typography.bodySmall,
                            )
                        }
                        if (selected == "original" && originalAllowed) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Checkbox(checked = originalConfirmed, onCheckedChange = { originalConfirmed = it })
                                Text(
                                    "Send the original size (larger airtime; may exceed the cap).",
                                    style = MaterialTheme.typography.bodySmall,
                                )
                            }
                        }
                    }
                    else -> Text("Select a tier to prepare the preview.")
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        },
        confirmButton = {
            Button(
                enabled = bytes != null && !preparing && !overCap && !originalBlocked,
                onClick = {
                    val data = bytes ?: return@Button
                    scope.launch {
                        val name = MediaFormats.fileNameForTier(selected, mime)
                        repository.sendAttachmentBytes(peerId, name, mime, data, null, isChannel)
                        onDismiss()
                    }
                },
            ) { Text("Send") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}
