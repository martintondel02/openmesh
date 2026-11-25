package org.openmesh.android.data.help

import android.content.Context
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

@Serializable
data class HelpTopicMeta(
    val slug: String,
    val title: String,
    val summary: String,
    val audience: String,
    val minutes: Int,
    val related: List<String> = emptyList(),
)

@Serializable
private data class HelpIndexWire(
    val version: Int = 1,
    val topics: List<HelpTopicMeta> = emptyList(),
)

data class HelpTopic(val meta: HelpTopicMeta, val markdown: String) {
    val isBeginner: Boolean get() = meta.audience == "beginner" || meta.audience == "both"
    val isAdvanced: Boolean get() = meta.audience == "advanced" || meta.audience == "both"
}

/**
 * Offline help corpus. The same `content/help` files feed the web Help Center and the bridge
 * `GET /api/help*`; Gradle copies them into `assets/help` so the app works with no network.
 */
class HelpRepository(private val context: Context) {
    private val json = Json { ignoreUnknownKeys = true }
    private val cache = mutableMapOf<String, HelpTopic>()

    val topics: List<HelpTopicMeta> by lazy {
        runCatching {
            val text = context.assets.open("help/index.json").bufferedReader().use { it.readText() }
            json.decodeFromString<HelpIndexWire>(text).topics
        }.getOrDefault(emptyList())
    }

    fun topic(slug: String): HelpTopic? = cache.getOrPut(slug) {
        val meta = topics.firstOrNull { it.slug == slug } ?: return null
        val markdown = runCatching {
            context.assets.open("help/$slug.md").bufferedReader().use { it.readText() }
        }.getOrDefault("")
        HelpTopic(meta, markdown)
    }.takeIf { it.markdown.isNotEmpty() }

    fun search(query: String, audience: AudienceFilter = AudienceFilter.ALL): List<HelpTopicMeta> {
        val q = query.trim().lowercase()
        return topics.filter { meta ->
            val audienceOk = when (audience) {
                AudienceFilter.ALL -> true
                AudienceFilter.BEGINNER -> meta.audience == "beginner" || meta.audience == "both"
                AudienceFilter.ADVANCED -> meta.audience == "advanced" || meta.audience == "both"
            }
            if (!audienceOk) {
                false
            } else if (q.isEmpty()) {
                true
            } else if (meta.title.lowercase().contains(q) || meta.summary.lowercase().contains(q)) {
                true
            } else {
                topic(meta.slug)?.markdown?.lowercase()?.contains(q) == true
            }
        }
    }
}

enum class AudienceFilter(val label: String) {
    ALL("All"), BEGINNER("Beginner"), ADVANCED("Advanced")
}

/**
 * Registry mapping every settings/radio/onboarding control to a help topic (ux-help-system.md
 * §4.1: every control gets an inline explanation with a deep link). Unit-tested for completeness.
 */
object HelpKeys {
    const val GETTING_STARTED = "getting-started"
    const val INSTALL = "install"
    const val FIRST_MESH = "first-mesh"
    const val RADIO = "radio-and-region"
    const val MESSAGING = "messaging"
    const val MEDIA = "media"
    const val CHANNELS = "channels"
    const val CONTACTS = "contacts-identity"
    const val MAP = "map-and-position"
    const val NODES = "nodes-telemetry"
    const val BACKBONE = "backbone"
    const val AGENTS = "agents"
    const val SECURITY = "security"
    const val TROUBLESHOOTING = "troubleshooting"

    /** control id -> help slug */
    val registry: Map<String, String> = linkedMapOf(
        "settings.mode" to GETTING_STARTED,
        "settings.name" to CONTACTS,
        "settings.role" to NODES,
        "settings.radio.profile" to RADIO,
        "settings.radio.region" to RADIO,
        "settings.radio.tx_power" to RADIO,
        "settings.radio.announce" to NODES,
        "settings.features.store_forward" to MESSAGING,
        "settings.features.position" to MAP,
        "settings.notifications" to MESSAGING,
        "settings.identity.backup" to SECURITY,
        "settings.backbone.tip" to BACKBONE,
        "settings.agents.tip" to AGENTS,
        "onboarding.role" to FIRST_MESH,
        "onboarding.country" to RADIO,
        "onboarding.privacy" to MAP,
        "chat.send" to MESSAGING,
        "chat.media" to MEDIA,
        "chat.retry" to MESSAGING,
        "channels.create" to CHANNELS,
        "channels.join" to CHANNELS,
        "contacts.scan" to CONTACTS,
        "contacts.verify" to CONTACTS,
        "radio.connect" to RADIO,
        "radio.signal" to NODES,
        "map.share" to MAP,
        "nodes.signal" to NODES,
        "help.audience" to GETTING_STARTED,
    )

    fun slugFor(controlId: String): String? = registry[controlId]
}
