package org.openmesh.android.media

import java.nio.charset.StandardCharsets
import kotlin.math.roundToInt

/**
 * Pure image-format helpers: AVIF capability preference, real-AVIF verification (magic bytes),
 * filename extensions and user-facing labels. Android-free so they are unit-tested on the JVM.
 */
object MediaFormats {
    const val AVIF_MIME: String = "image/avif"
    const val WEBP_MIME: String = "image/webp"
    const val JPEG_MIME: String = "image/jpeg"

    /** Preferred encode target: AVIF on API 30+ (Bitmap.CompressFormat.AVIF), else WebP lossy. */
    fun preferredImageMime(apiLevel: Int): String = if (apiLevel >= 30) AVIF_MIME else WEBP_MIME

    /**
     * True only when [bytes] really is an AVIF/AVIS ISO-BMFF file (`....ftyp` + `avif`/`avis` brand).
     * A compress() call can return success with empty/unknown bytes on some devices; never label
     * those as AVIF.
     */
    fun isAvif(bytes: ByteArray): Boolean {
        if (bytes.size < 12) return false
        if (!bytes.copyOfRange(4, 8).contentEquals("ftyp".toByteArray(StandardCharsets.US_ASCII))) return false
        val brand = String(bytes, 8, 4, StandardCharsets.US_ASCII)
        return brand == "avif" || brand == "avis"
    }

    fun extensionFor(mime: String): String = when {
        mime.contains("avif", ignoreCase = true) -> "avif"
        mime.contains("webp", ignoreCase = true) -> "webp"
        mime.contains("jpeg", ignoreCase = true) || mime.contains("jpg", ignoreCase = true) -> "jpg"
        else -> "img"
    }

    /** Preset quality (0..1) to the AVIF encoder's 1..100 quality byte. */
    fun avifQuality(quality: Double): Int =
        (quality * 100.0).roundToInt().coerceIn(1, 100)

    /** Encoder speed: tiny is fastest (10), medium/high use 8 (quality-biased but still fast). */
    fun avifSpeedForTier(tierId: String): Int = if (tierId.equals("tiny", ignoreCase = true)) 10 else 8

    /**
     * ravif numeric quality is perceptually lower than libwebp at the same number: match WebP
     * quality by scaling ~1.25x (integer floor, capped at 100).
     */
    fun calibratedAvifQuality(webpQuality: Int): Int =
        minOf(100, webpQuality.coerceIn(1, 100) * 5 / 4)

    enum class ImageFormatChoice { AVIF, WEBP }

    /** Send the strictly smaller encoding; WebP wins ties (never regress airtime on equal bytes). */
    fun chooseImageFormat(avifBytes: Int, webpBytes: Int): ImageFormatChoice =
        if (avifBytes in 1 until webpBytes) ImageFormatChoice.AVIF else ImageFormatChoice.WEBP

    /** Short format label for attachment subtitles ("AVIF", "WebP", "JPEG", …). */
    fun labelFor(mime: String): String = when {
        mime.contains("avif", ignoreCase = true) -> "AVIF"
        mime.contains("webp", ignoreCase = true) -> "WebP"
        mime.contains("jpeg", ignoreCase = true) -> "JPEG"
        mime.contains("png", ignoreCase = true) -> "PNG"
        mime.startsWith("image/", ignoreCase = true) -> "Image"
        else -> mime.substringBefore(';').ifBlank { "Attachment" }
    }

    /** Bubble filename for a compressed image tier: image-<tier>.<real extension>. */
    fun fileNameForTier(tier: String, mime: String): String =
        "image-${tier.substringBefore(' ').lowercase()}.${extensionFor(mime)}"
}
