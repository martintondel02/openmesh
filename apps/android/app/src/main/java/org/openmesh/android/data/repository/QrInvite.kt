package org.openmesh.android.data.repository

import org.openmesh.android.util.Hex
import java.net.URI
import java.net.URLDecoder
import java.net.URLEncoder

/**
 * `openmesh://contact?...` and `openmesh://channel?...` invite URIs (WEBSOCKET-API.md §4.3/4.4).
 *
 * Implemented with `java.net.URI` + explicit query encoding (not `android.net.Uri`) so the same
 * parser runs in JVM unit tests. Every scanned invite is validated before use.
 */
data class QrInvite(
    val kind: Kind,
    val nodeId: String,
    val name: String,
    val identityBase64: String = "",
    val pskBase64: String = "",
    val profile: String? = null,
    val region: String? = null,
) {
    enum class Kind { CONTACT, CHANNEL }

    companion object {
        fun parse(raw: String): QrInvite {
            val trimmed = raw.trim()
            val uri = runCatching { URI(trimmed) }.getOrElse { throw IllegalArgumentException("Not a valid URL: $trimmed") }
            require(uri.scheme == "openmesh") { "Not an openmesh:// link" }
            val kind = when (uri.host?.lowercase()) {
                "contact", "channel" -> uri.host!!.uppercase()
                else -> throw IllegalArgumentException("Unsupported invite type: ${uri.host}")
            }
            val params = parseQuery(uri.rawQuery)
            val id = params["node_id"] ?: params["id"]
                ?: throw IllegalArgumentException("Invite is missing a node/channel id")
            require(id.length == 16 && Hex.decodeOrNull(id) != null) {
                "Invite id must be 16 hex characters"
            }
            return QrInvite(
                kind = Kind.valueOf(kind),
                nodeId = id.lowercase(),
                name = (params["name"] ?: id.take(8)).take(32),
                identityBase64 = params["identity"].orEmpty(),
                pskBase64 = params["psk"].orEmpty(),
                profile = params["profile"],
                region = params["region"],
            )
        }

        fun contact(nodeId: String, name: String, identityBase64: String): String =
            build("contact", listOf("node_id" to nodeId, "name" to name, "identity" to identityBase64))

        fun channel(
            channelId: String,
            name: String,
            pskBase64: String,
            profile: String,
            region: String,
        ): String = build(
            "channel",
            listOf(
                "id" to channelId,
                "name" to name,
                "psk" to pskBase64,
                "profile" to profile,
                "region" to region,
            ),
        )

        private fun build(host: String, params: List<Pair<String, String>>): String {
            val query = params.joinToString("&") { (key, value) ->
                "${encode(key)}=${encode(value)}"
            }
            return "openmesh://$host?$query"
        }

        private fun parseQuery(query: String?): Map<String, String> {
            if (query.isNullOrBlank()) return emptyMap()
            return query.split('&').mapNotNull { part ->
                if (part.isBlank()) return@mapNotNull null
                val idx = part.indexOf('=')
                if (idx <= 0) return@mapNotNull null
                decode(part.substring(0, idx)) to decode(part.substring(idx + 1))
            }.toMap()
        }

        private fun encode(value: String): String =
            URLEncoder.encode(value, Charsets.UTF_8.name()).replace("+", "%20")

        private fun decode(value: String): String =
            runCatching { URLDecoder.decode(value, Charsets.UTF_8.name()) }.getOrDefault(value)
    }
}
