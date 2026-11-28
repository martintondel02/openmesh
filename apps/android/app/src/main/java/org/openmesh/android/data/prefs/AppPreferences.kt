package org.openmesh.android.data.prefs

import android.content.Context
import android.util.Base64
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.intPreferencesKey
import androidx.datastore.preferences.core.longPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map

private val Context.dataStore by preferencesDataStore(name = "openmesh")

enum class UiMode { BEGINNER, ADVANCED;
    companion object {
        fun from(value: String?): UiMode = if (value == "advanced") ADVANCED else BEGINNER
    }
    val wire: String get() = name.lowercase()
}

data class LocalPrefs(
    val uiMode: UiMode = UiMode.BEGINNER,
    val setupCompleted: Boolean = false,
    val nodeName: String = "openmesh-android",
    val role: String = "client",
    val profile: String = "MEDIUM_FAST",
    val region: String = "EU868",
    val storeForward: Boolean = false,
    val announceIntervalS: Long = 1800,
    val positionSharing: String = "private",
    val radioKind: String = "none",
    val autoConnect: Boolean = true,
    val notificationsEnabled: Boolean = true,
    val demoMode: Boolean = false,
    val identityCiphertext: String? = null,
    val engineStateB64: String? = null,
    val overrideEnabled: Boolean = false,
    val overrideAcknowledged: Boolean = false,
    val overrideDutyPct: Int? = null,
    val overridePowerDbm: Int? = null,
    val overrideFreqKhz: Long? = null,
    val overrideReason: String? = null,
    val defaultImageTier: String = "medium",
    val defaultVoiceTier: String = "tiny",
)

/** DataStore-backed app preferences and identity ciphertext (never the raw seed). */
class AppPreferences(private val context: Context) {
    private object Keys {
        val mode = stringPreferencesKey("ui_mode")
        val setupCompleted = booleanPreferencesKey("setup_completed")
        val nodeName = stringPreferencesKey("node_name")
        val role = stringPreferencesKey("role")
        val profile = stringPreferencesKey("profile")
        val region = stringPreferencesKey("region")
        val storeForward = booleanPreferencesKey("store_forward")
        val announceIntervalS = longPreferencesKey("announce_interval_s")
        val positionSharing = stringPreferencesKey("position_sharing")
        val radioKind = stringPreferencesKey("radio_kind")
        val autoConnect = booleanPreferencesKey("auto_connect")
        val notifications = booleanPreferencesKey("notifications_enabled")
        val demoMode = booleanPreferencesKey("demo_mode")
        val identityCiphertext = stringPreferencesKey("identity_ciphertext")
        val engineState = stringPreferencesKey("engine_state_b64")
        val overrideEnabled = booleanPreferencesKey("override_enabled")
        val overrideAcknowledged = booleanPreferencesKey("override_acknowledged")
        val overrideDuty = intPreferencesKey("override_duty_pct")
        val overridePower = intPreferencesKey("override_power_dbm")
        val overrideFreq = longPreferencesKey("override_frequency_khz")
        val overrideReason = stringPreferencesKey("override_reason")
        val defaultImageTier = stringPreferencesKey("default_image_tier")
        val defaultVoiceTier = stringPreferencesKey("default_voice_tier")
    }

    val prefs: Flow<LocalPrefs> = context.dataStore.data.map { p ->
        LocalPrefs(
            uiMode = UiMode.from(p[Keys.mode]),
            setupCompleted = p[Keys.setupCompleted] ?: false,
            nodeName = p[Keys.nodeName] ?: "openmesh-android",
            role = p[Keys.role] ?: "client",
            profile = p[Keys.profile] ?: "MEDIUM_FAST",
            region = p[Keys.region] ?: "EU868",
            storeForward = p[Keys.storeForward] ?: false,
            announceIntervalS = p[Keys.announceIntervalS] ?: 1800,
            positionSharing = p[Keys.positionSharing] ?: "private",
            radioKind = p[Keys.radioKind] ?: "none",
            autoConnect = p[Keys.autoConnect] ?: true,
            notificationsEnabled = p[Keys.notifications] ?: true,
            demoMode = p[Keys.demoMode] ?: false,
            identityCiphertext = p[Keys.identityCiphertext],
            engineStateB64 = p[Keys.engineState],
            overrideEnabled = p[Keys.overrideEnabled] ?: false,
            overrideAcknowledged = p[Keys.overrideAcknowledged] ?: false,
            overrideDutyPct = p[Keys.overrideDuty],
            overridePowerDbm = p[Keys.overridePower],
            overrideFreqKhz = p[Keys.overrideFreq],
            overrideReason = p[Keys.overrideReason],
            defaultImageTier = p[Keys.defaultImageTier] ?: "medium",
            defaultVoiceTier = p[Keys.defaultVoiceTier] ?: "tiny",
        )
    }

    suspend fun current(): LocalPrefs = prefs.first()

    suspend fun setMode(mode: UiMode) = edit { it[Keys.mode] = mode.wire }
    suspend fun setSetupCompleted(value: Boolean) = edit { it[Keys.setupCompleted] = value }
    suspend fun setNodeName(value: String) = edit { it[Keys.nodeName] = value }
    suspend fun setRole(value: String) = edit { it[Keys.role] = value }
    suspend fun setProfile(value: String) = edit { it[Keys.profile] = value }
    suspend fun setRegion(value: String) = edit { it[Keys.region] = value }
    suspend fun setStoreForward(value: Boolean) = edit { it[Keys.storeForward] = value }
    suspend fun setAnnounceInterval(value: Long) = edit { it[Keys.announceIntervalS] = value }
    suspend fun setPositionSharing(value: String) = edit { it[Keys.positionSharing] = value }
    suspend fun setRadioKind(value: String) = edit { it[Keys.radioKind] = value }
    suspend fun setAutoConnect(value: Boolean) = edit { it[Keys.autoConnect] = value }
    suspend fun setNotifications(value: Boolean) = edit { it[Keys.notifications] = value }
    suspend fun setDemoMode(value: Boolean) = edit { it[Keys.demoMode] = value }

    suspend fun setIdentityCiphertext(value: String) = edit { it[Keys.identityCiphertext] = value }

    suspend fun setDefaultImageTier(value: String) = edit { it[Keys.defaultImageTier] = value }
    suspend fun setDefaultVoiceTier(value: String) = edit { it[Keys.defaultVoiceTier] = value }

    suspend fun setOverride(
        enabled: Boolean,
        acknowledged: Boolean,
        dutyPct: Int?,
        powerDbm: Int?,
        freqKhz: Long?,
        reason: String?,
    ) = edit { prefs ->
        prefs[Keys.overrideEnabled] = enabled
        prefs[Keys.overrideAcknowledged] = acknowledged
        if (dutyPct != null) prefs[Keys.overrideDuty] = dutyPct else prefs.remove(Keys.overrideDuty)
        if (powerDbm != null) prefs[Keys.overridePower] = powerDbm else prefs.remove(Keys.overridePower)
        if (freqKhz != null) prefs[Keys.overrideFreq] = freqKhz else prefs.remove(Keys.overrideFreq)
        if (reason != null) prefs[Keys.overrideReason] = reason else prefs.remove(Keys.overrideReason)
    }
    /** Factory reset: wipe every persisted app preference and engine state. */
    suspend fun clearAll() = edit { it.clear() }

    suspend fun setEngineState(bytes: ByteArray) =
        edit { it[Keys.engineState] = Base64.encodeToString(bytes, Base64.NO_WRAP) }

    suspend fun engineState(): ByteArray? {
        val b64 = context.dataStore.data.first()[Keys.engineState] ?: return null
        return runCatching { Base64.decode(b64, Base64.NO_WRAP) }.getOrNull()
    }

    private suspend fun edit(block: (androidx.datastore.preferences.core.MutablePreferences) -> Unit) {
        context.dataStore.edit(block)
    }

    companion object {
        val MODES = listOf(UiMode.BEGINNER, UiMode.ADVANCED)
        val PROFILES = listOf("LONG_SLOW", "LONG_FAST", "MEDIUM_FAST", "SHORT_FAST", "SHORT_TURBO")
        val REGIONS = listOf("EU868", "US915", "AU915", "IN865", "KR920", "AS923")
        val ROLES = listOf("client", "tracker", "repeater", "gateway")
        val REGIONS_V2 = listOf("EU868", "EU433", "US915", "AU915", "IN865", "KR920", "AS923")
    }
}
