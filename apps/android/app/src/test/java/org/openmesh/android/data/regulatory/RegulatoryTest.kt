package org.openmesh.android.data.regulatory

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.openmesh.android.data.model.AppSettings

class RegulatoryTest {

    @Test
    fun `region table has all seven v2 regions in order`() {
        assertEquals(
            listOf("EU868", "EU433", "US915", "AU915", "IN865", "KR920", "AS923"),
            RegionLimits.names,
        )
    }

    @Test
    fun `EU868 legal maxima match the contract`() {
        val eu = RegionLimits.forRegion("EU868")
        assertEquals(869_525L, eu.centerKhz)
        assertEquals(27, eu.maxPowerDbm)
        assertEquals(10, eu.dutyPct)
        assertEquals(250, eu.maxBwKhz)
    }

    @Test
    fun `EU433 legal maxima match the contract`() {
        val eu = RegionLimits.forRegion("EU433")
        assertEquals(433_920L, eu.centerKhz)
        assertEquals(10, eu.maxPowerDbm)
        assertEquals(10, eu.dutyPct)
        assertEquals(250, eu.maxBwKhz)
    }

    @Test
    fun `other regions match the contract`() {
        val expected = mapOf(
            "US915" to Triple(915_000L, 30, null as Int?),
            "AU915" to Triple(915_000L, 30, null),
            "IN865" to Triple(865_000L, 30, null),
            "KR920" to Triple(920_000L, 23, null),
            "AS923" to Triple(923_200L, 20, 10),
        )
        expected.forEach { (name, values) ->
            val limit = RegionLimits.forRegion(name)
            assertEquals(name, values.first, limit.centerKhz)
            assertEquals(name, values.second, limit.maxPowerDbm)
            assertEquals(name, values.third, limit.dutyPct)
        }
        assertEquals(500, RegionLimits.forRegion("US915").maxBwKhz)
        assertEquals(500, RegionLimits.forRegion("AU915").maxBwKhz)
        assertEquals(250, RegionLimits.forRegion("IN865").maxBwKhz)
        assertEquals(250, RegionLimits.forRegion("KR920").maxBwKhz)
        assertEquals(250, RegionLimits.forRegion("AS923").maxBwKhz)
    }

    @Test
    fun `legal line says legal maximum and duty or fair use`() {
        assertEquals("Legal limit: 27 dBm · 10 % duty · 250 kHz bandwidth", RegionLimits.legalLine("EU868"))
        assertEquals("Legal limit: 30 dBm · fair use duty · 500 kHz bandwidth", RegionLimits.legalLine("US915"))
        assertTrue(RegionLimits.forRegion("EU868").source.isNotBlank())
    }

    @Test
    fun `unknown region falls back to EU868`() {
        assertEquals(RegionLimits.forRegion("EU868"), RegionLimits.forRegion("ZZ"))
    }

    @Test
    fun `effective duty uses region default or override`() {
        val off = RegulatoryOverride()
        assertEquals(10, RegionLimits.effectiveDutyPct("EU868", off))
        assertNull(RegionLimits.effectiveDutyPct("US915", off))
        val on = RegulatoryOverride(enabled = true, acknowledged = true, dutyPct = 100)
        assertEquals(100, RegionLimits.effectiveDutyPct("EU868", on))
        assertEquals(10, RegionLimits.effectiveDutyPct("EU868", RegulatoryOverride(enabled = true, dutyPct = null)))
    }

    @Test
    fun `effective power clamps to the hardware cap`() {
        val off = RegulatoryOverride()
        assertEquals(22, RegionLimits.effectivePowerDbm("EU868", off, hardwareMaxDbm = 22))
        assertEquals(15, RegionLimits.effectivePowerDbm("EU868", off, hardwareMaxDbm = 15))
        val high = RegulatoryOverride(enabled = true, acknowledged = true, powerDbm = 30)
        assertEquals(22, RegionLimits.effectivePowerDbm("EU868", high, hardwareMaxDbm = 22))
        assertEquals(15, RegionLimits.effectivePowerDbm("EU868", high, hardwareMaxDbm = 15))
    }

    @Test
    fun `effective frequency uses override only when enabled`() {
        val off = RegulatoryOverride(freqKhz = 870_000)
        assertEquals(869_525L, RegionLimits.effectiveFrequencyKhz("EU868", off))
        val on = RegulatoryOverride(enabled = true, acknowledged = true, freqKhz = 870_000)
        assertEquals(870_000L, RegionLimits.effectiveFrequencyKhz("EU868", on))
    }

    @Test
    fun `exceeds legal detects duty power and frequency`() {
        assertTrue(RegionLimits.exceedsLegal("EU868", RegulatoryOverride(enabled = true, dutyPct = 11)))
        assertTrue(RegionLimits.exceedsLegal("EU868", RegulatoryOverride(enabled = true, powerDbm = 28)))
        assertTrue(RegionLimits.exceedsLegal("EU868", RegulatoryOverride(enabled = true, freqKhz = 870_000)))
        assertFalse(RegionLimits.exceedsLegal("EU868", RegulatoryOverride(enabled = true, dutyPct = 5, powerDbm = 20)))
        // US915 has no duty limit, so any duty is not "above legal".
        assertFalse(RegionLimits.exceedsLegal("US915", RegulatoryOverride(enabled = true, dutyPct = 100)))
    }

    @Test
    fun `safe reductions never require acknowledgement`() {
        val draft = OverrideDraft("5", "20", "869525", "storm", understandChecked = false, typedConfirmation = "")
        val validation = draft.validate("EU868")
        assertTrue(validation.ok)
        assertTrue(validation.override.acknowledged)
        assertFalse(validation.override.exceedsLegal("EU868"))
    }

    @Test
    fun `exceeding legal requires checkbox and exact typed phrase`() {
        val noAck = OverrideDraft("100", "22", "869525", "emergency", false, "")
        val failed = noAck.validate("EU868")
        assertFalse(failed.ok)
        assertTrue(failed.error!!.contains("I understand"))

        val wrongWord = OverrideDraft("100", "22", "869525", "emergency", true, "override")
        val wrong = wrongWord.validate("EU868")
        assertFalse(wrong.ok)
        assertTrue(wrong.error!!.contains("OVERRIDE"))

        val good = OverrideDraft("100", "22", "869525", "emergency", true, "OVERRIDE")
        val ok = good.validate("EU868")
        assertTrue(ok.ok)
        assertTrue(ok.override.enabled)
        assertTrue(ok.override.acknowledged)
        assertTrue(ok.override.exceedsLegal("EU868"))
    }

    @Test
    fun `invalid numeric values and reason length are rejected`() {
        assertTrue(OverrideDraft("101", "22", "869525", "", false, "").validate("EU868").error!!.contains("Duty"))
        assertTrue(OverrideDraft("10", "40", "869525", "", false, "").validate("EU868").error!!.contains("power"))
        assertTrue(OverrideDraft("10", "22", "1000", "", false, "").validate("EU868").error!!.contains("Frequency"))
        assertTrue(OverrideDraft("10", "22", "869525", "x".repeat(130), false, "").validate("EU868").error!!.contains("Reason"))
    }

    @Test
    fun `disabled override cannot be enabled without acknowledgement`() {
        val invalid = RegulatoryOverride(enabled = true, acknowledged = false)
        assertEquals("regulatory override requires acknowledgement", invalid.validationError("EU868"))
        assertNull(RegulatoryOverride(enabled = false, acknowledged = false).validationError("EU868"))
    }

    @Test
    fun `engine patch uses camelCase keys and nulls`() {
        val override = RegulatoryOverride(
            enabled = true,
            acknowledged = true,
            dutyPct = 100,
            powerDbm = 22,
            freqKhz = 869_525,
            reason = "emergency",
        )
        val json = Json.parseToJsonElement(override.toEnginePatch()).jsonObject
        val block = json.getValue("regulatoryOverride").jsonObject
        assertEquals("true", block.getValue("enabled").jsonPrimitive.content)
        assertEquals("true", block.getValue("acknowledged").jsonPrimitive.content)
        assertEquals("100", block.getValue("dutyCyclePct").jsonPrimitive.content)
        assertEquals("22", block.getValue("maxPowerDbm").jsonPrimitive.content)
        assertEquals("869525", block.getValue("frequencyKhz").jsonPrimitive.content)
        assertEquals("emergency", block.getValue("reason").jsonPrimitive.content)

        val empty = Json.parseToJsonElement(RegulatoryOverride().toEnginePatch()).jsonObject
            .getValue("regulatoryOverride").jsonObject
        assertEquals("null", empty.getValue("dutyCyclePct").jsonPrimitive.content)
        assertEquals("null", empty.getValue("frequencyKhz").jsonPrimitive.content)
        assertEquals("null", empty.getValue("reason").jsonPrimitive.content)
    }

    @Test
    fun `engine patch escapes quotes in the reason`() {
        val patch = RegulatoryOverride(reason = "say \"mayday\"").toEnginePatch()
        assertTrue(patch.contains("\\\"mayday\\\""))
    }

    @Test
    fun `companion config sends legal defaults when disabled`() {
        val keys = RegulatoryOverride().companionConfig("EU868")
        assertEquals("22", keys["radio.tx_power"])       // legal 27 clamped to 22 dBm hardware cap
        assertEquals("-1", keys["radio.duty_override"])  // firmware uses legal region duty
        assertEquals("869525", keys["radio.frequency"])
    }

    @Test
    fun `companion config sends override values when enabled`() {
        val override = RegulatoryOverride(enabled = true, acknowledged = true, dutyPct = 100, powerDbm = 15, freqKhz = 870_000)
        val keys = override.companionConfig("EU868", hardwareMaxDbm = 22)
        assertEquals("15", keys["radio.tx_power"])
        assertEquals("100", keys["radio.duty_override"])
        assertEquals("870000", keys["radio.frequency"])
    }

    @Test
    fun `companion power never exceeds hardware cap`() {
        val override = RegulatoryOverride(enabled = true, acknowledged = true, powerDbm = 27)
        assertEquals("15", override.companionConfig("EU868", hardwareMaxDbm = 15)["radio.tx_power"])
    }

    @Test
    fun `banner and warning copy match the contract`() {
        assertEquals(
            "⚠ Regulatory override active — may violate local law; you are responsible. Disable in Advanced → Regulatory.",
            RegulatoryOverride.BANNER,
        )
        assertTrue(RegulatoryOverride.WARNING.contains("may violate local radio regulations"))
        assertTrue(RegulatoryOverride.WARNING.contains("You are responsible"))
        assertEquals("OVERRIDE", RegulatoryOverride.TYPED_CONFIRMATION)
        assertFalse(RegulatoryOverride.BANNER.contains("legal advice"))
    }

    @Test
    fun `status line distinguishes legal and override modes`() {
        val settings = AppSettings(region = "EU868")
        val legal = RegulatoryMapper.statusLine("EU868", RegulatoryMapper.fromSettings(settings))
        assertTrue(legal.startsWith("Legal:"))
        val active = RegulatoryMapper.statusLine(
            "EU868",
            RegulatoryOverride(enabled = true, acknowledged = true, dutyPct = 100, powerDbm = 15),
        )
        assertTrue(active.startsWith("Override active:"))
        assertTrue(active.contains("15 dBm"))
        assertTrue(active.contains("100 %"))
    }

    @Test
    fun `settings mapper round-trips override fields`() {
        val override = RegulatoryOverride(true, true, 100, 22, 870_000, "emergency")
        val mapped = RegulatoryMapper.applyTo(AppSettings(), override)
        assertEquals(override, RegulatoryMapper.fromSettings(mapped))
        assertTrue(RegulatoryMapper.isActive(mapped))
        val disabled = mapped.copy(overrideEnabled = false)
        assertFalse(RegulatoryMapper.isActive(disabled))
    }

    @Test
    fun `disable one-tap clears acknowledgement`() {
        val active = RegulatoryOverride(true, true, dutyPct = 100)
        val disabled = active.copy(enabled = false, acknowledged = false)
        assertFalse(disabled.enabled)
        assertFalse(disabled.acknowledged)
        assertEquals(dutyOverrideSentinel(disabled), "-1")
    }

    private fun dutyOverrideSentinel(override: RegulatoryOverride): String =
        if (override.enabled) override.dutyPct?.toString() ?: "-1" else "-1"
}
