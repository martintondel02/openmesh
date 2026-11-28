package org.openmesh.android.data.model

import java.net.URLEncoder

/**
 * Built-in "Public Mesh" channel shared by every client.
 *
 * The canonical channel address is the PROTOCOL-DERIVED id (`SHA-256("openmesh/v1/chan" || HKDF(
 * psk, "channel/" || name))[0..8]` with the channel high-bit set) — clients must use the id the
 * core exposes; the invite URL carries only name + published PSK (the old cosmetic `baac…` id is
 * dropped). Public by design: anyone with OpenMesh can read/post, senders are signature-verified.
 *
 * Pure constants/helpers so they are unit-tested (`PublicMeshTest`) and shared by UI/repository.
 */
object PublicMesh {
    const val NAME: String = "Public Mesh"

    /** Published 32-byte channel PSK, base64url without padding (as shared in the invite URL). */
    const val PSK_BASE64URL: String = "-w-KmLkWsRmxrtQqcPT2sXrlcTHTiaCNu1F-Qp_YqZI"

    /** Protocol-derived channel address (high bit set); the bridge/core expose the same value. */
    const val CANONICAL_CHANNEL_ID: String = "a16d63142854cef4"

    /** Join URL: name + PSK only; clients derive/obtain the id, the URL carries no id parameter. */
    const val JOIN_URL: String =
        "openmesh://channel?name=" +
            "Public%20Mesh&psk=" + PSK_BASE64URL

    const val BADGE: String = "PUBLIC"

    fun isPublicChannel(channelId: String, name: String): Boolean =
        name == NAME || channelId.lowercase() == CANONICAL_CHANNEL_ID

    fun warning(): String =
        "Public channel: anyone with OpenMesh can read and post here. Messages are signed so " +
            "senders can be verified, but nothing here is private."

    fun headerSubtitle(): String = "Public · anyone can join"

    fun shortWarning(): String = "Anyone with OpenMesh can read/post · senders are signed"

    /** Decoded 32-byte PSK; the join URL/QR stores the base64url text form. */
    fun pskBytes(): ByteArray = java.util.Base64.getUrlDecoder().decode(PSK_BASE64URL)

    fun joinUrl(name: String = NAME, psk: String = PSK_BASE64URL): String =
        "openmesh://channel?name=${URLEncoder.encode(name, Charsets.UTF_8.name()).replace("+", "%20")}" +
            "&psk=${URLEncoder.encode(psk, Charsets.UTF_8.name())}"

    /** Pinned order for channel lists: Public Mesh first, then the existing order. */
    fun pinFirst(channels: List<Channel>): List<Channel> =
        channels.sortedWith(compareByDescending<Channel> { it.isPublic }.thenBy { it.name.lowercase() })
}
