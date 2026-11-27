package org.openmesh.android.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * JVM tests for the pure attachment presentation policy: MIME classification, Codec2 duration
 * estimation, downsampling, view-state labels and fallback copy.
 */
class AttachmentUiPolicyTest {

    // ------------------------------------------------------------ classification

    @Test
    fun `mime classification covers images voice and other files`() {
        assertEquals(AttachmentViewKind.IMAGE, AttachmentUiPolicy.kindFor("image/webp"))
        assertEquals(AttachmentViewKind.IMAGE, AttachmentUiPolicy.kindFor("image/jpeg"))
        assertEquals(AttachmentViewKind.IMAGE, AttachmentUiPolicy.kindFor("IMAGE/PNG"))
        assertEquals(AttachmentViewKind.VOICE, AttachmentUiPolicy.kindFor("audio/c2-1300"))
        assertEquals(AttachmentViewKind.VOICE, AttachmentUiPolicy.kindFor("audio/ogg; codecs=opus"))
        assertEquals(AttachmentViewKind.VOICE, AttachmentUiPolicy.kindFor("audio/mp4"))
        assertEquals(AttachmentViewKind.FILE, AttachmentUiPolicy.kindFor("application/cbor"))
        assertEquals(AttachmentViewKind.FILE, AttachmentUiPolicy.kindFor(""))
    }

    @Test
    fun `codec2 detection is limited to raw codec2 mimes`() {
        assertTrue(AttachmentUiPolicy.isCodec2("audio/c2-1300"))
        assertTrue(AttachmentUiPolicy.isCodec2("audio/c2-3200"))
        assertFalse(AttachmentUiPolicy.isCodec2("audio/ogg"))
        assertFalse(AttachmentUiPolicy.isCodec2("audio/mp4"))
    }

    // ------------------------------------------------------------ duration

    @Test
    fun `codec2 duration is estimated from frame size`() {
        // 308 bytes / 7 bytes per 40 ms frame = 44 frames = 1760 ms (the bridge fixture).
        assertEquals(1_760L, AttachmentUiPolicy.estimateCodec2DurationMs("audio/c2-1300", 308))
        // 8 bytes per 20 ms frame for the 3.2k mode.
        assertEquals(1_000L, AttachmentUiPolicy.estimateCodec2DurationMs("audio/c2-3200", 400))
        assertNull(AttachmentUiPolicy.estimateCodec2DurationMs("audio/ogg", 308))
        assertNull(AttachmentUiPolicy.estimateCodec2DurationMs("audio/c2-1300", 0))
    }

    @Test
    fun `metadata duration wins over the codec2 estimate`() {
        assertEquals(1_949, AttachmentUiPolicy.effectiveDurationMs("audio/c2-1300", 1_949, 308))
        assertEquals(1_760, AttachmentUiPolicy.effectiveDurationMs("audio/c2-1300", null, 308))
        assertNull(AttachmentUiPolicy.effectiveDurationMs("application/cbor", null, 260))
    }

    @Test
    fun `voice label shows duration and size like the media tier label`() {
        val label = AttachmentUiPolicy.attachmentLabel("audio/c2-1300", null, 308)
        assertTrue("unexpected label: $label", label.contains("Codec2 1.3k"))
        assertTrue("unexpected label: $label", label.contains("2 s"))
        assertTrue("unexpected label: $label", label.contains("308 B"))

        val image = AttachmentUiPolicy.attachmentLabel("image/webp", null, 6018)
        assertTrue("unexpected label: $image", image.contains("WebP"))
        assertTrue("unexpected label: $image", image.contains("5.9 KiB"))
    }

    // ------------------------------------------------------------ downsampling

    @Test
    fun `inSampleSize keeps the decoded edge near the requested maximum`() {
        assertEquals(1, AttachmentUiPolicy.inSampleSize(240, 320, 1280))
        assertEquals(1, AttachmentUiPolicy.inSampleSize(1280, 1280, 1280))
        assertEquals(2, AttachmentUiPolicy.inSampleSize(4000, 3000, 1280))
        assertEquals(4, AttachmentUiPolicy.inSampleSize(8000, 6000, 1280))
        assertEquals(1, AttachmentUiPolicy.inSampleSize(0, 100, 1280))
        assertEquals(1, AttachmentUiPolicy.inSampleSize(100, 100, 0))
    }

    // ------------------------------------------------------------ view state copy

    @Test
    fun `voice state text is explicit for every state`() {
        assertEquals("Tap to play", AttachmentUiPolicy.voiceStateText(VoicePlaybackUiState.IDLE))
        assertTrue(AttachmentUiPolicy.voiceStateText(VoicePlaybackUiState.LOADING).contains("Preparing"))
        assertTrue(AttachmentUiPolicy.voiceStateText(VoicePlaybackUiState.PLAYING).contains("Playing"))
        assertEquals(
            "The radio is busy",
            AttachmentUiPolicy.voiceStateText(VoicePlaybackUiState.ERROR, "The radio is busy"),
        )
        assertTrue(
            AttachmentUiPolicy.voiceStateText(VoicePlaybackUiState.ERROR, null).contains("could not be played"),
        )
    }

    @Test
    fun `fallback copy offers a next step and never blames the user`() {
        val missingImage = AttachmentUiPolicy.imageFallbackText(fileExists = false)
        val brokenImage = AttachmentUiPolicy.imageFallbackText(fileExists = true)
        val missingVoice = AttachmentUiPolicy.voiceFallbackText(fileExists = false)
        for (text in listOf(missingImage, brokenImage, missingVoice)) {
            assertTrue(text.isNotBlank())
            assertFalse(text.contains("invalid", ignoreCase = true))
            assertFalse(text.contains("you failed", ignoreCase = true))
        }
        assertTrue(missingImage.contains("not on this phone"))
        assertTrue(brokenImage.contains("could not be opened"))
        assertTrue(missingVoice.contains("not on this phone"))
    }

    @Test
    fun `display name prefers a real filename over the attachment id`() {
        assertEquals("voice-1s.c2", AttachmentUiPolicy.displayName("voice-1s.c2", "audio/c2-1300", AttachmentViewKind.VOICE))
        assertEquals("Photo", AttachmentUiPolicy.displayName(null, "image/jpeg", AttachmentViewKind.IMAGE))
        assertEquals("Photo (WebP)", AttachmentUiPolicy.displayName("att_3341274591", "image/webp", AttachmentViewKind.IMAGE))
        assertEquals("Voice note", AttachmentUiPolicy.displayName("att_-1", "audio/c2-1300", AttachmentViewKind.VOICE))
        assertEquals("Attachment", AttachmentUiPolicy.displayName(" ", "application/cbor", AttachmentViewKind.FILE))
    }

    @Test
    fun `decode limits are sane`() {
        assertEquals(1280, AttachmentUiPolicy.IMAGE_MAX_EDGE_PX)
        assertTrue(AttachmentUiPolicy.THUMBNAIL_MAX_HEIGHT_DP in 120..320)
    }
}
