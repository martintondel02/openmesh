package org.openmesh.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.openmesh.android.data.transport.CborCodec

class CborCodecTest {

    @Test
    fun `hello payload round trips`() {
        val map = linkedMapOf<String, CborCodec.Value>(
            "proto" to CborCodec.UInt(1),
            "app_version" to CborCodec.Text("openmesh-android/0.1.0"),
            "wants_events" to CborCodec.Bool(true),
        )
        val decoded = CborCodec.decodeMap(CborCodec.encode(CborCodec.Map(map)))!!
        assertEquals(1L, (decoded["proto"] as CborCodec.UInt).value)
        assertEquals("openmesh-android/0.1.0", (decoded["app_version"] as CborCodec.Text).value)
        assertEquals(true, (decoded["wants_events"] as CborCodec.Bool).value)
    }

    @Test
    fun `byte strings and nested arrays round trip`() {
        val nodeId = byteArrayOf(1, 2, 3, 4, 5, 6, 7, 8)
        val entries = CborCodec.Array(
            listOf(
                CborCodec.Array(listOf(CborCodec.Text("tx_frames"), CborCodec.UInt(42))),
            ),
        )
        val value = CborCodec.Map(
            linkedMapOf(
                "node_id" to CborCodec.Bytes(nodeId),
                "entries" to entries,
            ),
        )
        val decoded = CborCodec.decodeMap(CborCodec.encode(value))!!
        assertTrue(nodeId.contentEquals((decoded["node_id"] as CborCodec.Bytes).value))
        val array = (decoded["entries"] as CborCodec.Array).values
        val pair = (array.first() as CborCodec.Array).values
        assertEquals("tx_frames", (pair[0] as CborCodec.Text).value)
        assertEquals(42L, (pair[1] as CborCodec.UInt).value)
    }

    @Test
    fun `negative integers decode with sign`() {
        val value = CborCodec.Map(linkedMapOf("snr" to CborCodec.NInt(-7)))
        val decoded = CborCodec.decodeMap(CborCodec.encode(value))!!
        assertEquals(-7L, (decoded["snr"] as CborCodec.NInt).value)
    }
}
