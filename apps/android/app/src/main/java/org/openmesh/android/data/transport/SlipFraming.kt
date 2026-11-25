package org.openmesh.android.data.transport

import org.openmesh.android.util.Crc16

/**
 * Companion API framing: SLIP delimiters around `OM | type | len(LE) | payload | crc16(LE)`.
 *
 * See docs/architecture/COMPANION-API.md §1. SLIP is used because Android USB-serial reads
 * are a byte stream, and the same framing is used on BLE where notifications split frames.
 */
object SlipFraming {
    const val END = 0xC0
    const val ESC = 0xDB
    const val ESC_END = 0xDC
    const val ESC_ESC = 0xDD

    const val MAGIC_0 = 'O'.code.toByte()
    const val MAGIC_1 = 'M'.code.toByte()
    const val HEADER_LEN = 5 // OM + type + u16 len
    const val MAX_PAYLOAD = 1024
    const val CRC_LEN = 2

    /** Add SLIP delimiters and byte stuffing to a complete companion frame. */
    fun encode(frame: ByteArray): ByteArray {
        val out = ArrayList<Byte>(frame.size + 8)
        out.add(END.toByte())
        for (b in frame) {
            when (b.toInt() and 0xFF) {
                END -> { out.add(ESC.toByte()); out.add(ESC_END.toByte()) }
                ESC -> { out.add(ESC.toByte()); out.add(ESC_ESC.toByte()) }
                else -> out.add(b)
            }
        }
        out.add(END.toByte())
        return out.toByteArray()
    }

    /** Build a companion frame including magic, length and CRC-16/CCITT-FALSE (little-endian). */
    fun buildFrame(type: Int, payload: ByteArray): ByteArray {
        require(type in 1..13) { "unknown companion message type $type" }
        require(payload.size <= MAX_PAYLOAD) { "payload ${payload.size} exceeds $MAX_PAYLOAD" }
        val out = ByteArray(HEADER_LEN + payload.size + CRC_LEN)
        out[0] = MAGIC_0
        out[1] = MAGIC_1
        out[2] = type.toByte()
        out[3] = (payload.size and 0xFF).toByte()
        out[4] = ((payload.size ushr 8) and 0xFF).toByte()
        payload.copyInto(out, HEADER_LEN)
        val crc = Crc16.compute(out, 2, 3 + payload.size) // type + len + payload
        out[HEADER_LEN + payload.size] = (crc and 0xFF).toByte()
        out[HEADER_LEN + payload.size + 1] = ((crc ushr 8) and 0xFF).toByte()
        return out
    }

    /** A validated companion frame. */
    data class Frame(val type: Int, val payload: ByteArray)

    /** Parse a de-SLIPped frame; returns null for malformed, oversized or CRC-failing input. */
    fun parse(raw: ByteArray): Frame? {
        if (raw.size < HEADER_LEN + CRC_LEN) return null
        if (raw[0] != MAGIC_0 || raw[1] != MAGIC_1) return null
        val type = raw[2].toInt() and 0xFF
        if (type !in 1..13) return null
        val len = (raw[3].toInt() and 0xFF) or ((raw[4].toInt() and 0xFF) shl 8)
        if (len > MAX_PAYLOAD) return null
        if (raw.size != HEADER_LEN + len + CRC_LEN) return null
        val expected = (raw[HEADER_LEN + len].toInt() and 0xFF) or
            ((raw[HEADER_LEN + len + 1].toInt() and 0xFF) shl 8)
        if (Crc16.compute(raw, 2, 3 + len) != expected) return null
        return Frame(type, raw.copyOfRange(HEADER_LEN, HEADER_LEN + len))
    }

    /** Incremental SLIP decoder: feed arbitrary chunks, get complete de-stuffed frames. */
    class Decoder {
        private val buffer = ArrayList<Byte>(4096)
        private var inFrame = false
        private var escape = false

        fun feed(chunk: ByteArray): List<ByteArray> {
            val frames = ArrayList<ByteArray>()
            for (byte in chunk) {
                val value = byte.toInt() and 0xFF
                if (value == END) {
                    if (inFrame && buffer.isNotEmpty()) {
                        frames.add(buffer.toByteArray())
                    }
                    buffer.clear()
                    inFrame = true
                    escape = false
                    continue
                }
                if (!inFrame) continue
                if (escape) {
                    when (value) {
                        ESC_END -> buffer.add(END.toByte())
                        ESC_ESC -> buffer.add(ESC.toByte())
                        else -> { /* invalid escape: drop frame */ buffer.clear(); inFrame = false }
                    }
                    escape = false
                    continue
                }
                if (value == ESC) {
                    escape = true
                    continue
                }
                buffer.add(byte)
                if (buffer.size > MAX_PAYLOAD + HEADER_LEN + CRC_LEN + 64) {
                    // Runaway stream: reset rather than grow without bound.
                    buffer.clear()
                    inFrame = false
                }
            }
            return frames
        }
    }
}
