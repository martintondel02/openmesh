package org.openmesh.android.data.transport

/**
 * Typed radio-link progress model shared by transports, the repository and the UI.
 *
 * Everything in this file is pure Kotlin (no Android imports) so the phase/health/error copy can
 * be unit-tested on the JVM, and so the same wording is used by every surface (radio screen,
 * banner, settings, nodes header, foreground-service notification).
 *
 * Lifecycle of a link attempt:
 *   NONE → SCANNING (BLE only) → CONNECTING → HANDSHAKING → READY
 * Any step can end in ERROR; a ready link that stops answering stays READY but becomes STALE
 * (see [RadioHealthPolicy]), and a lost link becomes ERROR with [LinkError.LINK_LOST].
 */
enum class RadioPhase { NONE, SCANNING, CONNECTING, HANDSHAKING, READY, ERROR }

/**
 * Link health for a READY link. UNKNOWN before the first reply / when not ready, HEALTHY while the
 * companion radio answers its 15 s keep-alive, STALE when it has been silent for too long.
 */
enum class RadioHealth { UNKNOWN, HEALTHY, STALE }

/** Why a connection attempt or an established link failed; drives the beginner hint copy. */
enum class LinkError {
    BLUETOOTH_OFF,
    BLUETOOTH_PERMISSION,
    NO_RADIO_FOUND,
    SCAN_FAILED,
    GATT_FAILED,
    SERVICE_MISSING,
    HANDSHAKE_TIMEOUT,
    LINK_LOST,
    USB_NO_DEVICE,
    USB_PERMISSION,
    USB_PORT,
    UNKNOWN,
}

/**
 * One snapshot of what a transport is doing right now. Transports only ever set the phases they
 * own (SCANNING/CONNECTING/HANDSHAKING/ERROR/NONE); READY is decided by the repository once the
 * companion handshake (HelloAck) has completed.
 */
data class LinkProgress(
    val phase: RadioPhase = RadioPhase.NONE,
    /** Short, plain-language status line ("Looking for OpenMesh radios…"). */
    val message: String = "",
    val deviceName: String? = null,
    val deviceAddress: String? = null,
    val rssi: Int? = null,
    /** How many candidate radios the scan saw (0 when not scanning). */
    val devicesFound: Int = 0,
    /** 1-based attempt counter (kept for future automatic retries). */
    val attempt: Int = 1,
    /** Set when [phase] is ERROR. */
    val error: LinkError? = null,
    /** System.currentTimeMillis() when this phase started, for the elapsed counter. */
    val sinceMs: Long = 0L,
) {
    val isInProgress: Boolean
        get() = phase == RadioPhase.SCANNING || phase == RadioPhase.CONNECTING || phase == RadioPhase.HANDSHAKING
}

/**
 * Beginner-language hints (1–2 short bullets) for each failure. Copy rules:
 * docs/design/ux-help-system.md §6 — second person, no blame, no protocol jargon, actionable.
 */
object RadioErrorHints {
    fun hintFor(error: LinkError): String = when (error) {
        LinkError.BLUETOOTH_OFF ->
            "• Turn Bluetooth on in your phone's quick settings.\n• Then try again."
        LinkError.BLUETOOTH_PERMISSION ->
            "• Allow Nearby devices / Bluetooth access when Android asks.\n" +
                "• You can also enable it in Settings → Apps → OpenMesh → Permissions."
        LinkError.NO_RADIO_FOUND ->
            "• Check the radio is switched on (battery or USB power).\n" +
                "• Keep the phone within a few metres of the radio."
        LinkError.SCAN_FAILED ->
            "• Turn Bluetooth off and on again in your phone's quick settings.\n• Then try again."
        LinkError.GATT_FAILED ->
            "• Move closer to the radio and try again.\n" +
                "• If another phone is already using this radio, disconnect that first."
        LinkError.SERVICE_MISSING ->
            "• This device does not seem to run OpenMesh companion firmware.\n" +
                "• Check the firmware version, or connect the radio over USB instead."
        LinkError.HANDSHAKE_TIMEOUT ->
            "• The radio answered but did not finish setting up the link.\n" +
                "• Its firmware may be older or newer than the app; update it, or try USB."
        LinkError.LINK_LOST ->
            "• The radio stopped answering — it may be out of range or switched off.\n" +
                "• Move closer or check the cable, then reconnect."
        LinkError.USB_NO_DEVICE ->
            "• Plug the radio in with a USB data cable.\n" +
                "• Some cables only charge — try the cable the radio came with."
        LinkError.USB_PERMISSION ->
            "• Accept the USB prompt when Android asks.\n" +
                "• If you dismissed it, unplug the radio and plug it in again."
        LinkError.USB_PORT ->
            "• Close other apps that may be using the radio.\n" +
                "• Unplug it, plug it back in, then try again."
        LinkError.UNKNOWN ->
            "• Switch the radio off and on again, then try again.\n" +
                "• If it keeps happening, open Help → Troubleshooting."
    }
}

/** Pure health rule shared by the repository ticker and the presentation layer. */
object RadioHealthPolicy {
    /** The companion client pings every 15 s; allow two missed pings before calling it stale. */
    const val STALE_AFTER_MS: Long = 30_000L

    fun compute(phase: RadioPhase, lastReplyMs: Long, nowMs: Long): RadioHealth = when {
        phase != RadioPhase.READY -> RadioHealth.UNKNOWN
        lastReplyMs <= 0L -> RadioHealth.STALE
        nowMs - lastReplyMs > STALE_AFTER_MS -> RadioHealth.STALE
        else -> RadioHealth.HEALTHY
    }
}
