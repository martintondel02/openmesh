package org.openmesh.android.util

/** Hex helpers for node ids, channel ids and QR payloads. */
object Hex {
    private const val DIGITS = "0123456789abcdef"

    fun encode(bytes: ByteArray): String {
        val out = StringBuilder(bytes.size * 2)
        for (b in bytes) {
            val v = b.toInt() and 0xFF
            out.append(DIGITS[v ushr 4]).append(DIGITS[v and 0x0F])
        }
        return out.toString()
    }

    fun decodeOrNull(hex: String): ByteArray? {
        val clean = hex.trim().removePrefix("0x")
        if (clean.length % 2 != 0) return null
        val out = ByteArray(clean.length / 2)
        for (i in out.indices) {
            val hi = DIGITS.indexOf(clean[i * 2].lowercaseChar())
            val lo = DIGITS.indexOf(clean[i * 2 + 1].lowercaseChar())
            if (hi < 0 || lo < 0) return null
            out[i] = ((hi shl 4) or lo).toByte()
        }
        return out
    }

    /** Pretty short form used in lists: first 4 + last 4 hex characters. */
    fun short(nodeId: String): String =
        if (nodeId.length <= 10) nodeId else "${nodeId.take(4)}…${nodeId.takeLast(4)}"
}
