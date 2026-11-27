package org.openmesh.android.media

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.media.MediaPlayer
import android.os.Handler
import android.os.Looper
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import java.io.File
import java.util.concurrent.atomic.AtomicLong

/**
 * Plays an attachment: Codec2 payloads are decoded through the FFI to 8 kHz mono s16le PCM and
 * played with AudioTrack; Opus/OGG and AAC/MP4 use the platform MediaPlayer.
 *
 * Callbacks are delivered on the main thread. [onStarted] fires when audio actually begins,
 * [onDone] when it finishes naturally (not after [stop]), [onError] with beginner-readable text.
 * Codec2 decode + playback run off the main thread so a multi-second note never blocks the UI.
 */
class AttachmentPlayer(
    private val context: Context,
    private val codec: VoiceCodec = FfiVoiceCodec(),
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val mainHandler = Handler(Looper.getMainLooper())
    private val generation = AtomicLong(0)

    @Volatile private var mediaPlayer: MediaPlayer? = null
    @Volatile private var audioTrack: AudioTrack? = null
    @Volatile private var job: Job? = null

    fun play(
        file: File,
        mime: String,
        onStarted: () -> Unit = {},
        onDone: () -> Unit = {},
        onError: (String) -> Unit = {},
    ) {
        stop()
        val token = generation.incrementAndGet()
        when (VoicePlaybackPolicy.kindFor(mime)) {
            PlaybackKind.PLATFORM_MEDIA -> playPlatform(token, file, onStarted, onDone, onError)
            PlaybackKind.CODEC2_PCM -> playCodec2(token, file, mime, onStarted, onDone, onError)
        }
    }

    private fun playPlatform(
        token: Long,
        file: File,
        onStarted: () -> Unit,
        onDone: () -> Unit,
        onError: (String) -> Unit,
    ) {
        runCatching {
            val player = MediaPlayer().apply {
                setAudioAttributes(
                    AudioAttributes.Builder()
                        .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                        .setUsage(AudioAttributes.USAGE_MEDIA)
                        .build(),
                )
                setDataSource(file.absolutePath)
                setOnCompletionListener {
                    if (token != generation.get()) return@setOnCompletionListener
                    stop()
                    onDone()
                }
                setOnErrorListener { _, what, extra ->
                    stop()
                    onError("The audio player reported an error ($what/$extra).")
                    true
                }
                prepare()
                start()
            }
            mediaPlayer = player
            onStarted()
        }.onFailure {
            runCatching { mediaPlayer?.release() }
            mediaPlayer = null
            onError(it.message ?: "Playback failed")
        }
    }

    private fun playCodec2(
        token: Long,
        file: File,
        mime: String,
        onStarted: () -> Unit,
        onDone: () -> Unit,
        onError: (String) -> Unit,
    ) {
        job = scope.launch {
            runCatching {
                val tier = VoicePlaybackPolicy.tierForMime(mime)
                val payload = file.readBytes()
                val pcm = codec.decode(tier, payload)
                if (token != generation.get()) return@launch
                mainHandler.post {
                    if (token == generation.get()) onStarted()
                }
                playPcm(pcm)
            }.onFailure {
                if (token == generation.get()) {
                    mainHandler.post { onError(it.message ?: "Codec2 playback failed") }
                }
            }.onSuccess {
                if (token == generation.get()) {
                    mainHandler.post {
                        if (token == generation.get()) onDone()
                    }
                }
            }
        }
    }

    /** Blocking AudioTrack write loop; called on [Dispatchers.IO] so the UI thread stays free. */
    private fun playPcm(pcm: ByteArray) {
        // A small streaming buffer makes write() block until audio is consumed. Passing the whole
        // PCM length as buffer size made every write return immediately and stop() cut the note off.
        val minBuffer = AudioTrack.getMinBufferSize(
            8_000,
            AudioFormat.CHANNEL_OUT_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
        )
        val bufferSize = if (minBuffer > 0) minBuffer.coerceAtLeast(4_096) else 4_096
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
            bufferSize,
            AudioTrack.MODE_STREAM,
            AudioManager.AUDIO_SESSION_ID_GENERATE,
        )
        audioTrack = track
        try {
            track.play()
            var offset = 0
            while (offset < pcm.size) {
                val written = track.write(pcm, offset, minOf(4_096, pcm.size - offset))
                if (written <= 0) break
                offset += written
            }
            track.stop()
        } finally {
            runCatching { track.release() }
            if (audioTrack === track) audioTrack = null
        }
    }

    fun stop() {
        generation.incrementAndGet()
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
