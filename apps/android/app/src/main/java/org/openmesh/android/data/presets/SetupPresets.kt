package org.openmesh.android.data.presets

import android.content.Context
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import java.io.File

/**
 * Beginner secure presets shared with the bridge setup API and web demo.
 *
 * Source of truth: `content/presets/setup.json` (normative, docs/design/android-ga.md §3). Gradle
 * copies it to `assets/presets/setup.json`; unit tests parse the repo file directly and assert
 * this Kotlin model maps it exactly.
 */
@Serializable
data class RolePreset(
    val profile: String,
    @SerialName("store_forward") val storeForward: Boolean,
    @SerialName("position_sharing") val positionSharing: String,
    @SerialName("announce_interval_s") val announceIntervalS: Long,
    val description: String = "",
)

@Serializable
data class SecurityDefaults(
    @SerialName("api_bind") val apiBind: String = "127.0.0.1",
    @SerialName("backbone_auth") val backboneAuth: String = "token",
    @SerialName("backbone_bind_public") val backboneBindPublic: Boolean = false,
    val notes: String = "",
)

@Serializable
data class SetupPresets(
    val version: Int = 1,
    val description: String = "",
    @SerialName("default_region") val defaultRegion: String = "EU868",
    @SerialName("country_regions") val countryRegions: Map<String, String> = emptyMap(),
    val roles: Map<String, RolePreset> = emptyMap(),
    val security: SecurityDefaults = SecurityDefaults(),
    // Media caps are owned by the engine/profile table; keep unknown JSON so the parser stays
    // forward-compatible when the preset file grows.
    val media: JsonElement? = null,
) {
    fun regionFor(country: String?): String =
        countryRegions[country?.trim()?.uppercase().orEmpty()] ?: defaultRegion

    fun roleFor(role: String): RolePreset? = roles[role.trim().lowercase()]
}

object PresetLoader {
    val json = Json { ignoreUnknownKeys = true; isLenient = false }

    fun parse(text: String): SetupPresets = json.decodeFromString(text)

    fun fromAssets(context: Context): SetupPresets {
        val text = context.assets.open("presets/setup.json").bufferedReader().use { it.readText() }
        return parse(text)
    }

    /** Used by JVM unit tests: resolves the real repo file (apps/android/app working dir). */
    fun fromRepoFile(candidates: List<File> = defaultRepoCandidates()): SetupPresets {
        val file = candidates.firstOrNull { it.isFile }
            ?: error(
                "content/presets/setup.json not found; tried:\n" +
                    candidates.joinToString("\n") { "  ${it.absolutePath}" },
            )
        return parse(file.readText())
    }

    fun defaultRepoCandidates(): List<File> {
        val cwd = File(System.getProperty("user.dir"))
        val repoProp = System.getProperty("openmesh.repoRoot")?.let(::File)
        val candidates = mutableListOf<File>()
        if (repoProp != null) candidates += File(repoProp, "content/presets/setup.json")
        // Gradle unit tests run with cwd = apps/android/app or apps/android depending on AGP.
        candidates += File(cwd, "../../content/presets/setup.json")
        candidates += File(cwd, "../../../content/presets/setup.json")
        candidates += File(cwd, "content/presets/setup.json")
        return candidates.map { it.canonicalFile }
    }
}

/** What the onboarding wizard collects in step 1–2. */
data class BeginnerAnswers(
    val role: String,
    val country: String,
    val nodeName: String,
    val privacy: String = "private",
)

/** Result of applying the secure preset locally (no network, no bridge). */
data class AppliedPreset(
    val region: String,
    val profile: String,
    val storeForward: Boolean,
    val announceIntervalS: Long,
    val positionSharing: String,
)

object BeginnerPreset {
    fun apply(presets: SetupPresets, answers: BeginnerAnswers): AppliedPreset {
        val role = presets.roleFor(answers.role) ?: error("unknown role '${answers.role}'")
        return AppliedPreset(
            region = presets.regionFor(answers.country),
            profile = role.profile,
            storeForward = role.storeForward,
            announceIntervalS = role.announceIntervalS,
            positionSharing = role.positionSharing,
        )
    }

    /**
     * Position sharing from the preset: "private" keeps it off until explicitly shared,
     * "channel" shares on joined channels, "off" disables the control entirely.
     */
    fun positionEnabled(preset: AppliedPreset): Boolean = preset.positionSharing == "channel"
}
