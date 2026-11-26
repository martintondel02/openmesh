package org.openmesh.android.media

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.media.MediaPlayer
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

/**
 * Plays an attachment: Codec2 payloads are decoded through the FFI to 8 kHz mono s16le PCM and
 * played with AudioTrack; Opus/OGG and AAC/MP4 use the platform MediaPlayer.
 */
class AttachmentPlayer(
    private val context: Context,
    private val codec: VoiceCodec = FfiVoiceCodec(),
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var mediaPlayer: MediaPlayer? = null
    private var audioTrack: AudioTrack? = null
    private var job: Job? = null

    fun play(file: File, mime: String, onError: (String) -> Unit = {}) {
        stop()
        when (VoicePlaybackPolicy.kindFor(mime)) {
            PlaybackKind.PLATFORM_MEDIA -> playPlatform(file, onError)
            PlaybackKind.CODEC2_PCM -> playCodec2(file, mime, onError)
        }
    }

    private fun playPlatform(file: File, onError: (String) -> Unit) {
        runCatching {
            mediaPlayer = MediaPlayer().apply {
                setAudioAttributes(
                    AudioAttributes.Builder()
                        .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                        .setUsage(AudioAttributes.USAGE_MEDIA)
                        .build(),
                )
                setDataSource(file.absolutePath)
                setOnCompletionListener { stop() }
                prepare()
                start()
            }
        }.onFailure { onError(it.message ?: "Playback failed") }
    }

    private fun playCodec2(file: File, mime: String, onError: (String) -> Unit) {
        job = scope.launch {
            runCatching {
                val tier = VoicePlaybackPolicy.tierForMime(mime)
                val payload = file.readBytes()
                val pcm = codec.decode(tier, payload)
                withContext(Dispatchers.Main) { playPcm(pcm) }
            }.onFailure { onError(it.message ?: "Codec2 playback failed") }
        }
    }

    private fun playPcm(pcm: ByteArray) {
        val track = AudioTrack(
            AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                .build(),
            AudioFormat.Builder()
                .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                .setSampleRate(8_000)
                .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                .build(),
            pcm.size.coerceAtLeast(1_024),
            AudioTrack.MODE_STREAM,
            AudioManager.AUDIO_SESSION_ID_GENERATE,
        )
        audioTrack = track
        track.play()
        var offset = 0
        while (offset < pcm.size) {
            val written = track.write(pcm, offset, minOf(4_096, pcm.size - offset))
            if (written <= 0) break
            offset += written
        }
        track.stop()
        runCatching { track.release() }
        audioTrack = null
    }

    fun stop() {
        job?.cancel()
        job = null
        runCatching { mediaPlayer?.stop() }
        runCatching { mediaPlayer?.release() }
        mediaPlayer = null
        runCatching { audioTrack?.pause() }
        runCatching { audioTrack?.flush() }
        runCatching { audioTrack?.release() }
        audioTrack = null
    }
}
