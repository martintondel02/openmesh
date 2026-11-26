package org.openmesh.android.media

import android.annotation.SuppressLint
import android.content.Context
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import java.io.ByteArrayOutputStream
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.max

/**
 * Raw PCM recorder for Codec2 voice tiers: 8 kHz mono s16le from the microphone. The thread
 * records until [stop] or the duration cap; samples are kept for the waveform preview.
 */
class PcmRecorder(private val context: Context) {
    private var record: AudioRecord? = null
    private var thread: Thread? = null
    private val running = AtomicBoolean(false)
    private val output = ByteArrayOutputStream()
    private val amplitudes = mutableListOf<Int>()
    private var startedAt = 0L
    private var maxBytes = Int.MAX_VALUE

    val sampleRate = 8_000

    @SuppressLint("MissingPermission") // RECORD_AUDIO is requested by the UI before recording.
    fun start(maxSeconds: Int) {
        val minBuffer = AudioRecord.getMinBufferSize(
            sampleRate,
            AudioFormat.CHANNEL_IN_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
        ).coerceAtLeast(4_096)
        val recorder = AudioRecord(
            MediaRecorder.AudioSource.MIC,
            sampleRate,
            AudioFormat.CHANNEL_IN_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
            max(minBuffer, sampleRate / 5), // ~200 ms
        )
        check(recorder.state == AudioRecord.STATE_INITIALIZED) { "Microphone is not available" }
        output.reset()
        amplitudes.clear()
        startedAt = System.currentTimeMillis()
        maxBytes = sampleRate * 2 * maxSeconds
        running.set(true)
        recorder.startRecording()
        record = recorder
        thread = Thread {
            val buffer = ByteArray(max(minBuffer, 3_200))
            while (running.get()) {
                val read = recorder.read(buffer, 0, buffer.size)
                if (read > 0) {
                    val room = maxBytes - output.size()
                    if (room <= 0) {
                        running.set(false)
                        break
                    }
                    output.write(buffer, 0, minOf(read, room))
                    amplitudes += peakAmplitude(buffer, minOf(read, room))
                }
            }
        }.apply { isDaemon = true; start() }
    }

    fun stop(): PcmRecording {
        running.set(false)
        thread?.join(500)
        thread = null
        val recorder = record
        record = null
        runCatching { recorder?.stop() }
        runCatching { recorder?.release() }
        val pcm = output.toByteArray()
        val durationMs = (System.currentTimeMillis() - startedAt).toInt()
        // Trim to whole 2-byte samples; the codec wrapper truncates to whole codec frames.
        val trimmed = if (pcm.size % 2 == 0) pcm else pcm.copyOf(pcm.size - 1)
        return PcmRecording(trimmed, durationMs, amplitudes.toList())
    }

    fun cancel() {
        running.set(false)
        thread?.join(500)
        thread = null
        runCatching { record?.stop() }
        runCatching { record?.release() }
        record = null
    }

    private fun peakAmplitude(buffer: ByteArray, length: Int): Int {
        var peak = 0
        var i = 0
        while (i + 1 < length) {
            val sample = ((buffer[i].toInt() and 0xFF) or (buffer[i + 1].toInt() shl 8)).toShort().toInt()
            peak = max(peak, kotlin.math.abs(sample))
            i += 2
        }
        return peak
    }

    data class PcmRecording(val pcm: ByteArray, val durationMs: Int, val waveform: List<Int>)
}
