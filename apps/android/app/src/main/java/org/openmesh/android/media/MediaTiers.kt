package org.openmesh.android.media

import android.content.Context
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import java.io.File
import kotlin.math.floor

@Serializable
data class ImageTier(
    val id: String = "",
    @SerialName("max_dim") val maxDim: Int? = null,
    val quality: Double? = null,
    val mime: String = "image/webp",
)

@Serializable
data class VoiceTier(
    val id: String = "",
    val codec: String,
    val mode: String,
    val mime: String,
    @SerialName("sample_rate") val sampleRate: Int,
    val channels: Int = 1,
    @SerialName("frame_ms") val frameMs: Int? = null,
    @SerialName("bits_per_second") val bitsPerSecond: Int,
    @SerialName("max_duration_s") val maxDurationS: Int,
)

@Serializable
data class MediaTierDefaults(val image: String = "medium", val voice: String = "tiny")

@Serializable
data class MediaTierPresets(
    val version: Int = 1,
    @SerialName("airtime_note") val airtimeNote: String = "",
    val images: Map<String, ImageTier> = emptyMap(),
    val voice: Map<String, VoiceTier> = emptyMap(),
    val defaults: MediaTierDefaults = MediaTierDefaults(),
) {
    val imageOrder: List<String> get() = listOf("tiny", "medium", "high", "original").filter { it in images }
    val voiceOrder: List<String> get() = listOf("tiny", "medium", "high").filter { it in voice }
}

object MediaTierLoader {
    val json = Json { ignoreUnknownKeys = true }

    fun parse(text: String): MediaTierPresets = json.decodeFromString(text)

    fun fromAssets(context: Context): MediaTierPresets =
        context.assets.open("presets/media.json").bufferedReader().use { parse(it.readText()) }

    fun fromRepoFile(candidates: List<File> = defaultRepoCandidates()): MediaTierPresets {
        val file = candidates.firstOrNull { it.isFile }
            ?: error("content/presets/media.json not found; tried:\n" + candidates.joinToString("\n") { "  ${it.absolutePath}" })
        return parse(file.readText())
    }

    fun defaultRepoCandidates(): List<File> {
        val cwd = File(System.getProperty("user.dir"))
        val prop = System.getProperty("openmesh.repoRoot")?.let(::File)
        return buildList {
            if (prop != null) add(File(prop, "content/presets/media.json"))
            add(File(cwd, "../../content/presets/media.json"))
            add(File(cwd, "../../../content/presets/media.json"))
            add(File(cwd, "content/presets/media.json"))
        }.map { it.canonicalFile }
    }
}

/** Image/voice tier policy shared by composer, recorder and tests. */
object MediaTierPolicy {
    fun imageTier(presets: MediaTierPresets, id: String): ImageTier =
        presets.images[id.lowercase()] ?: presets.images.getValue(presets.defaults.image)

    fun voiceTier(presets: MediaTierPresets, id: String): VoiceTier =
        presets.voice[id.lowercase()] ?: presets.voice.getValue(presets.defaults.voice)

    /** Rough target bytes for a WebP tier: actual bytes are measured after compression. */
    fun estimateImageBytes(tier: ImageTier): Long {
        val dim = tier.maxDim ?: return Long.MAX_VALUE
        val q = tier.quality ?: 0.7
        val pixels = dim.toLong() * dim
        return (pixels * q * 0.06).toLong().coerceAtLeast(64)
    }

    /** Largest tier within a budget; falls back to tiny (never returns "original"). */
    fun suggestImageTier(presets: MediaTierPresets, capBytes: Int): String {
        val candidates = presets.imageOrder.filter { it != "original" }
        return candidates.lastOrNull { estimateImageBytes(presets.images.getValue(it)) <= capBytes }
            ?: candidates.first()
    }

    /** Original is allowed only with explicit confirmation and is blocked on long-range profiles. */
    fun originalAllowed(profile: String): Boolean = !profile.uppercase().startsWith("LONG_")

    fun voiceBytesPerSecond(tier: VoiceTier): Int = when {
        // Codec2 raw frame sizes from the crate: 7 B/40 ms (1300) and 8 B/20 ms (3200).
        tier.codec == "codec2" && tier.mode == "1300" -> 175
        tier.codec == "codec2" && tier.mode == "3200" -> 400
        else -> (tier.bitsPerSecond + 7) / 8
    }

    fun effectiveVoiceMaxSeconds(tier: VoiceTier, profileCapBytes: Int): Int {
        val byBytes = floor(profileCapBytes.toDouble() / voiceBytesPerSecond(tier).coerceAtLeast(1)).toInt()
        return minOf(tier.maxDurationS, byBytes).coerceAtLeast(0)
    }

    fun voiceMime(tierId: String, presets: MediaTierPresets): String =
        presets.voice[tierId.lowercase()]?.mime ?: "audio/c2-1300"

    fun voiceIsCodec2(mime: String): Boolean = mime.startsWith("audio/c2-")

    /** Label for the message bubble, e.g. "Codec2 1.3k · 8 s · 1.4 KiB". */
    fun attachmentLabel(mime: String, durationMs: Int?, sizeBytes: Int): String {
        val format = when {
            mime.startsWith("audio/c2-1300") -> "Codec2 1.3k"
            mime.startsWith("audio/c2-3200") -> "Codec2 3.2k"
            mime.contains("opus") -> "Opus"
            mime.startsWith("audio/mp4") -> "AAC"
            mime.startsWith("image/webp") -> "WebP"
            mime.startsWith("image/jpeg") -> "JPEG"
            mime.startsWith("image/") -> "Image"
            else -> mime.substringBefore(';').ifBlank { "Attachment" }
        }
        val duration = durationMs?.let { "${((it + 500) / 1000)} s" }
        val size = when {
            sizeBytes >= 1024 -> String.format("%.1f KiB", sizeBytes / 1024.0)
            else -> "$sizeBytes B"
        }
        return listOfNotNull(format, duration, size).joinToString(" · ")
    }
}
