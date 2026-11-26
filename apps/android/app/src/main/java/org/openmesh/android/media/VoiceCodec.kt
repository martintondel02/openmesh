package org.openmesh.android.media

import uniffi.openmesh_ffi.FfiException
import uniffi.openmesh_ffi.openmeshVoiceDecode
import uniffi.openmesh_ffi.openmeshVoiceEncode
import uniffi.openmesh_ffi.openmeshVoiceTiersJson

/**
 * Thin wrapper over the Rust Codec2 FFI (`crates/openmesh-ffi`, generated package
 * `uniffi.openmesh_ffi`): top-level `openmeshVoiceEncode/Decode/TiersJson`.
 *
 * PCM is 8 kHz mono s16le little-endian. Tiny = Codec2 1300 (7 B/40 ms), medium = Codec2 3200
 * (8 B/20 ms); high is rejected by the FFI (the platform Opus/AAC recorder handles it).
 */
interface VoiceCodec {
    fun encode(tier: String, pcmS16le: ByteArray): ByteArray
    fun decode(tier: String, payload: ByteArray): ByteArray
    fun tiersJson(): String
}

class VoiceCodecException(message: String, cause: Throwable? = null) : Exception(message, cause)

class FfiVoiceCodec : VoiceCodec {
    override fun encode(tier: String, pcmS16le: ByteArray): ByteArray =
        try {
            openmeshVoiceEncode(tier, pcmS16le)
        } catch (e: FfiException) {
            throw VoiceCodecException(e.message ?: "Codec2 encode failed", e)
        } catch (t: Throwable) {
            // JNA library loading failures are Errors, not Exceptions; never crash the UI.
            throw VoiceCodecException(t.message ?: "Codec2 encode failed", t)
        }

    override fun decode(tier: String, payload: ByteArray): ByteArray =
        try {
            openmeshVoiceDecode(tier, payload)
        } catch (e: FfiException) {
            throw VoiceCodecException(e.message ?: "Codec2 decode failed", e)
        } catch (t: Throwable) {
            throw VoiceCodecException(t.message ?: "Codec2 decode failed", t)
        }

    override fun tiersJson(): String = try {
        openmeshVoiceTiersJson()
    } catch (t: Throwable) {
        throw VoiceCodecException(t.message ?: "voice tier metadata failed", t)
    }
}

/** Playback strategy for an attachment MIME (unit-tested dispatch). */
enum class PlaybackKind { CODEC2_PCM, PLATFORM_MEDIA }

object VoicePlaybackPolicy {
    fun kindFor(mime: String): PlaybackKind =
        if (mime.startsWith("audio/c2-1300") || mime.startsWith("audio/c2-3200")) {
            PlaybackKind.CODEC2_PCM
        } else {
            PlaybackKind.PLATFORM_MEDIA
        }

    /** Tiny/medium Codec2 tiers are the only ones the FFI encoder handles. */
    fun isFfiEncodable(tierId: String): Boolean = tierId.lowercase() in setOf("tiny", "medium")

    fun tierForMime(mime: String): String = when {
        mime.startsWith("audio/c2-1300") -> "tiny"
        mime.startsWith("audio/c2-3200") -> "medium"
        else -> "high"
    }
}
