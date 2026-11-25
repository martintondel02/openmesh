package org.openmesh.android.util

import kotlin.math.ceil
import kotlin.math.pow

/**
 * LoRa airtime model, mirroring `crates/openmesh-core/src/util.rs` (Semtech AN1200.13).
 *
 * The UI must show the cost of a message before it is sent; this is the single source of
 * that estimate on Android. Keep it in sync with the Rust implementation; the unit test
 * asserts the protocol's worked examples.
 */
enum class LoraProfile(
    val label: String,
    val sf: Int,
    val bwKhz: Int,
    /** 1..4 for 4/5..4/8 coding rate. */
    val cr: Int,
    val preambleSymbols: Int = 16,
    val explicitHeader: Boolean = true,
    val crc: Boolean = true,
) {
    LONG_SLOW("Long slow", sf = 12, bwKhz = 125, cr = 4),
    LONG_FAST("Long fast", sf = 11, bwKhz = 250, cr = 1),
    MEDIUM_FAST("Medium fast", sf = 9, bwKhz = 250, cr = 1),
    SHORT_FAST("Short fast", sf = 7, bwKhz = 250, cr = 1),
    SHORT_TURBO("Short turbo", sf = 7, bwKhz = 500, cr = 1);

    /** Payload bytes for a 240-byte OMP frame (the MTU). */
    fun airtimeMs(payloadBytes: Int): Int = estimateAirtimeMs(this, payloadBytes)

    val maxFrameAirtimeMs: Int get() = airtimeMs(240)

    companion object {
        fun fromName(name: String): LoraProfile =
            entries.firstOrNull { it.name == name } ?: MEDIUM_FAST
    }
}

data class AirtimeEstimate(
    val profile: LoraProfile,
    val payloadBytes: Int,
    val frameBytes: Int,
    val airtimeMs: Int,
    /** Wall clock if the sender is limited to [dutyCyclePct] percent transmit time. */
    val wallClockMsAtDutyCycle: Long,
) {
    val airtimeSeconds: Double get() = airtimeMs / 1000.0
}

fun estimateAirtimeMs(profile: LoraProfile, payloadLen: Int): Int {
    val bwHz = profile.bwKhz * 1000.0
    val tsym = 2.0.pow(profile.sf) / bwHz
    val preamble = (profile.preambleSymbols + 4.25) * tsym
    val pl = payloadLen + if (profile.crc) 2 else 0
    val ih = if (profile.explicitHeader) 0 else 1
    val de = if (profile.sf >= 11 && profile.bwKhz == 125) 1 else 0
    val num = 8.0 * pl - 4.0 * profile.sf + 28.0 - 20.0 * ih
    val den = 4.0 * (profile.sf - 2.0 * de)
    val n = maxOf(ceil(num / den), 0.0)
    val symbols = 8.0 + n * (profile.cr + 4.0)
    val totalMs = (preamble + symbols * tsym) * 1000.0
    return maxOf(ceil(totalMs).toInt(), 1)
}

/**
 * Estimate the cost of transferring [bytes] as an OMP resource: manifest + 180-byte chunks +
 * one window ACK per 8 chunks + completion ACK, per docs/architecture/PROTOCOL.md §9.
 */
fun estimateResourceTransfer(profile: LoraProfile, bytes: Int, chunkBytes: Int = 180): AirtimeEstimate {
    val chunks = maxOf(1, (bytes + chunkBytes - 1) / chunkBytes)
    val manifestFrame = 191
    val chunkFrame = 213 // 22 header + 1 flags + 4 msg_ref + 4 res_id + 2 index + 180
    val ackFrame = 143
    val windowAcks = (chunks + 7) / 8
    var airtime = estimateAirtimeMs(profile, manifestFrame)
    airtime += chunks * estimateAirtimeMs(profile, chunkFrame)
    airtime += windowAcks * estimateAirtimeMs(profile, ackFrame)
    airtime += estimateAirtimeMs(profile, 120) // completion ACK
    val frameBytes = manifestFrame + chunks * chunkFrame + windowAcks * ackFrame + 120
    return AirtimeEstimate(profile, bytes, frameBytes, airtime, 0L)
}

/** Wall-clock at a duty cycle: airtime / duty. 1 % => 100x; 10 % => 10x; null => 1x. */
fun wallClockMs(airtimeMs: Int, dutyCyclePct: Int?): Long =
    if (dutyCyclePct == null || dutyCyclePct <= 0) airtimeMs.toLong()
    else airtimeMs.toLong() * 100L / dutyCyclePct

/** Region profiles from `crates/openmesh-core/src/util.rs`; keep in sync. */
enum class Region(val label: String, val dutyCyclePct: Int?, val maxPowerDbm: Int) {
    EU868("EU 868 MHz", 1, 14),
    US915("US 915 MHz", null, 20),
    AU915("AU 915 MHz", null, 20),
    IN865("IN 865 MHz", null, 20),
    KR920("KR 920 MHz", null, 14),
    AS923("AS 923 MHz", 1, 14);

    companion object {
        fun fromName(name: String): Region = entries.firstOrNull { it.name == name } ?: EU868
    }
}
