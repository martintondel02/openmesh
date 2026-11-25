package org.openmesh.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.openmesh.android.data.transport.SlipFraming

class SlipFramingTest {

    @Test
    fun `frame round trips through slip with escaping`() {
        // Payload deliberately contains SLIP delimiter and escape bytes.
        val payload = byteArrayOf(0xC0.toByte(), 0xDB.toByte(), 0x01, 0xC0.toByte())
        val frame = SlipFraming.buildFrame(6, payload)
        val wire = SlipFraming.encode(frame)
        val decoded = SlipFraming.Decoder().feed(wire)
        assertEquals(1, decoded.size)
        val parsed = SlipFraming.parse(decoded.first())
        assertNotNull(parsed)
        assertEquals(6, parsed!!.type)
        assertTrue(payload.contentEquals(parsed.payload))
        assertTrue(wire.first() == 0xC0.toByte() && wire.last() == 0xC0.toByte())
    }

    @Test
    fun `crc failure is rejected`() {
        val frame = SlipFraming.buildFrame(1, byteArrayOf(1, 2, 3))
        frame[frame.size - 1] = (frame[frame.size - 1] + 1).toByte()
        assertNull(SlipFraming.parse(frame))
    }

    @Test
    fun `oversized and truncated frames are rejected`() {
        val ok = SlipFraming.buildFrame(12, byteArrayOf(9))
        assertNull(SlipFraming.parse(ok.copyOfRange(0, ok.size - 1)))
        assertNull(SlipFraming.parse(ByteArray(4)))
        // Corrupt the length field to exceed MAX_PAYLOAD.
        val bad = SlipFraming.buildFrame(12, ByteArray(4))
        bad[3] = 0x01
        bad[4] = 0x08 // 2049 > 1024
        assertNull(SlipFraming.parse(bad))
    }

    @Test
    fun `decoder handles arbitrary chunk boundaries`() {
        val payload = ByteArray(300) { (it and 0xFF).toByte() }
        val wire = SlipFraming.encode(SlipFraming.buildFrame(7, payload))
        val decoder = SlipFraming.Decoder()
        val frames = ArrayList<ByteArray>()
        var offset = 0
        for (chunk in intArrayOf(1, 2, 3, 7, 11, 100, 200)) {
            val end = minOf(offset + chunk, wire.size)
            frames += decoder.feed(wire.copyOfRange(offset, end))
            offset = end
            if (offset >= wire.size) break
        }
        if (offset < wire.size) frames += decoder.feed(wire.copyOfRange(offset, wire.size))
        assertEquals(1, frames.size)
        assertEquals(300, SlipFraming.parse(frames.first())!!.payload.size)
    }
}
