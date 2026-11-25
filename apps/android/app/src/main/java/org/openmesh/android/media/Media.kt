package org.openmesh.android.media

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.media.AudioAttributes
import android.media.MediaPlayer
import android.media.MediaRecorder
import android.net.Uri
import java.io.ByteArrayOutputStream
import java.io.File
import kotlin.math.max
import kotlin.math.min

/** Attachment caps and voice limits per LoRa profile (docs/architecture/PROTOCOL.md §9.4). */
object MediaMath {
    fun capBytes(profile: String): Int = when (profile.uppercase()) {
        "SHORT_TURBO", "SHORT_FAST" -> 24 * 1024
        "MEDIUM_FAST" -> 12 * 1024
        "LONG_FAST" -> 6 * 1024
        "LONG_SLOW" -> 1536
        else -> 12 * 1024
    }

    fun maxVoiceSeconds(profile: String): Int = when (profile.uppercase()) {
        "SHORT_TURBO", "SHORT_FAST" -> 15
        "MEDIUM_FAST" -> 10
        "LONG_FAST" -> 8
        "LONG_SLOW" -> 4
        else -> 10
    }

    /** Downscale dimensions so the longest edge is at most [maxEdge] (never upscale). */
    fun scale(width: Int, height: Int, maxEdge: Int): Pair<Int, Int> {
        if (width <= 0 || height <= 0) return 1 to 1
        if (width <= maxEdge && height <= maxEdge) return width to height
        val ratio = min(maxEdge.toDouble() / width, maxEdge.toDouble() / height)
        return max(1, (width * ratio).toInt()) to max(1, (height * ratio).toInt())
    }

    fun maxEdgeForCap(capBytes: Int): Int = when {
        capBytes <= 2 * 1024 -> 320
        capBytes <= 6 * 1024 -> 512
        capBytes <= 12 * 1024 -> 768
        else -> 1024
    }

    /** Rough target bytes for tests: JPEG at quality 80 is ~0.12 bytes/pixel on photos. */
    fun estimatedJpegBytes(width: Int, height: Int, quality: Int = 80): Int =
        (width * height * (quality / 100.0) * 0.12).toInt()
}

/** Decode → downscale → JPEG-compress until the bytes fit the profile cap. */
object ImageCompressor {
    fun compress(context: Context, uri: Uri, capBytes: Int): ByteArray? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        context.contentResolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
            ?: return null
        var maxEdge = MediaMath.maxEdgeForCap(capBytes)
        repeat(4) {
            val sample = sampleSize(bounds.outWidth, bounds.outHeight, maxEdge * 2)
            val options = BitmapFactory.Options().apply { inSampleSize = sample }
            val bitmap = context.contentResolver.openInputStream(uri)?.use {
                BitmapFactory.decodeStream(it, null, options)
            } ?: return null
            val (w, h) = MediaMath.scale(bitmap.width, bitmap.height, maxEdge)
            val scaled = if (w == bitmap.width && h == bitmap.height) bitmap else Bitmap.createScaledBitmap(bitmap, w, h, true)
            for (quality in intArrayOf(85, 70, 55, 40, 30)) {
                val out = ByteArrayOutputStream()
                scaled.compress(Bitmap.CompressFormat.JPEG, quality, out)
                if (quality == 30 && out.size() > capBytes) {
                    // fall through to a smaller dimension on the next iteration
                }
                if (out.size() <= capBytes) {
                    if (scaled !== bitmap) scaled.recycle()
                    bitmap.recycle()
                    return out.toByteArray()
                }
                if (quality == 30) {
                    if (scaled !== bitmap) scaled.recycle()
                    bitmap.recycle()
                    return@repeat
                }
            }
            maxEdge = max(160, (maxEdge * 3) / 4)
        }
        return null
    }

    fun sampleSize(width: Int, height: Int, targetEdge: Int): Int {
        var sample = 1
        var w = width
        var h = height
        while (w / 2 >= targetEdge && h / 2 >= targetEdge) {
            w /= 2
            h /= 2
            sample *= 2
        }
        return sample
    }
}

/** AAC-LC mono 16 kHz ~12 kbps MP4 voice recorder (Android-native alternative to web Opus). */
class VoiceRecorder(private val context: Context) {
    private var recorder: MediaRecorder? = null
    private var file: File? = null
    private var startedAt = 0L
    private val amplitudes = mutableListOf<Int>()

    val waveform: List<Int> get() = amplitudes.toList()

    fun start(profile: String): File {
        val target = File(context.filesDir, "voice").apply { mkdirs() }
            .resolve("voice-${System.currentTimeMillis()}.m4a")
        val media = if (android.os.Build.VERSION.SDK_INT >= 31) {
            MediaRecorder(context)
        } else {
            @Suppress("DEPRECATION")
            MediaRecorder()
        }
        media.setAudioSource(MediaRecorder.AudioSource.MIC)
        media.setOutputFormat(MediaRecorder.OutputFormat.MPEG_4)
        media.setAudioEncoder(MediaRecorder.AudioEncoder.AAC)
        media.setAudioSamplingRate(16_000)
        media.setAudioChannels(1)
        media.setAudioEncodingBitRate(12_000)
        media.setMaxDuration(MediaMath.maxVoiceSeconds(profile) * 1000)
        media.setOutputFile(target.absolutePath)
        media.prepare()
        media.start()
        recorder = media
        file = target
        startedAt = System.currentTimeMillis()
        amplitudes.clear()
        return target
    }

    fun stop(): Recording {
        val media = recorder ?: error("not recording")
        val amplitude = runCatching { media.maxAmplitude }.getOrDefault(0)
        amplitudes += amplitude
        media.stop()
        media.release()
        recorder = null
        val target = file ?: error("no file")
        val duration = (System.currentTimeMillis() - startedAt).toInt()
        return Recording(target, duration, target.length().toInt(), amplitudes.toList())
    }

    fun cancel() {
        runCatching { recorder?.stop() }
        runCatching { recorder?.release() }
        recorder = null
        file?.delete()
        file = null
    }

    data class Recording(val file: File, val durationMs: Int, val bytes: Int, val waveform: List<Int>)

}

/** Simple playback + waveform normalisation for the thread UI. */
class VoicePlayer(private val context: Context) {
    private var player: MediaPlayer? = null

    fun play(file: File, onDone: () -> Unit = {}) {
        stop()
        player = MediaPlayer().apply {
            setAudioAttributes(
                AudioAttributes.Builder()
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .build(),
            )
            setDataSource(file.absolutePath)
            setOnCompletionListener { onDone(); stop() }
            prepare()
            start()
        }
    }

    fun stop() {
        runCatching { player?.stop() }
        runCatching { player?.release() }
        player = null
    }

    companion object {
        /** Normalise amplitude samples to 0..1 for drawing bars. */
        fun normalize(samples: List<Int>): List<Float> {
            if (samples.isEmpty()) return emptyList()
            val max = samples.maxOrNull()?.coerceAtLeast(1) ?: 1
            return samples.map { it.toFloat() / max }
        }
    }
}
