package org.openmesh.android.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** JVM tests for AVIF verification, capability preference, extensions and format labels. */
class MediaFormatsTest {
    private fun isoBmffBytes(majorBrand: String): ByteArray {
        val header = ByteArray(16)
        header[0] = 0x00
        header[1] = 0x00
        header[2] = 0x00
        header[3] = 0x20
        "ftyp".toByteArray().copyInto(header, 4)
        majorBrand.toByteArray().copyInto(header, 8)
        return header
    }

    @Test
    fun `real avif and avis brands are recognised`() {
        assertTrue(MediaFormats.isAvif(isoBmffBytes("avif")))
        assertTrue(MediaFormats.isAvif(isoBmffBytes("avis")))
    }

    @Test
    fun `webp jpeg empty and truncated bytes are not avif`() {
        assertFalse(MediaFormats.isAvif(ByteArray(0)))
        assertFalse(MediaFormats.isAvif(byteArrayOf(0x1, 0x2, 0x3)))
        // RIFF/WEBP header
        val webp = ByteArray(16).apply {
            "RIFF".toByteArray().copyInto(this, 0)
            "WEBP".toByteArray().copyInto(this, 8)
        }
        assertFalse(MediaFormats.isAvif(webp))
        // JPEG SOI header
        assertFalse(MediaFormats.isAvif(byteArrayOf(0xFF.toByte(), 0xD8.toByte(), 0xFF.toByte(), 0xE0.toByte(), 0, 0x10, 0x4A, 0x46, 0x49, 0x46)))
        // ISO-BMFF but not AVIF (e.g. mp4 brand)
        assertFalse(MediaFormats.isAvif(isoBmffBytes("isom")))
    }

    @Test
    fun `capability preference is avif on api 30 plus and webp below`() {
        assertEquals("image/webp", MediaFormats.preferredImageMime(26))
        assertEquals("image/webp", MediaFormats.preferredImageMime(29))
        assertEquals("image/avif", MediaFormats.preferredImageMime(30))
        assertEquals("image/avif", MediaFormats.preferredImageMime(37))
    }

    @Test
    fun `extensions and filenames follow the real encoded mime`() {
        assertEquals("avif", MediaFormats.extensionFor("image/avif"))
        assertEquals("webp", MediaFormats.extensionFor("image/webp"))
        assertEquals("jpg", MediaFormats.extensionFor("image/jpeg"))
        assertEquals("jpg", MediaFormats.extensionFor("image/jpg"))
        assertEquals("img", MediaFormats.extensionFor("image/png"))
        assertEquals("image-tiny.avif", MediaFormats.fileNameForTier("tiny", "image/avif"))
        assertEquals("image-high.webp", MediaFormats.fileNameForTier("high", "image/webp"))
        assertEquals("image-medium.jpg", MediaFormats.fileNameForTier("medium", "image/jpeg"))
    }

    @Test
    fun `preset quality and speed map to the encoder bytes`() {
        assertEquals(28, MediaFormats.avifQuality(0.28))
        assertEquals(40, MediaFormats.avifQuality(0.40))
        assertEquals(50, MediaFormats.avifQuality(0.50))
        assertEquals(1, MediaFormats.avifQuality(0.0))
        assertEquals(100, MediaFormats.avifQuality(1.0))
        assertEquals(10, MediaFormats.avifSpeedForTier("tiny"))
        assertEquals(8, MediaFormats.avifSpeedForTier("medium"))
        assertEquals(8, MediaFormats.avifSpeedForTier("high"))
        assertEquals(8, MediaFormats.avifSpeedForTier("unknown"))
    }

    @Test
    fun `calibrated avif quality matches webp perceptually at 1_25x`() {
        assertEquals(35, MediaFormats.calibratedAvifQuality(28))
        assertEquals(50, MediaFormats.calibratedAvifQuality(40))
        assertEquals(62, MediaFormats.calibratedAvifQuality(50))
        assertEquals(100, MediaFormats.calibratedAvifQuality(100))
        assertEquals(1, MediaFormats.calibratedAvifQuality(0))
    }

    @Test
    fun `smaller output wins and webp wins ties`() {
        assertEquals(MediaFormats.ImageFormatChoice.AVIF, MediaFormats.chooseImageFormat(1_900, 2_800))
        assertEquals(MediaFormats.ImageFormatChoice.WEBP, MediaFormats.chooseImageFormat(2_900, 2_800))
        assertEquals(MediaFormats.ImageFormatChoice.WEBP, MediaFormats.chooseImageFormat(2_800, 2_800))
        assertEquals(MediaFormats.ImageFormatChoice.WEBP, MediaFormats.chooseImageFormat(0, 2_800))
    }

    @Test
    fun `labels match the existing bubble vocabulary`() {
        assertEquals("AVIF", MediaFormats.labelFor("image/avif"))
        assertEquals("WebP", MediaFormats.labelFor("image/webp"))
        assertEquals("JPEG", MediaFormats.labelFor("image/jpeg"))
        assertEquals("PNG", MediaFormats.labelFor("image/png"))
        assertEquals("Image", MediaFormats.labelFor("image/x-unknown"))
    }
}
