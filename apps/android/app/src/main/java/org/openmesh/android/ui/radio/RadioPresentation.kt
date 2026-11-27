package org.openmesh.android.ui.radio

import org.openmesh.android.data.model.RadioStatus
import org.openmesh.android.data.transport.LinkError
import org.openmesh.android.data.transport.RadioErrorHints
import org.openmesh.android.data.transport.RadioHealth
import org.openmesh.android.data.transport.RadioHealthPolicy
import org.openmesh.android.data.transport.RadioPhase

/**
 * Pure presentation/error-mapping helpers for the radio link surfaces (banner, radio screen,
 * settings row, nodes header, foreground-service notification).
 *
 * Deliberately free of Android/Compose imports so every rule here is unit-tested on the JVM
 * (`app/src/test/java/org/openmesh/android/ui/radio/RadioPresentationTest.kt`). The Compose layer
 * only maps [RadioTone] to theme colors and renders the strings.
 *
 * Copy rules: docs/design/ux-help-system.md §6 — second person, active voice, no blame,
 * no protocol jargon in the top card, every state has a next action.
 */
enum class RadioTone { NEUTRAL, PROGRESS, POSITIVE, WARNING, ERROR }

/** Per-step state of the "Scan → Connect → Handshake → Ready" rail. */
enum class StepState { DONE, ACTIVE, PENDING, FAILED }

data class RadioStep(val label: String, val state: StepState)

/** Headline copy for the state card on the radio screen. */
data class PhaseCopy(
    val title: String,
    val subtitle: String,
    val tone: RadioTone,
    val showSpinner: Boolean,
    /** Accessibility state description; the spinner is never the only indicator. */
    val stateDescription: String,
)

/** One-line copy for the chat-list banner. */
data class BannerCopy(val title: String, val subtitle: String, val tone: RadioTone)

/** Persistent error card copy (only shown while the phase is ERROR). */
data class ErrorCopy(val title: String, val message: String, val hint: String)

/** Transport to retry with when the user taps "Try again" on the error card. */
enum class RetryTransport { USB, BLE }

object RadioPresentation {

    private const val DEMO_TITLE = "Demo mode · virtual mesh"
    private const val READY_TITLE = "Radio ready"

    private val railLabels = listOf("Scan", "Connect", "Handshake", "Ready")

    fun isRunning(phase: RadioPhase): Boolean =
        phase == RadioPhase.SCANNING || phase == RadioPhase.CONNECTING || phase == RadioPhase.HANDSHAKING

    /** The link is healthy enough to hand new messages to the engine. */
    fun canSend(radio: RadioStatus, demoMode: Boolean): Boolean = demoMode || radio.phase == RadioPhase.READY

    /**
     * Whether a user send must be held locally (QUEUED). Kept next to the presentation rules so
     * the UI and repository can be tested against the same decision.
     */
    fun shouldQueueLocally(demoMode: Boolean, phase: RadioPhase): Boolean =
        !demoMode && phase != RadioPhase.READY

    // ---------------------------------------------------------------- radio screen card

    fun phaseCopy(radio: RadioStatus, demoMode: Boolean): PhaseCopy {
        if (demoMode) {
            return PhaseCopy(
                title = DEMO_TITLE,
                subtitle = "Messages are simulated on the phone. Turn demo mode off in Settings to use a real radio.",
                tone = RadioTone.NEUTRAL,
                showSpinner = false,
                stateDescription = "Demo mode, virtual mesh",
            )
        }
        return when (radio.phase) {
            RadioPhase.SCANNING -> PhaseCopy(
                title = "Looking for a radio…",
                subtitle = radio.phaseMessage.ifBlank { "Keep the radio switched on and within a few metres." },
                tone = RadioTone.PROGRESS,
                showSpinner = true,
                stateDescription = "Radio scanning for OpenMesh radios",
            )
            RadioPhase.CONNECTING -> {
                val name = radio.deviceName ?: "the radio"
                PhaseCopy(
                    title = "Connecting to $name…",
                    subtitle = connectingSubtitle(radio),
                    tone = RadioTone.PROGRESS,
                    showSpinner = true,
                    stateDescription = "Radio connecting to $name",
                )
            }
            RadioPhase.HANDSHAKING -> PhaseCopy(
                title = "Radio handshake…",
                subtitle = "Checking that the radio runs compatible OpenMesh firmware. This takes a few seconds.",
                tone = RadioTone.PROGRESS,
                showSpinner = true,
                stateDescription = "Radio handshake in progress",
            )
            RadioPhase.READY -> {
                val stale = radio.health == RadioHealth.STALE
                PhaseCopy(
                    title = if (stale) "$READY_TITLE · not answering" else READY_TITLE,
                    subtitle = if (stale) {
                        "The radio stopped answering. It may be out of range or switched off — tap Reconnect."
                    } else {
                        "Connected over ${radio.transportName} and answering."
                    },
                    tone = if (stale) RadioTone.WARNING else RadioTone.POSITIVE,
                    showSpinner = false,
                    stateDescription = if (stale) {
                        "Radio ready but not answering"
                    } else {
                        "Radio ready over ${radio.transportName}"
                    },
                )
            }
            RadioPhase.ERROR -> PhaseCopy(
                title = "Radio needs attention",
                subtitle = radio.lastError ?: "The radio link could not be set up.",
                tone = RadioTone.ERROR,
                showSpinner = false,
                stateDescription = "Radio error: ${radio.lastError ?: "connection failed"}",
            )
            RadioPhase.NONE -> PhaseCopy(
                title = "No radio connected",
                subtitle = if (radio.queuedMessages > 0) {
                    queuedCardText(radio.queuedMessages)
                } else {
                    "Connect a radio to send and receive messages over the air."
                },
                tone = RadioTone.NEUTRAL,
                showSpinner = false,
                stateDescription = "Radio not connected",
            )
        }
    }

    private fun connectingSubtitle(radio: RadioStatus): String {
        val details = mutableListOf<String>()
        radio.rssi?.let { details += "signal $it dBm" }
        if (radio.devicesFound > 1) details += "${radio.devicesFound} radios found"
        return if (details.isEmpty()) {
            "Hold the phone close to the radio while it connects."
        } else {
            details.joinToString(" · ")
        }
    }

    // ---------------------------------------------------------------- banner

    fun bannerCopy(radio: RadioStatus, demoMode: Boolean): BannerCopy {
        if (demoMode) {
            return BannerCopy(
                title = DEMO_TITLE,
                subtitle = "Messages are simulated — turn demo mode off in Settings to use a radio.",
                tone = RadioTone.NEUTRAL,
            )
        }
        return when (radio.phase) {
            RadioPhase.SCANNING -> BannerCopy(
                title = "Looking for radio…",
                subtitle = radio.phaseMessage.ifBlank { "Keep the radio switched on and nearby." },
                tone = RadioTone.PROGRESS,
            )
            RadioPhase.CONNECTING -> BannerCopy(
                title = "Connecting to ${radio.deviceName ?: "OpenMesh radio"}…",
                subtitle = "Hold the phone close to the radio.",
                tone = RadioTone.PROGRESS,
            )
            RadioPhase.HANDSHAKING -> BannerCopy(
                title = "Radio handshake…",
                subtitle = "Checking the radio firmware and settings.",
                tone = RadioTone.PROGRESS,
            )
            RadioPhase.READY -> {
                val stale = radio.health == RadioHealth.STALE
                BannerCopy(
                    title = if (stale) "Radio ready · not answering" else "Radio ready · ${radio.transportName}",
                    subtitle = if (stale) {
                        "Tap to reconnect."
                    } else {
                        "${radio.profile.replace('_', ' ').lowercase()} · ${radio.region} · " +
                            "${radio.airtimePct.toString().take(4)}% airtime"
                    },
                    tone = if (stale) RadioTone.WARNING else RadioTone.POSITIVE,
                )
            }
            RadioPhase.ERROR -> BannerCopy(
                title = "Radio error — tap to fix",
                subtitle = radio.lastError ?: "The radio link broke. Tap for what to do next.",
                tone = RadioTone.ERROR,
            )
            RadioPhase.NONE -> BannerCopy(
                title = "No radio — tap to connect",
                subtitle = if (radio.queuedMessages > 0) {
                    queuedCardText(radio.queuedMessages)
                } else {
                    "Messages are queued until you connect a radio."
                },
                tone = RadioTone.WARNING,
            )
        }
    }

    // ---------------------------------------------------------------- step rail

    fun stepRail(radio: RadioStatus, demoMode: Boolean): List<RadioStep> {
        if (demoMode) return emptyList()
        val states = when (radio.phase) {
            RadioPhase.NONE -> listOf(
                StepState.PENDING, StepState.PENDING, StepState.PENDING, StepState.PENDING,
            )
            RadioPhase.SCANNING -> listOf(
                StepState.ACTIVE, StepState.PENDING, StepState.PENDING, StepState.PENDING,
            )
            RadioPhase.CONNECTING -> listOf(
                StepState.DONE, StepState.ACTIVE, StepState.PENDING, StepState.PENDING,
            )
            RadioPhase.HANDSHAKING -> listOf(
                StepState.DONE, StepState.DONE, StepState.ACTIVE, StepState.PENDING,
            )
            RadioPhase.READY -> listOf(
                StepState.DONE, StepState.DONE, StepState.DONE, StepState.DONE,
            )
            RadioPhase.ERROR -> when {
                // A link that died after being ready: all setup steps finished, readiness failed.
                radio.lastErrorKind == LinkError.LINK_LOST ->
                    listOf(StepState.DONE, StepState.DONE, StepState.DONE, StepState.FAILED)
                radio.deviceAddress != null ->
                    listOf(StepState.DONE, StepState.DONE, StepState.FAILED, StepState.PENDING)
                else ->
                    listOf(StepState.FAILED, StepState.PENDING, StepState.PENDING, StepState.PENDING)
            }
        }
        return railLabels.zip(states) { label, state -> RadioStep(label, state) }
    }

    // ---------------------------------------------------------------- error card / health

    fun errorCard(radio: RadioStatus): ErrorCopy? {
        if (radio.phase != RadioPhase.ERROR) return null
        return ErrorCopy(
            title = "Could not set up the radio link",
            message = radio.lastError ?: "The radio link broke.",
            hint = radio.lastErrorHint ?: RadioErrorHints.hintFor(LinkError.UNKNOWN),
        )
    }

    fun healthLabel(radio: RadioStatus): String = when (radio.health) {
        RadioHealth.HEALTHY -> "Connected and answering"
        RadioHealth.STALE -> "Not answering"
        RadioHealth.UNKNOWN -> "Waiting for the first reply"
    }

    fun signalSummary(radio: RadioStatus): String =
        "RSSI ${radio.rssi?.let { "$it dBm" } ?: "—"} · SNR ${radio.snr?.let { "$it dB" } ?: "—"}"

    fun elapsedSeconds(radio: RadioStatus, nowMs: Long): Long {
        if (!isRunning(radio.phase)) return 0L
        if (radio.phaseSinceMs <= 0L) return 0L
        return ((nowMs - radio.phaseSinceMs) / 1_000L).coerceAtLeast(0L)
    }

    fun staleAfterMs(): Long = RadioHealthPolicy.STALE_AFTER_MS

    // ---------------------------------------------------------------- queued messages

    fun queuedChipLabel(count: Int): String = "$count queued"

    fun queuedCardText(count: Int): String = if (count == 1) {
        "1 message queued — connect a radio to send"
    } else {
        "$count messages queued — connect a radio to send"
    }

    // ---------------------------------------------------------------- other surfaces

    /** Retry with the transport the last attempt used (USB serial vs BLE). */
    fun retryTransport(transportName: String): RetryTransport =
        if (transportName.contains("USB", ignoreCase = true)) RetryTransport.USB else RetryTransport.BLE

    /** "Nodes" tab header tail. */
    fun nodesSubtitle(radio: RadioStatus, demoMode: Boolean): String {
        if (demoMode) return "demo data (demo mode)"
        return when (radio.phase) {
            RadioPhase.READY -> "listening via ${radio.transportName}"
            RadioPhase.SCANNING, RadioPhase.CONNECTING, RadioPhase.HANDSHAKING -> "waiting for the radio…"
            RadioPhase.ERROR -> "radio error — tap to fix"
            RadioPhase.NONE -> "no radio connected"
        }
    }

    /** Foreground-service notification body. */
    fun notificationText(
        radio: RadioStatus,
        demoMode: Boolean,
        knownNodes: Int,
        engineRunning: Boolean,
    ): String {
        if (demoMode) return DEMO_TITLE
        return when (radio.phase) {
            RadioPhase.READY -> if (radio.health == RadioHealth.STALE) {
                "Radio not answering — tap to reconnect"
            } else {
                "Radio ready · ${radio.transportName} · $knownNodes nodes"
            }
            RadioPhase.SCANNING -> "Looking for radio…"
            RadioPhase.CONNECTING -> "Connecting to the radio…"
            RadioPhase.HANDSHAKING -> "Radio handshake…"
            RadioPhase.ERROR -> "Radio error — tap to fix"
            RadioPhase.NONE -> if (engineRunning) "Mesh running · no radio connected" else "Mesh stopped"
        }
    }
}
