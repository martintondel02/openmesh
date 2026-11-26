package org.openmesh.android.media

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ImageDecoder
import android.os.Build
import android.media.AudioAttributes
import android.media.MediaPlayer
import android.media.MediaRecorder
import android.net.Uri
import java.io.ByteArrayOutputStream
import java.io.File
import kotlin.math.max
import kotlin.math.min
import org.openmesh.android.util.MeshLog

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

    /** Result of tier compression; failures carry a user-facing reason and are logged. */
    sealed interface Result {
        data class Success(val bytes: ByteArray, val mime: String, val width: Int, val height: Int) : Result
        data class Failure(val reason: String) : Result
    }

    /**
     * Compress to a tier target (WebP, JPEG fallback), downscaling/quality-stepping until the
     * profile cap is met. Robust against content URIs that BitmapFactory cannot handle: uses
     * ImageDecoder (API 28+) with a software allocator first, then BitmapFactory sampling.
     */
    fun compressTier(context: Context, uri: Uri, maxDim: Int, quality: Double, capBytes: Int): Result {
        MeshLog.i("image compress: begin uri=$uri maxDim=$maxDim quality=$quality cap=$capBytes")
        var edge = maxDim
        var attempt = 0
        var lastSize = -1
        var lastReason = "no decode succeeded"
        while (attempt < 6) {
            attempt += 1
            val bitmap = decodeScaled(context, uri, edge)
            if (bitmap == null) {
                lastReason = "could not decode the selected image (attempt $attempt, edge=$edge)"
                MeshLog.w("image compress: $lastReason")
                edge = max(160, (edge * 3) / 4)
                continue
            }
            val (w, h) = MediaMath.scale(bitmap.width, bitmap.height, edge)
            val scaled = if (w == bitmap.width && h == bitmap.height) bitmap else
                Bitmap.createScaledBitmap(bitmap, w, h, true).also { if (it !== bitmap) bitmap.recycle() }
            val encoded = encode(scaled, quality)
            if (encoded == null) {
                lastReason = "could not encode the image (attempt $attempt, ${scaled.width}x${scaled.height})"
                MeshLog.w("image compress: $lastReason")
                scaled.recycle()
                edge = max(160, (edge * 3) / 4)
                continue
            }
            lastSize = encoded.first.size
            MeshLog.i(
                "image compress: attempt=$attempt edge=$edge size=${scaled.width}x${scaled.height} " +
                    "mime=${encoded.second} bytes=$lastSize cap=$capBytes",
            )
            if (encoded.first.isNotEmpty() && encoded.first.size <= capBytes) {
                val result = Result.Success(encoded.first, encoded.second, scaled.width, scaled.height)
                scaled.recycle()
                MeshLog.i("image compress: success mime=${result.mime} bytes=${result.bytes.size} ${result.width}x${result.height}")
                return result
            }
            scaled.recycle()
            // Reduce dimensions first, then quality on later attempts.
            edge = max(160, (edge * 3) / 4)
        }
        val reason = if (lastSize > 0) {
            "Image is ${(lastSize / 1024.0).toString().take(4)} KiB after compression, above the ${capBytes / 1024} KiB profile cap; try a smaller tier."
        } else {
            lastReason
        }
        MeshLog.w("image compress: failed: $reason")
        return Result.Failure(reason)
    }

    private fun decodeScaled(context: Context, uri: Uri, edge: Int): Bitmap? {
        if (Build.VERSION.SDK_INT >= 28) {
            runCatching {
                val source = ImageDecoder.createSource(context.contentResolver, uri)
                return ImageDecoder.decodeBitmap(source) { decoder, info, _ ->
                    decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
                    val w = info.size.width
                    val h = info.size.height
                    val (tw, th) = MediaMath.scale(w, h, edge)
                    if (tw != w || th != h) decoder.setTargetSize(tw, th)
                }
            }.onFailure { MeshLog.w("image compress: ImageDecoder failed (${it.message}); falling back to BitmapFactory") }
        }
        // BitmapFactory fallback (also API 26/27): bounds first, then sampled decode; retry full
        // decode if the sampled one fails, and re-open the stream each time (content URIs are
        // usually re-openable; log the failure clearly if not).
        return runCatching {
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            val boundsOk = context.contentResolver.openInputStream(uri)?.use {
                BitmapFactory.decodeStream(it, null, bounds) != null || bounds.outWidth > 0
            } ?: false
            if (!boundsOk || bounds.outWidth <= 0 || bounds.outHeight <= 0) {
                MeshLog.w("image compress: bounds decode failed for $uri (boundsOk=$boundsOk ${bounds.outWidth}x${bounds.outHeight})")
                return@runCatching null
            }
            val sample = sampleSize(bounds.outWidth, bounds.outHeight, edge)
            val options = BitmapFactory.Options().apply {
                inSampleSize = sample
                inPreferredConfig = Bitmap.Config.ARGB_8888
            }
            val sampled = decodeStream(context, uri, options)
            MeshLog.i("image compress: BitmapFactory decode sample=$sample bounds=${bounds.outWidth}x${bounds.outHeight} -> ${sampled?.width}x${sampled?.height}")
            if (sampled != null) return@runCatching sampled
            val fullOptions = BitmapFactory.Options()
            decodeStream(context, uri, fullOptions)
        }.getOrElse {
            MeshLog.w("image compress: decode threw: ${it.message}")
            null
        }
    }

    private fun decodeStream(context: Context, uri: Uri, options: BitmapFactory.Options): Bitmap? =
        runCatching {
            context.contentResolver.openInputStream(uri)?.use { stream ->
                BitmapFactory.decodeStream(stream, null, options)
            }.also { if (it == null) MeshLog.w("image compress: decodeStream returned null for $uri") }
        }.getOrElse {
            MeshLog.w("image compress: open/decode failed for $uri: ${it.message}")
            null
        }

    /** WebP (lossy where supported) with JPEG fallback; returns bytes + actual MIME or null. */
    private fun encode(bitmap: Bitmap, quality: Double): Pair<ByteArray, String>? {
        val q = (quality * 100).toInt().coerceIn(1, 100)
        val webp = runCatching {
            val out = ByteArrayOutputStream()
            val format = if (Build.VERSION.SDK_INT >= 30) Bitmap.CompressFormat.WEBP_LOSSY else
                @Suppress("DEPRECATION") Bitmap.CompressFormat.WEBP
            val ok = bitmap.compress(format, q, out)
            MeshLog.i("image compress: webp q=$q ok=$ok bytes=${out.size()}")
            if (ok && out.size() > 0) out.toByteArray() to "image/webp" else null
        }.getOrElse {
            MeshLog.w("image compress: webp threw: ${it.message}")
            null
        }
        if (webp != null) return webp
        return runCatching {
            val out = ByteArrayOutputStream()
            val ok = bitmap.compress(Bitmap.CompressFormat.JPEG, q, out)
            MeshLog.i("image compress: jpeg fallback q=$q ok=$ok bytes=${out.size()}")
            if (ok && out.size() > 0) out.toByteArray() to "image/jpeg" else null
        }.getOrElse {
            MeshLog.w("image compress: jpeg threw: ${it.message}")
            null
        }
    }

    /** Original image bytes + resolver MIME; the caller enforces the profile cap and confirmation. */
    fun readOriginal(context: Context, uri: Uri): Pair<ByteArray, String>? {
        val bytes = context.contentResolver.openInputStream(uri)?.use { it.readBytes() } ?: return null
        val mime = context.contentResolver.getType(uri) ?: "application/octet-stream"
        return bytes to mime
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

    var lastMime: String = "audio/mp4"
        private set

    fun start(profile: String): File {
        val useOpus = android.os.Build.VERSION.SDK_INT >= 29
        lastMime = if (useOpus) "audio/ogg; codecs=opus" else "audio/mp4"
        val target = File(context.filesDir, "voice").apply { mkdirs() }
            .resolve("voice-${System.currentTimeMillis()}.${if (useOpus) "ogg" else "m4a"}")
        val media = if (android.os.Build.VERSION.SDK_INT >= 31) {
            MediaRecorder(context)
        } else {
            @Suppress("DEPRECATION")
            MediaRecorder()
        }
        media.setAudioSource(MediaRecorder.AudioSource.MIC)
        if (useOpus) {
            media.setOutputFormat(MediaRecorder.OutputFormat.OGG)
            media.setAudioEncoder(MediaRecorder.AudioEncoder.OPUS)
            media.setAudioSamplingRate(48_000)
            media.setAudioChannels(1)
            media.setAudioEncodingBitRate(12_000)
        } else {
            media.setOutputFormat(MediaRecorder.OutputFormat.MPEG_4)
            media.setAudioEncoder(MediaRecorder.AudioEncoder.AAC)
            media.setAudioSamplingRate(16_000)
            media.setAudioChannels(1)
            media.setAudioEncodingBitRate(12_000)
        }
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
