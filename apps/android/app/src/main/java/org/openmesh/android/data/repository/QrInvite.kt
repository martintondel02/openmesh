package org.openmesh.android.data.repository

import android.net.Uri
import org.openmesh.android.util.Hex

/**
 * `openmesh://contact?...` and `openmesh://channel?...` invite URIs (WEBSOCKET-API.md §4.3/4.4).
 *
 * Example contact:
 * `openmesh://contact?node_id=<16 hex>&name=Alice&identity=<base64 72B>`
 *
 * Example channel:
 * `openmesh://channel?id=<16 hex>&name=family&psk=<base64 32B>&profile=MEDIUM_FAST&region=EU868`
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
            val uri = Uri.parse(raw.trim())
            require(uri.scheme == "openmesh") { "Not an openmesh:// link" }
            val kind = when (uri.host) {
                "contact", "channel" -> uri.host!!.uppercase()
                else -> error("Unsupported invite type: ${uri.host}")
            }
            val id = uri.getQueryParameter("node_id") ?: uri.getQueryParameter("id")
                ?: error("Invite is missing a node/channel id")
            require(id.length == 16 && Hex.decodeOrNull(id) != null) {
                "Invite id must be 16 hex characters"
            }
            return QrInvite(
                kind = Kind.valueOf(kind),
                nodeId = id.lowercase(),
                name = uri.getQueryParameter("name")?.take(32) ?: id.take(8),
                identityBase64 = uri.getQueryParameter("identity").orEmpty(),
                pskBase64 = uri.getQueryParameter("psk").orEmpty(),
                profile = uri.getQueryParameter("profile"),
                region = uri.getQueryParameter("region"),
            )
        }

        fun contact(nodeId: String, name: String, identityBase64: String): String =
            Uri.Builder()
                .scheme("openmesh")
                .authority("contact")
                .appendQueryParameter("node_id", nodeId)
                .appendQueryParameter("name", name)
                .appendQueryParameter("identity", identityBase64)
                .build()
                .toString()

        fun channel(
            channelId: String,
            name: String,
            pskBase64: String,
            profile: String,
            region: String,
        ): String = Uri.Builder()
            .scheme("openmesh")
            .authority("channel")
            .appendQueryParameter("id", channelId)
            .appendQueryParameter("name", name)
            .appendQueryParameter("psk", pskBase64)
            .appendQueryParameter("profile", profile)
            .appendQueryParameter("region", region)
            .build()
            .toString()
    }
}
