package org.openmesh.android.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MediaMathTest {
    @Test
    fun `attachment caps follow the protocol profile table`() {
        assertEquals(24 * 1024, MediaMath.capBytes("SHORT_TURBO"))
        assertEquals(24 * 1024, MediaMath.capBytes("SHORT_FAST"))
        assertEquals(12 * 1024, MediaMath.capBytes("MEDIUM_FAST"))
        assertEquals(6 * 1024, MediaMath.capBytes("LONG_FAST"))
        assertEquals(1536, MediaMath.capBytes("LONG_SLOW"))
        assertEquals(12 * 1024, MediaMath.capBytes("UNKNOWN"))
    }

    @Test
    fun `voice duration caps match the profile table`() {
        assertEquals(15, MediaMath.maxVoiceSeconds("SHORT_TURBO"))
        assertEquals(15, MediaMath.maxVoiceSeconds("SHORT_FAST"))
        assertEquals(10, MediaMath.maxVoiceSeconds("MEDIUM_FAST"))
        assertEquals(8, MediaMath.maxVoiceSeconds("LONG_FAST"))
        assertEquals(4, MediaMath.maxVoiceSeconds("LONG_SLOW"))
    }

    @Test
    fun `downscale never upscales and preserves aspect ratio`() {
        assertEquals(100 to 50, MediaMath.scale(100, 50, 512))
        val (w, h) = MediaMath.scale(4000, 3000, 1024)
        assertEquals(1024, w)
        assertTrue(h in 767..769)
    }

    @Test
    fun `max edge grows with the attachment cap`() {
        val edges = listOf(1536, 6 * 1024, 12 * 1024, 24 * 1024).map(MediaMath::maxEdgeForCap)
        assertEquals(edges.sorted(), edges)
        assertTrue(edges.first() < edges.last())
    }

    @Test
    fun `decoder sample size halves until near the target edge`() {
        assertEquals(1, ImageCompressor.sampleSize(1000, 800, 1024))
        assertEquals(2, ImageCompressor.sampleSize(4000, 3000, 1024))
        assertTrue(ImageCompressor.sampleSize(8000, 6000, 512) >= 4)
    }

    @Test
    fun `voice waveform normalisation is bounded and stable`() {
        assertEquals(emptyList<Float>(), VoicePlayer.normalize(emptyList()))
        val normalised = VoicePlayer.normalize(listOf(0, 50, 100))
        assertEquals(1f, normalised.max(), 0.0001f)
        assertTrue(normalised[0] == 0f)
    }
}
