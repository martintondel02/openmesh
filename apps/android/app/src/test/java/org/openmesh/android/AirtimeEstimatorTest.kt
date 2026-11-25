package org.openmesh.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.openmesh.android.util.LoraProfile
import org.openmesh.android.util.Region
import org.openmesh.android.util.estimateResourceTransfer
import org.openmesh.android.util.wallClockMs

/**
 * Cross-checks the Kotlin airtime model against PROTOCOL.md Appendix A (which is generated from
 * the Rust implementation in `crates/openmesh-core/src/util.rs`). If these fail, the Android UI
 * is showing users the wrong cost and the model has drifted from the spec.
 */
class AirtimeEstimatorTest {

    @Test
    fun `medium fast matches the protocol worked example`() {
        assertEquals(611, LoraProfile.MEDIUM_FAST.airtimeMs(240))
        assertEquals(519, LoraProfile.MEDIUM_FAST.airtimeMs(201))
        assertEquals(488, LoraProfile.MEDIUM_FAST.airtimeMs(186))
    }

    @Test
    fun `profile ordering is monotonic for the same payload`() {
        val order = listOf(
            LoraProfile.SHORT_TURBO,
            LoraProfile.SHORT_FAST,
            LoraProfile.MEDIUM_FAST,
            LoraProfile.LONG_FAST,
            LoraProfile.LONG_SLOW,
        )
        val airtimes = order.map { it.airtimeMs(240) }
        assertEquals(airtimes.sorted(), airtimes)
        assertEquals(97, LoraProfile.SHORT_TURBO.airtimeMs(240))
        assertEquals(13_509, LoraProfile.LONG_SLOW.airtimeMs(240))
    }

    @Test
    fun `five kib image transfer is close to the spec example`() {
        val estimate = estimateResourceTransfer(LoraProfile.MEDIUM_FAST, 5 * 1024)
        // Spec Appendix A: ~17.8 s; this model's ACK counting yields ~18.3 s.
        assertTrue(estimate.airtimeMs in 17_000..19_500)
        assertEquals(29, (5 * 1024 + 179) / 180)
    }

    @Test
    fun `one percent duty cycle stretches wall clock by 100x`() {
        val estimate = estimateResourceTransfer(LoraProfile.MEDIUM_FAST, 5 * 1024)
        assertEquals(estimate.airtimeMs.toLong() * 100L, wallClockMs(estimate.airtimeMs, 1))
        assertEquals(estimate.airtimeMs.toLong(), wallClockMs(estimate.airtimeMs, null))
    }

    @Test
    fun `region duty cycles match the core constants`() {
        assertEquals(1, Region.EU868.dutyCyclePct)
        assertEquals(1, Region.AS923.dutyCyclePct)
        assertEquals(null, Region.US915.dutyCyclePct)
        assertEquals(14, Region.EU868.maxPowerDbm)
    }
}
