package org.openmesh.android.data.transport

import java.io.ByteArrayOutputStream

/**
 * Minimal CBOR (RFC 7049) encoder/decoder for the companion API's payloads.
 *
 * The companion API uses CBOR maps with text keys and simple values (unsigned ints, text,
 * byte strings, booleans, arrays). Pulling a full CBOR library into the app for ~13 message
 * types is not worth it; this codec deliberately supports exactly that subset and rejects
 * anything else. Malformed input must never throw out of the client — callers get null/errors.
 */
object CborCodec {

    sealed interface Value
    data class UInt(val value: Long) : Value
    data class NInt(val value: Long) : Value
    data class Bytes(val value: ByteArray) : Value
    data class Text(val value: String) : Value
    data class Bool(val value: Boolean) : Value
    data object Null : Value
    data class Array(val values: List<Value>) : Value
    data class Map(val entries: LinkedHashMap<String, Value>) : Value

    // ---------------------------------------------------------------- encoding

    fun encode(value: Value): ByteArray {
        val out = ByteArrayOutputStream()
        write(value, out)
        return out.toByteArray()
    }

    private fun write(value: Value, out: ByteArrayOutputStream) {
        when (value) {
            is UInt -> writeTypeAndValue(0, value.value, out)
            is NInt -> writeTypeAndValue(1, -1L - value.value, out)
            is Bytes -> {
                writeTypeAndValue(2, value.value.size.toLong(), out)
                out.write(value.value)
            }
            is Text -> {
                val bytes = value.value.toByteArray(Charsets.UTF_8)
                writeTypeAndValue(3, bytes.size.toLong(), out)
                out.write(bytes)
            }
            is Array -> {
                writeTypeAndValue(4, value.values.size.toLong(), out)
                value.values.forEach { write(it, out) }
            }
            is Map -> {
                writeTypeAndValue(5, value.entries.size.toLong(), out)
                value.entries.forEach { (k, v) ->
                    write(Text(k), out)
                    write(v, out)
                }
            }
            is Bool -> out.write(if (value.value) 0xF5 else 0xF4)
            Null -> out.write(0xF6)
        }
    }

    private fun writeTypeAndValue(major: Int, value: Long, out: ByteArrayOutputStream) {
        val base = major shl 5
        when {
            value < 24 -> out.write(base or value.toInt())
            value < 0x100 -> { out.write(base or 24); out.write(value.toInt()) }
            value < 0x10000 -> {
                out.write(base or 25)
                out.write((value ushr 8).toInt() and 0xFF)
                out.write(value.toInt() and 0xFF)
            }
            value < 0x1_0000_0000L -> {
                out.write(base or 26)
                for (shift in intArrayOf(24, 16, 8, 0)) out.write((value ushr shift).toInt() and 0xFF)
            }
            else -> {
                out.write(base or 27)
                for (shift in intArrayOf(56, 48, 40, 32, 24, 16, 8, 0)) out.write((value ushr shift).toInt() and 0xFF)
            }
        }
    }

    // ---------------------------------------------------------------- decoding

    class DecodeException(message: String) : IllegalArgumentException(message)

    fun decode(bytes: ByteArray): Value = Reader(bytes).readValue()

    /** Decode and require a map; convenience for companion messages. */
    fun decodeMap(bytes: ByteArray): LinkedHashMap<String, Value>? =
        (decode(bytes) as? Map)?.entries

    private class Reader(private val data: ByteArray) {
        private var pos = 0

        fun readValue(): Value {
            val initial = readByte()
            val major = initial ushr 5
            val minor = initial and 0x1F
            return when (major) {
                0 -> UInt(readLength(minor))
                1 -> NInt(-1L - readLength(minor))
                2 -> Bytes(readBytes(readLength(minor).toInt()))
                3 -> Text(readBytes(readLength(minor).toInt()).toString(Charsets.UTF_8))
                4 -> {
                    val count = readLength(minor).toInt()
                    if (count > 65_536) throw DecodeException("array too large")
                    Array(List(count) { readValue() })
                }
                5 -> {
                    val count = readLength(minor).toInt()
                    if (count > 65_536) throw DecodeException("map too large")
                    val map = LinkedHashMap<String, Value>(count)
                    repeat(count) {
                        val key = readValue() as? Text ?: throw DecodeException("non-text map key")
                        map[key.value] = readValue()
                    }
                    Map(map)
                }
                6 -> {
                    readLength(minor) // tag
                    readValue()
                }
                7 -> when (minor) {
                    20 -> Bool(false)
                    21 -> Bool(true)
                    22 -> Null
                    else -> throw DecodeException("unsupported simple value $minor")
                }
                else -> throw DecodeException("invalid major type $major")
            }
        }

        private fun readLength(minor: Int): Long = when {
            minor < 24 -> minor.toLong()
            minor == 24 -> readByte().toLong()
            minor == 25 -> ((readByte() shl 8) or readByte()).toLong()
            minor == 26 -> {
                var v = 0L
                repeat(4) { v = (v shl 8) or readByte().toLong() }
                v
            }
            minor == 27 -> {
                var v = 0L
                repeat(8) { v = (v shl 8) or readByte().toLong() }
                v
            }
            else -> throw DecodeException("indefinite-length items are not supported")
        }

        private fun readBytes(count: Int): ByteArray {
            if (count < 0 || pos + count > data.size) throw DecodeException("truncated byte string")
            val out = data.copyOfRange(pos, pos + count)
            pos += count
            return out
        }

        private fun readByte(): Int {
            if (pos >= data.size) throw DecodeException("unexpected end of CBOR")
            return data[pos++].toInt() and 0xFF
        }
    }
}
