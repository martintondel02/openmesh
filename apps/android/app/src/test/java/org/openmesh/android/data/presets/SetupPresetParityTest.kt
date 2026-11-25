package org.openmesh.android.data.presets

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Parity with the single source of truth: content/presets/setup.json (Android GA §3, §5 test
 * requirement). The test resolves the real repository file, not a copied asset, so drift fails CI.
 */
class SetupPresetParityTest {
    private val presets = PresetLoader.fromRepoFile()

    @Test
    fun `presets file exists and has the expected schema version`() {
        assertTrue("version must be >= 1", presets.version >= 1)
        assertTrue("default region must be set", presets.defaultRegion.isNotBlank())
    }

    @Test
    fun `all four beginner roles are present with the documented values`() {
        val expected = mapOf(
            "client" to Triple("MEDIUM_FAST", false, 1800L),
            "tracker" to Triple("MEDIUM_FAST", false, 3600L),
            "repeater" to Triple("LONG_FAST", true, 3600L),
            "gateway" to Triple("LONG_FAST", true, 3600L),
        )
        assertEquals(expected.keys, presets.roles.keys)
        expected.forEach { (role, values) ->
            val preset = presets.roles.getValue(role)
            assertEquals("$role profile", values.first, preset.profile)
            assertEquals("$role store_forward", values.second, preset.storeForward)
            assertEquals("$role announce_interval_s", values.third, preset.announceIntervalS)
        }
    }

    @Test
    fun `position sharing semantics are private channel off`() {
        assertEquals("private", presets.roles.getValue("client").positionSharing)
        assertEquals("channel", presets.roles.getValue("tracker").positionSharing)
        assertEquals("off", presets.roles.getValue("repeater").positionSharing)
        assertEquals("off", presets.roles.getValue("gateway").positionSharing)
    }

    @Test
    fun `every country maps to one of the supported regions`() {
        val supported = setOf("EU868", "US915", "AU915", "IN865", "KR920", "AS923")
        assertTrue("expected a broad country table", presets.countryRegions.size >= 50)
        presets.countryRegions.forEach { (country, region) ->
            assertEquals("country codes are ISO-2 uppercase", 2, country.length)
            assertEquals(country.uppercase(), country)
            assertTrue("$country maps to unsupported region $region", region in supported)
        }
    }

    @Test
    fun `region lookup falls back to the default and is case-insensitive`() {
        assertEquals(presets.defaultRegion, presets.regionFor("ZZ"))
        val norway = presets.regionFor("no")
        assertEquals(presets.countryRegions.getValue("NO"), norway)
        assertEquals(presets.defaultRegion, presets.regionFor(""))
    }

    @Test
    fun `android applied preset matches the shared table for every role`() {
        presets.roles.forEach { (role, expected) ->
            val applied = BeginnerPreset.apply(presets, BeginnerAnswers(role = role, country = "NO", nodeName = "x"))
            assertEquals(expected.profile, applied.profile)
            assertEquals(expected.storeForward, applied.storeForward)
            assertEquals(expected.announceIntervalS, applied.announceIntervalS)
            assertEquals(expected.positionSharing, applied.positionSharing)
            assertEquals(presets.regionFor("NO"), applied.region)
        }
    }

    @Test
    fun `position toggle only follows the channel preset`() {
        val client = BeginnerPreset.apply(presets, BeginnerAnswers("client", "NO", "x"))
        val tracker = BeginnerPreset.apply(presets, BeginnerAnswers("tracker", "NO", "x"))
        val repeater = BeginnerPreset.apply(presets, BeginnerAnswers("repeater", "NO", "x"))
        assertFalse(BeginnerPreset.positionEnabled(client))
        assertTrue(BeginnerPreset.positionEnabled(tracker))
        assertFalse(BeginnerPreset.positionEnabled(repeater))
    }

    @Test
    fun `security defaults are documented and safe`() {
        assertEquals("127.0.0.1", presets.security.apiBind)
        assertEquals("token", presets.security.backboneAuth)
        assertFalse(presets.security.backboneBindPublic)
    }
}
