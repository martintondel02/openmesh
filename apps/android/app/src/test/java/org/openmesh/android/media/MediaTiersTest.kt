package org.openmesh.android.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.openmesh.android.data.model.AppSettings

/** Parity and policy tests for v0.6.0 media tiers (content/presets/media.json is the source). */
class MediaTiersTest {
    private val presets = MediaTierLoader.fromRepoFile()

    @Test
    fun `preset file parses with schema version and defaults`() {
        assertEquals(1, presets.version)
        assertEquals("medium", presets.defaults.image)
        assertEquals("tiny", presets.defaults.voice)
        assertTrue(presets.airtimeNote.contains("measured bytes"))
    }

    @Test
    fun `image tiers match the shared asset exactly`() {
        assertEquals(listOf("tiny", "medium", "high", "original"), presets.imageOrder)
        assertEquals(256, presets.images.getValue("tiny").maxDim)
        assertEquals(0.28, presets.images.getValue("tiny").quality!!, 0.0001)
        assertEquals(512, presets.images.getValue("medium").maxDim)
        assertEquals(0.40, presets.images.getValue("medium").quality!!, 0.0001)
        assertEquals(1024, presets.images.getValue("high").maxDim)
        assertEquals(0.50, presets.images.getValue("high").quality!!, 0.0001)
        assertEquals("image/avif", presets.images.getValue("tiny").mime)
        assertEquals("image/avif", presets.images.getValue("medium").mime)
        assertEquals("image/avif", presets.images.getValue("high").mime)
        assertEquals("original", presets.images.getValue("original").mime)
        assertEquals(null, presets.images.getValue("original").maxDim)
    }

    @Test
    fun `voice tiers match the shared asset exactly`() {
        val tiny = presets.voice.getValue("tiny")
        assertEquals("codec2", tiny.codec)
        assertEquals("1300", tiny.mode)
        assertEquals("audio/c2-1300", tiny.mime)
        assertEquals(8_000, tiny.sampleRate)
        assertEquals(40, tiny.frameMs)
        assertEquals(1_300, tiny.bitsPerSecond)
        assertEquals(30, tiny.maxDurationS)

        val medium = presets.voice.getValue("medium")
        assertEquals("3200", medium.mode)
        assertEquals("audio/c2-3200", medium.mime)
        assertEquals(20, medium.frameMs)
        assertEquals(3_200, medium.bitsPerSecond)
        assertEquals(20, medium.maxDurationS)

        val high = presets.voice.getValue("high")
        assertEquals("opus", high.codec)
        assertEquals("audio/ogg; codecs=opus", high.mime)
        assertEquals(12_000, high.bitsPerSecond)
        assertEquals(15, high.maxDurationS)
        assertEquals(listOf("tiny", "medium", "high"), presets.voiceOrder)
    }

    @Test
    fun `auto-suggest picks the largest tier under the profile cap`() {
        assertEquals("tiny", MediaTierPolicy.suggestImageTier(presets, 2 * 1024))
        assertEquals("medium", MediaTierPolicy.suggestImageTier(presets, 14 * 1024))
        assertEquals("high", MediaTierPolicy.suggestImageTier(presets, 100 * 1024))
        // Never suggests "original".
        assertTrue(MediaTierPolicy.suggestImageTier(presets, Int.MAX_VALUE) != "original")
    }

    @Test
    fun `original is disabled on long-range profiles`() {
        assertFalse(MediaTierPolicy.originalAllowed("LONG_SLOW"))
        assertFalse(MediaTierPolicy.originalAllowed("LONG_FAST"))
        assertTrue(MediaTierPolicy.originalAllowed("MEDIUM_FAST"))
        assertTrue(MediaTierPolicy.originalAllowed("SHORT_FAST"))
    }

    @Test
    fun `bytes per second matches the bitrates`() {
        assertEquals(175, MediaTierPolicy.voiceBytesPerSecond(presets.voice.getValue("tiny")))
        assertEquals(400, MediaTierPolicy.voiceBytesPerSecond(presets.voice.getValue("medium")))
        assertEquals(1_500, MediaTierPolicy.voiceBytesPerSecond(presets.voice.getValue("high")))
    }

    @Test
    fun `effective voice cap is min of tier max and profile cap`() {
        val tiny = presets.voice.getValue("tiny")
        val medium = presets.voice.getValue("medium")
        val high = presets.voice.getValue("high")
        assertEquals(30, MediaTierPolicy.effectiveVoiceMaxSeconds(tiny, 12 * 1024))
        assertEquals(11, MediaTierPolicy.effectiveVoiceMaxSeconds(tiny, 2 * 1024))
        assertEquals(20, MediaTierPolicy.effectiveVoiceMaxSeconds(medium, 12 * 1024))
        assertEquals(15, MediaTierPolicy.effectiveVoiceMaxSeconds(medium, 6 * 1024))
        assertEquals(0, MediaTierPolicy.effectiveVoiceMaxSeconds(high, 100))
    }

    @Test
    fun `mime mapping and codec detection`() {
        assertEquals("audio/c2-1300", MediaTierPolicy.voiceMime("tiny", presets))
        assertEquals("audio/c2-3200", MediaTierPolicy.voiceMime("medium", presets))
        assertTrue(MediaTierPolicy.voiceIsCodec2("audio/c2-1300"))
        assertTrue(MediaTierPolicy.voiceIsCodec2("audio/c2-3200"))
        assertFalse(MediaTierPolicy.voiceIsCodec2("audio/ogg; codecs=opus"))
        assertFalse(MediaTierPolicy.voiceIsCodec2("audio/mp4"))
    }

    @Test
    fun `playback dispatch picks codec2 decode or platform media`() {
        assertEquals(PlaybackKind.CODEC2_PCM, VoicePlaybackPolicy.kindFor("audio/c2-1300"))
        assertEquals(PlaybackKind.CODEC2_PCM, VoicePlaybackPolicy.kindFor("audio/c2-3200"))
        assertEquals(PlaybackKind.PLATFORM_MEDIA, VoicePlaybackPolicy.kindFor("audio/ogg; codecs=opus"))
        assertEquals(PlaybackKind.PLATFORM_MEDIA, VoicePlaybackPolicy.kindFor("audio/mp4"))
        assertEquals("tiny", VoicePlaybackPolicy.tierForMime("audio/c2-1300"))
        assertEquals("medium", VoicePlaybackPolicy.tierForMime("audio/c2-3200"))
        assertEquals("high", VoicePlaybackPolicy.tierForMime("audio/ogg"))
    }

    @Test
    fun `only tiny and medium are ffi encodable`() {
        assertTrue(VoicePlaybackPolicy.isFfiEncodable("tiny"))
        assertTrue(VoicePlaybackPolicy.isFfiEncodable("medium"))
        assertFalse(VoicePlaybackPolicy.isFfiEncodable("high"))
    }

    @Test
    fun `tier lookup falls back to the media defaults`() {
        assertEquals(presets.images.getValue("medium"), MediaTierPolicy.imageTier(presets, "unknown"))
        assertEquals(presets.voice.getValue("tiny"), MediaTierPolicy.voiceTier(presets, "unknown"))
        assertEquals(presets.voice.getValue("high"), MediaTierPolicy.voiceTier(presets, "high"))
    }

    @Test
    fun `app settings defaults match media presets`() {
        val settings = AppSettings()
        assertEquals(presets.defaults.image, settings.defaultImageTier)
        assertEquals(presets.defaults.voice, settings.defaultVoiceTier)
    }

    @Test
    fun `attachment labels show format duration and size`() {
        assertEquals("Codec2 1.3k · 8 s · 1.3 KiB", MediaTierPolicy.attachmentLabel("audio/c2-1300", 8_000, 1_300))
        assertEquals("Codec2 3.2k · 5 s · 2.0 KiB", MediaTierPolicy.attachmentLabel("audio/c2-3200", 5_000, 2_048))
        assertEquals("Opus · 8 s · 7.0 KiB", MediaTierPolicy.attachmentLabel("audio/ogg; codecs=opus", 8_000, 7_168))
        assertEquals("AAC · 3 s · 1.0 KiB", MediaTierPolicy.attachmentLabel("audio/mp4", 3_000, 1_024))
        assertEquals("WebP · 512 B", MediaTierPolicy.attachmentLabel("image/webp", null, 512))
        assertEquals("AVIF · 2.0 KiB", MediaTierPolicy.attachmentLabel("image/avif", null, 2_048))
        assertEquals("JPEG · 2.0 KiB", MediaTierPolicy.attachmentLabel("image/jpeg", null, 2_048))
    }

    @Test
    fun `voice codec wrapper maps FFI errors to VoiceCodecException`() {
        val codec = FfiVoiceCodec()
        val error = assertThrows(VoiceCodecException::class.java) {
            codec.encode("tiny", ByteArray(0))
        }
        assertNotNull(error.message)
        assertTrue(error.message!!.isNotBlank())
    }

    @Test
    fun `image tier estimates are ordered and never upscale the original`() {
        val tiny = MediaTierPolicy.estimateImageBytes(presets.images.getValue("tiny"))
        val medium = MediaTierPolicy.estimateImageBytes(presets.images.getValue("medium"))
        val high = MediaTierPolicy.estimateImageBytes(presets.images.getValue("high"))
        assertTrue(tiny < medium && medium < high)
        assertEquals(Long.MAX_VALUE, MediaTierPolicy.estimateImageBytes(presets.images.getValue("original")))
    }
}
