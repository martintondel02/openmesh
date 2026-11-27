package org.openmesh.android.media

/**
 * Pure presentation helpers for received attachments (images, voice notes, other files).
 *
 * Android-free so the MIME/format/state decisions and view-state labels are unit-tested on the JVM
 * (`app/src/test/java/org/openmesh/android/media/AttachmentUiPolicyTest.kt`). The Compose layer only
 * decodes bitmaps and renders these strings.
 */
enum class AttachmentViewKind { IMAGE, VOICE, FILE }

/** Playback state shown on a voice-note bubble. */
enum class VoicePlaybackUiState { IDLE, LOADING, PLAYING, ERROR }

object AttachmentUiPolicy {
    /** Decoded images are downsampled to at most this edge; enough for a full-screen phone view. */
    const val IMAGE_MAX_EDGE_PX: Int = 1280

    /** Larger of width/height used by the bubble thumbnail; full-screen viewer reuses the decode. */
    const val THUMBNAIL_MAX_HEIGHT_DP: Int = 220

    fun kindFor(mime: String): AttachmentViewKind = when {
        isImage(mime) -> AttachmentViewKind.IMAGE
        isVoice(mime) -> AttachmentViewKind.VOICE
        else -> AttachmentViewKind.FILE
    }

    fun isImage(mime: String): Boolean = mime.startsWith("image/", ignoreCase = true)

    fun isVoice(mime: String): Boolean =
        mime.startsWith("audio/", ignoreCase = true)

    /** True when the payload is a raw Codec2 bitstream that must be decoded through the FFI. */
    fun isCodec2(mime: String): Boolean = mime.startsWith("audio/c2-", ignoreCase = true)

    /**
     * Codec2 carries no container duration: 1.3 k uses 7-byte/40 ms frames, 3.2 k 8-byte/20 ms
     * frames. Returns null for other formats (their player knows the real duration).
     */
    fun estimateCodec2DurationMs(mime: String, sizeBytes: Int): Long? {
        if (!isCodec2(mime) || sizeBytes <= 0) return null
        val (bytesPerFrame, frameMs) = if (mime.startsWith("audio/c2-3200")) 8 to 20L else 7 to 40L
        return (sizeBytes / bytesPerFrame).toLong() * frameMs
    }

    /** Prefer the duration carried in the message metadata, else a Codec2 estimate. */
    fun effectiveDurationMs(mime: String, durationMs: Int?, sizeBytes: Int): Int? =
        durationMs?.takeIf { it > 0 } ?: estimateCodec2DurationMs(mime, sizeBytes)?.toInt()

    /** Bubble subtitle, reusing the shared media-tier label ("Codec2 1.3k · 2 s · 308 B"). */
    fun attachmentLabel(mime: String, durationMs: Int?, sizeBytes: Int): String =
        MediaTierPolicy.attachmentLabel(mime, effectiveDurationMs(mime, durationMs, sizeBytes), sizeBytes)

    /**
     * inSampleSize for BitmapFactory: power of two that keeps the decoded edge at/above [maxEdge]
     * without going below it (standard Android downsampling rule).
     */
    fun inSampleSize(width: Int, height: Int, maxEdge: Int): Int {
        if (width <= 0 || height <= 0 || maxEdge <= 0) return 1
        var sample = 1
        while (maxOf(width, height) / sample > maxEdge * 2) {
            sample *= 2
        }
        return sample
    }

    /** Image bubble fallback copy; never a dead end (docs/design/ux-help-system.md §4.3). */
    fun imageFallbackText(fileExists: Boolean): String = if (fileExists) {
        "This image could not be opened."
    } else {
        "The image file is not on this phone yet. Ask the sender to send it again."
    }

    /** Voice-note fallback copy when the payload never reached this phone. */
    fun voiceFallbackText(fileExists: Boolean): String = if (fileExists) {
        "This voice note could not be played."
    } else {
        "The voice note file is not on this phone yet. Ask the sender to send it again."
    }

    /** Voice-note status line, including a visible loading/playing/error state. */
    fun voiceStateText(state: VoicePlaybackUiState, detail: String? = null): String = when (state) {
        VoicePlaybackUiState.IDLE -> "Tap to play"
        VoicePlaybackUiState.LOADING -> "Preparing voice note…"
        VoicePlaybackUiState.PLAYING -> "Playing…"
        VoicePlaybackUiState.ERROR -> detail?.takeIf { it.isNotBlank() } ?: "This voice note could not be played."
    }

    /** Human label for the attachment: filename when present, else a sensible default. */
    fun displayName(name: String?, mime: String, kind: AttachmentViewKind): String {
        val trimmed = name?.trim().orEmpty()
        if (trimmed.isNotEmpty() && !trimmed.matches(Regex("att_[-0-9]+"))) return trimmed
        return when (kind) {
            AttachmentViewKind.IMAGE -> if (mime.contains("webp")) "Photo (WebP)" else "Photo"
            AttachmentViewKind.VOICE -> "Voice note"
            AttachmentViewKind.FILE -> "Attachment"
        }
    }
}
