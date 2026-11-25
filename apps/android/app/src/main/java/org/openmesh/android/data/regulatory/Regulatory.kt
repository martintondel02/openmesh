package org.openmesh.android.data.regulatory

import org.openmesh.android.data.model.AppSettings

/**
 * Region legal maxima, mirroring docs/design/regulatory-and-regions.md §1 and the core/firmware
 * tables. The UI shows these values; the engine/firmware enforce them. Values are EIRP ceilings
 * and a high-gain antenna can exceed them — every surface says "verify your local rules".
 */
data class RegionLimit(
    val name: String,
    val centerKhz: Long,
    val maxPowerDbm: Int,
    val dutyPct: Int?,
    val maxBwKhz: Int,
    val hardwareMaxDbm: Int = 22,
    val source: String = "verify your local rules",
) {
    val dutyLabel: String get() = dutyPct?.let { "$it %" } ?: "fair use"
    val legalLine: String
        get() = "Legal limit: $maxPowerDbm dBm · $dutyLabel duty · $maxBwKhz kHz bandwidth"
}

object RegionLimits {
    /** Hardware caps: SX1262 boards 22 dBm, LR1110/T1000-E 15 dBm; default Android companion = 22. */
    const val DEFAULT_HARDWARE_MAX_DBM = 22
    const val MIN_FREQUENCY_KHZ = 150_000L
    const val MAX_FREQUENCY_KHZ = 960_000L

    val all: List<RegionLimit> = listOf(
        RegionLimit("EU868", 869_525, 27, 10, 250, 22, "CEPT ERC/REC 70-03 Annex 1 (EU/Norway)"),
        RegionLimit("EU433", 433_920, 10, 10, 250, 22, "CEPT 433.05–434.79 MHz"),
        RegionLimit("US915", 915_000, 30, null, 500, 22, "FCC 902–928 MHz (FHSS/DTS)"),
        RegionLimit("AU915", 915_000, 30, null, 500, 22, "ACMA 915–928 MHz"),
        RegionLimit("IN865", 865_000, 30, null, 250, 22, "India 865–867 MHz (audit pending)"),
        RegionLimit("KR920", 920_000, 23, null, 250, 22, "Korea 920–923 MHz"),
        RegionLimit("AS923", 923_200, 20, 10, 250, 22, "Conservative AS923 group floor"),
    )

    val names: List<String> get() = all.map { it.name }

    fun forRegion(name: String): RegionLimit =
        all.firstOrNull { it.name.equals(name.trim(), ignoreCase = true) } ?: all.first()

    fun legalLine(region: String): String = forRegion(region).legalLine

    fun effectiveDutyPct(region: String, override: RegulatoryOverride): Int? {
        val limit = forRegion(region)
        return if (override.enabled) override.dutyPct ?: limit.dutyPct else limit.dutyPct
    }

    /** Effective power clamp: legal/override request clamped to the hardware cap. */
    fun effectivePowerDbm(region: String, override: RegulatoryOverride, hardwareMaxDbm: Int = DEFAULT_HARDWARE_MAX_DBM): Int {
        val limit = forRegion(region)
        val requested = if (override.enabled) override.powerDbm ?: limit.maxPowerDbm else limit.maxPowerDbm
        return minOf(requested, hardwareMaxDbm)
    }

    fun effectiveFrequencyKhz(region: String, override: RegulatoryOverride): Long {
        val limit = forRegion(region)
        return if (override.enabled) override.freqKhz ?: limit.centerKhz else limit.centerKhz
    }

    /** True when the override requests more duty/power than the region allows or a custom frequency. */
    fun exceedsLegal(region: String, override: RegulatoryOverride): Boolean {
        if (!override.enabled) return false
        val limit = forRegion(region)
        // Fair-use regions have no numeric duty cap, so a duty value cannot exceed a limit there.
        val dutyExceeds = limit.dutyPct?.let { cap -> override.dutyPct?.let { it > cap } ?: false } ?: false
        val powerExceeds = override.powerDbm?.let { it > limit.maxPowerDbm } ?: false
        val freqDifferent = override.freqKhz?.let { it != limit.centerKhz } ?: false
        return dutyExceeds || powerExceeds || freqDifferent
    }

    /** A change that only reduces duty/power (or stays on the region frequency) is a safe reduction. */
    fun isSafeReduction(region: String, override: RegulatoryOverride): Boolean {
        if (!override.enabled) return true
        if (exceedsLegal(region, override)) return false
        val limit = forRegion(region)
        val dutyOk = override.dutyPct?.let { limit.dutyPct == null || it <= limit.dutyPct } ?: true
        val powerOk = override.powerDbm?.let { it <= limit.maxPowerDbm } ?: true
        val freqOk = override.freqKhz?.let { it == limit.centerKhz } ?: true
        return dutyOk && powerOk && freqOk
    }

    fun frequencyValid(khz: Long): Boolean = khz in MIN_FREQUENCY_KHZ..MAX_FREQUENCY_KHZ
}

/**
 * Regulatory override (core type mirrored on Android). `enabled && !acknowledged` is rejected by
 * the engine; this builder enforces the local gate too so the UI never sends an invalid patch.
 */
data class RegulatoryOverride(
    val enabled: Boolean = false,
    val acknowledged: Boolean = false,
    val dutyPct: Int? = null,
    val powerDbm: Int? = null,
    val freqKhz: Long? = null,
    val reason: String? = null,
) {
    fun exceedsLegal(region: String): Boolean = RegionLimits.exceedsLegal(region, this)

    fun requiresTypedAck(region: String): Boolean = enabled && exceedsLegal(region)

    fun validationError(region: String, hardwareMaxDbm: Int = RegionLimits.DEFAULT_HARDWARE_MAX_DBM): String? {
        dutyPct?.let { if (it !in 0..100) return "Duty cycle must be 0–100 %." }
        powerDbm?.let { if (it < -10 || it > hardwareMaxDbm) return "TX power must be between -10 and $hardwareMaxDbm dBm (hardware cap)." }
        freqKhz?.let { if (!RegionLimits.frequencyValid(it)) return "Frequency must be between 150 000 and 960 000 kHz." }
        if ((reason?.length ?: 0) > 120) return "Reason must be 120 characters or fewer."
        if (enabled && !acknowledged) return "regulatory override requires acknowledgement"
        return null
    }

    /** Engine/FFI JSON patch (camelCase, additive to FfiConfig). */
    fun toEnginePatch(): String = buildString {
        append("{\"regulatoryOverride\":{")
        append("\"enabled\":").append(enabled).append(',')
        append("\"acknowledged\":").append(acknowledged).append(',')
        append("\"dutyCyclePct\":").append(dutyPct?.toString() ?: "null").append(',')
        append("\"maxPowerDbm\":").append(powerDbm?.toString() ?: "null").append(',')
        append("\"frequencyKhz\":").append(freqKhz?.toString() ?: "null").append(',')
        append("\"reason\":").append(reason?.let { "\"${it.replace("\\", "\\\\").replace("\"", "\\\"")}\"" } ?: "null")
        append("}}")
    }

    /** Firmware companion ConfigSet keys (docs/design/regulatory-and-regions.md §2.3). */
    fun companionConfig(region: String, hardwareMaxDbm: Int = RegionLimits.DEFAULT_HARDWARE_MAX_DBM): Map<String, String> {
        val duty = if (enabled) dutyPct else null
        val power = RegionLimits.effectivePowerDbm(region, this, hardwareMaxDbm)
        val freq = RegionLimits.effectiveFrequencyKhz(region, this)
        return linkedMapOf(
            "radio.tx_power" to power.toString(),
            "radio.duty_override" to (duty?.toString() ?: "-1"),
            "radio.frequency" to freq.toString(),
        )
    }

    companion object {
        val WARNING = "Regulatory override active — transmissions may violate local radio regulations and void your radio's certification. Use only in a life-threatening emergency where local law permits. You are responsible for compliance and for any interference or damage."
        const val BANNER = "⚠ Regulatory override active — may violate local law; you are responsible. Disable in Advanced → Regulatory."
        const val TYPED_CONFIRMATION = "OVERRIDE"
        fun disabled() = RegulatoryOverride()
    }
}

/** Form state for the override modal; pure so the gating rules are unit-testable. */
data class OverrideDraft(
    val dutyText: String,
    val powerText: String,
    val frequencyText: String,
    val reason: String,
    val understandChecked: Boolean,
    val typedConfirmation: String,
) {
    fun toOverride(region: String, hardwareCap: Int = RegionLimits.DEFAULT_HARDWARE_MAX_DBM): RegulatoryOverride {
        val legal = RegionLimits.forRegion(region)
        val duty = dutyText.trim().toIntOrNull()
        val power = powerText.trim().toIntOrNull()
        val freq = frequencyText.trim().toLongOrNull()
        val candidate = RegulatoryOverride(
            enabled = true,
            acknowledged = understandChecked && typedConfirmation.trim() == RegulatoryOverride.TYPED_CONFIRMATION,
            dutyPct = duty ?: legal.dutyPct,
            powerDbm = power ?: legal.maxPowerDbm,
            freqKhz = freq ?: legal.centerKhz,
            reason = reason.trim().ifEmpty { null },
        )
        // Safe reductions never require acknowledgement: force it true if nothing exceeds legal.
        return if (!candidate.exceedsLegal(region)) candidate.copy(acknowledged = true) else candidate
    }

    fun validate(region: String, hardwareCap: Int = RegionLimits.DEFAULT_HARDWARE_MAX_DBM): OverrideValidation {
        val override = toOverride(region, hardwareCap)
        // Range/format/policy checks first, ignoring the acknowledgement gate so we can give the
        // UI-specific "I understand"/typed-phrase guidance below.
        override.copy(acknowledged = true).validationError(region, hardwareCap)?.let {
            return OverrideValidation(false, override, it)
        }
        if (override.exceedsLegal(region)) {
            if (!understandChecked) return OverrideValidation(false, override, "Tick “I understand” to enable an override above the legal limit.")
            if (typedConfirmation.trim() != RegulatoryOverride.TYPED_CONFIRMATION) {
                return OverrideValidation(false, override, "Type OVERRIDE exactly to confirm.")
            }
            if (!override.acknowledged) return OverrideValidation(false, override, "Acknowledgement is required for an override above legal limits.")
        }
        return OverrideValidation(true, override, null)
    }
}

data class OverrideValidation(val ok: Boolean, val override: RegulatoryOverride, val error: String?)

/** Helpers used by the repository to decide what to persist/send. */
object RegulatoryMapper {
    fun fromSettings(settings: AppSettings): RegulatoryOverride = RegulatoryOverride(
        enabled = settings.overrideEnabled,
        acknowledged = settings.overrideAcknowledged,
        dutyPct = settings.overrideDutyPct,
        powerDbm = settings.overridePowerDbm,
        freqKhz = settings.overrideFreqKhz,
        reason = settings.overrideReason,
    )

    fun applyTo(settings: AppSettings, override: RegulatoryOverride): AppSettings = settings.copy(
        overrideEnabled = override.enabled,
        overrideAcknowledged = override.acknowledged,
        overrideDutyPct = override.dutyPct,
        overridePowerDbm = override.powerDbm,
        overrideFreqKhz = override.freqKhz,
        overrideReason = override.reason,
    )

    fun isActive(settings: AppSettings): Boolean = settings.overrideEnabled

    fun statusLine(region: String, override: RegulatoryOverride, hardwareMaxDbm: Int = RegionLimits.DEFAULT_HARDWARE_MAX_DBM): String {
        val legal = RegionLimits.forRegion(region)
        return if (override.enabled) {
            val duty = override.dutyPct ?: legal.dutyPct
            val power = RegionLimits.effectivePowerDbm(region, override, hardwareMaxDbm)
            val freq = RegionLimits.effectiveFrequencyKhz(region, override)
            "Override active: ${power} dBm · ${duty?.let { "$it %" } ?: "fair use"} · ${freq / 1000.0} MHz"
        } else {
            "Legal: ${legal.maxPowerDbm} dBm · ${legal.dutyLabel} · ${legal.centerKhz / 1000.0} MHz"
        }
    }
}
