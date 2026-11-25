package org.openmesh.android

import org.junit.Assert.assertEquals
import org.junit.Test
import org.openmesh.android.util.Crc16

class Crc16Test {

    /** CRC-16/CCITT-FALSE has the published check value 0x29B1 for "123456789". */
    @Test
    fun `matches the published check value`() {
        val bytes = "123456789".toByteArray(Charsets.US_ASCII)
        assertEquals(0x29B1, Crc16.compute(bytes))
    }

    @Test
    fun `subrange computation is independent of prefix`() {
        val a = byteArrayOf(0, 1, 2, 3, 4, 5)
        val full = Crc16.compute(a, 0, a.size)
        val sub = Crc16.compute(a, 2, 4)
        assertEquals(full, sub)
    }
}
