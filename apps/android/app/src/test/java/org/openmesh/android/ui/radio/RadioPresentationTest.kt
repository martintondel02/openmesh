package org.openmesh.android.ui.radio

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.openmesh.android.data.model.RadioStatus
import org.openmesh.android.data.transport.LinkError
import org.openmesh.android.data.transport.RadioHealth
import org.openmesh.android.data.transport.RadioHealthPolicy
import org.openmesh.android.data.transport.RadioPhase

/**
 * JVM tests for the pure radio-link presentation rules: phase copy, step rail, banner, error
 * card, health staleness, elapsed counter, queued-message decisions and the other status surfaces.
 */
class RadioPresentationTest {

    private fun radio(
        phase: RadioPhase = RadioPhase.NONE,
        connected: Boolean = phase == RadioPhase.READY,
        health: RadioHealth = RadioHealth.UNKNOWN,
        transportName: String = "BLE",
        deviceName: String? = null,
        deviceAddress: String? = null,
        rssi: Int? = null,
        devicesFound: Int = 0,
        lastError: String? = null,
        lastErrorHint: String? = null,
        lastErrorKind: LinkError? = null,
        phaseSinceMs: Long = 0L,
        lastReplyMs: Long = 0L,
        queued: Int = 0,
    ) = RadioStatus(
        connected = connected,
        transportName = transportName,
        phase = phase,
        phaseMessage = when (phase) {
            RadioPhase.SCANNING -> "Looking for OpenMesh radios… (2 found)"
            RadioPhase.CONNECTING -> "Connecting to OpenMesh T1000-E…"
            RadioPhase.HANDSHAKING -> "Radio found. Finishing the secure setup…"
            else -> ""
        },
        deviceName = deviceName,
        deviceAddress = deviceAddress,
        rssi = rssi,
        devicesFound = devicesFound,
        lastError = lastError,
        lastErrorHint = lastErrorHint,
        lastErrorKind = lastErrorKind,
        phaseSinceMs = phaseSinceMs,
        health = health,
        lastReplyMs = lastReplyMs,
        queuedMessages = queued,
    )

    // ------------------------------------------------------------ phase card

    @Test
    fun `demo mode is never shown as an error or as no radio`() {
        val copy = RadioPresentation.phaseCopy(radio(), demoMode = true)
        assertEquals("Demo mode · virtual mesh", copy.title)
        assertTrue(copy.subtitle.contains("simulated"))
        assertEquals(RadioTone.NEUTRAL, copy.tone)
        assertFalse(copy.showSpinner)
        assertTrue(copy.stateDescription.contains("Demo"))
        assertNull(RadioPresentation.errorCard(radio(phase = RadioPhase.NONE)))
    }

    @Test
    fun `every real phase has text title subtitle and a state description`() {
        val phases = listOf(
            RadioPhase.NONE,
            RadioPhase.SCANNING,
            RadioPhase.CONNECTING,
            RadioPhase.HANDSHAKING,
            RadioPhase.READY,
            RadioPhase.ERROR,
        )
        for (phase in phases) {
            val copy = RadioPresentation.phaseCopy(
                radio(phase = phase, lastError = "boom", lastErrorHint = "• try"),
                demoMode = false,
            )
            assertTrue("${phase} title blank", copy.title.isNotBlank())
            assertTrue("${phase} subtitle blank", copy.subtitle.isNotBlank())
            assertTrue("${phase} stateDescription blank", copy.stateDescription.isNotBlank())
        }
    }

    @Test
    fun `scanning and connecting and handshaking show spinner and progress tone`() {
        for (phase in listOf(RadioPhase.SCANNING, RadioPhase.CONNECTING, RadioPhase.HANDSHAKING)) {
            val copy = RadioPresentation.phaseCopy(radio(phase = phase, deviceName = "OpenMesh T1000-E"), demoMode = false)
            assertTrue("${phase} should show a spinner", copy.showSpinner)
            assertEquals(RadioTone.PROGRESS, copy.tone)
        }
    }

    @Test
    fun `connecting copy names the radio and includes signal details`() {
        val copy = RadioPresentation.phaseCopy(
            radio(
                phase = RadioPhase.CONNECTING,
                deviceName = "OpenMesh T1000-E",
                rssi = -52,
                devicesFound = 2,
            ),
            demoMode = false,
        )
        assertEquals("Connecting to OpenMesh T1000-E…", copy.title)
        assertTrue(copy.subtitle.contains("-52 dBm"))
        assertTrue(copy.subtitle.contains("2 radios found"))
    }

    @Test
    fun `ready healthy shows positive tone and transport name`() {
        val copy = RadioPresentation.phaseCopy(
            radio(phase = RadioPhase.READY, health = RadioHealth.HEALTHY, transportName = "BLE"),
            demoMode = false,
        )
        assertEquals("Radio ready", copy.title)
        assertEquals(RadioTone.POSITIVE, copy.tone)
        assertTrue(copy.subtitle.contains("BLE"))
        assertFalse(copy.showSpinner)
    }

    @Test
    fun `ready stale keeps ready but warns and offers reconnect wording`() {
        val copy = RadioPresentation.phaseCopy(
            radio(phase = RadioPhase.READY, health = RadioHealth.STALE),
            demoMode = false,
        )
        assertTrue(copy.title.contains("not answering"))
        assertEquals(RadioTone.WARNING, copy.tone)
        assertTrue(copy.subtitle.contains("Reconnect"))
    }

    @Test
    fun `error phase shows error tone persistent message and hints`() {
        val copy = RadioPresentation.phaseCopy(
            radio(
                phase = RadioPhase.ERROR,
                lastError = "Bluetooth is turned off.",
                lastErrorHint = "• Turn Bluetooth on.",
            ),
            demoMode = false,
        )
        assertEquals(RadioTone.ERROR, copy.tone)
        assertEquals("Bluetooth is turned off.", copy.subtitle)
        val card = RadioPresentation.errorCard(
            radio(phase = RadioPhase.ERROR, lastError = "Bluetooth is turned off.", lastErrorHint = "• Turn Bluetooth on."),
        )
        assertNotNull(card)
        assertEquals("Bluetooth is turned off.", card!!.message)
        assertTrue(card.hint.startsWith("•"))
    }

    @Test
    fun `error card falls back to a generic beginner hint when none stored`() {
        val card = RadioPresentation.errorCard(radio(phase = RadioPhase.ERROR, lastError = "Odd failure"))
        assertNotNull(card)
        assertTrue(card!!.hint.startsWith("•"))
    }

    @Test
    fun `no radio phase mentions queued messages when messages are waiting`() {
        val copy = RadioPresentation.phaseCopy(radio(queued = 3), demoMode = false)
        assertEquals("No radio connected", copy.title)
        assertTrue(copy.subtitle.contains("3 messages queued"))
    }

    // ------------------------------------------------------------ step rail

    @Test
    fun `step rail follows the scan connect handshake ready progression`() {
        fun states(phase: RadioPhase, address: String? = null) =
            RadioPresentation.stepRail(radio(phase = phase, deviceAddress = address), demoMode = false).map { it.state }

        assertEquals(listOf(StepState.PENDING, StepState.PENDING, StepState.PENDING, StepState.PENDING), states(RadioPhase.NONE))
        assertEquals(listOf(StepState.ACTIVE, StepState.PENDING, StepState.PENDING, StepState.PENDING), states(RadioPhase.SCANNING))
        assertEquals(listOf(StepState.DONE, StepState.ACTIVE, StepState.PENDING, StepState.PENDING), states(RadioPhase.CONNECTING))
        assertEquals(listOf(StepState.DONE, StepState.DONE, StepState.ACTIVE, StepState.PENDING), states(RadioPhase.HANDSHAKING))
        assertEquals(listOf(StepState.DONE, StepState.DONE, StepState.DONE, StepState.DONE), states(RadioPhase.READY))
    }

    @Test
    fun `step rail error marks the failing step`() {
        val noDevice = RadioPresentation.stepRail(radio(phase = RadioPhase.ERROR), demoMode = false)
        assertEquals(StepState.FAILED, noDevice.first().state)

        val afterConnect = RadioPresentation.stepRail(
            radio(phase = RadioPhase.ERROR, deviceAddress = "AA:BB:CC:DD:EE:FF"),
            demoMode = false,
        )
        assertEquals(StepState.DONE, afterConnect[0].state)
        assertEquals(StepState.DONE, afterConnect[1].state)
        assertEquals(StepState.FAILED, afterConnect[2].state)
    }

    @Test
    fun `step rail marks ready as failed when an established link was lost`() {
        val states = RadioPresentation.stepRail(
            radio(
                phase = RadioPhase.ERROR,
                lastError = "Lost connection to the radio.",
                deviceAddress = "AA:BB:CC:DD:EE:FF",
                lastErrorKind = LinkError.LINK_LOST,
            ),
            demoMode = false,
        ).map { it.state }
        assertEquals(listOf(StepState.DONE, StepState.DONE, StepState.DONE, StepState.FAILED), states)
    }

    @Test
    fun `step rail is hidden in demo mode`() {
        assertTrue(RadioPresentation.stepRail(radio(), demoMode = true).isEmpty())
    }

    // ------------------------------------------------------------ banner

    @Test
    fun `banner copy is phase aware`() {
        val cases = mapOf(
            RadioPhase.SCANNING to "Looking for radio…",
            RadioPhase.HANDSHAKING to "Radio handshake…",
            RadioPhase.ERROR to "Radio error — tap to fix",
        )
        for ((phase, title) in cases) {
            assertEquals(title, RadioPresentation.bannerCopy(radio(phase = phase, lastError = "x"), demoMode = false).title)
        }
        assertEquals(
            "No radio — tap to connect",
            RadioPresentation.bannerCopy(radio(), demoMode = false).title,
        )
        assertEquals(
            "Radio ready · BLE",
            RadioPresentation.bannerCopy(radio(phase = RadioPhase.READY, health = RadioHealth.HEALTHY), demoMode = false).title,
        )
    }

    @Test
    fun `banner connecting includes the radio name`() {
        val banner = RadioPresentation.bannerCopy(
            radio(phase = RadioPhase.CONNECTING, deviceName = "OpenMesh T1000-E"),
            demoMode = false,
        )
        assertEquals("Connecting to OpenMesh T1000-E…", banner.title)
    }

    @Test
    fun `banner shows demo mode instead of no radio`() {
        val banner = RadioPresentation.bannerCopy(radio(), demoMode = true)
        assertEquals("Demo mode · virtual mesh", banner.title)
        assertEquals(RadioTone.NEUTRAL, banner.tone)
    }

    @Test
    fun `banner queued subtitle is honest when no radio is connected`() {
        val banner = RadioPresentation.bannerCopy(radio(queued = 2), demoMode = false)
        assertTrue(banner.subtitle.contains("2 messages queued"))
    }

    // ------------------------------------------------------------ health / elapsed

    @Test
    fun `health policy marks a silent ready link stale and a fresh one healthy`() {
        val now = 10_000_000L
        assertEquals(RadioHealth.UNKNOWN, RadioHealthPolicy.compute(RadioPhase.NONE, now, now))
        assertEquals(RadioHealth.STALE, RadioHealthPolicy.compute(RadioPhase.READY, 0L, now))
        assertEquals(
            RadioHealth.HEALTHY,
            RadioHealthPolicy.compute(RadioPhase.READY, now - 5_000L, now),
        )
        assertEquals(
            RadioHealth.HEALTHY,
            RadioHealthPolicy.compute(RadioPhase.READY, now - RadioHealthPolicy.STALE_AFTER_MS, now),
        )
        assertEquals(
            RadioHealth.STALE,
            RadioHealthPolicy.compute(RadioPhase.READY, now - RadioHealthPolicy.STALE_AFTER_MS - 1, now),
        )
    }

    @Test
    fun `health label never says connected for unknown health`() {
        assertEquals("Waiting for the first reply", RadioPresentation.healthLabel(radio()))
        assertEquals("Not answering", RadioPresentation.healthLabel(radio(health = RadioHealth.STALE)))
        assertEquals("Connected and answering", RadioPresentation.healthLabel(radio(health = RadioHealth.HEALTHY)))
    }

    @Test
    fun `elapsed seconds only counts while an attempt is in progress`() {
        val now = 1_000_000L
        assertEquals(0L, RadioPresentation.elapsedSeconds(radio(phase = RadioPhase.READY, phaseSinceMs = now - 9_000), now))
        assertEquals(0L, RadioPresentation.elapsedSeconds(radio(phase = RadioPhase.SCANNING, phaseSinceMs = 0L), now))
        assertEquals(4L, RadioPresentation.elapsedSeconds(radio(phase = RadioPhase.SCANNING, phaseSinceMs = now - 4_500), now))
        assertEquals(9L, RadioPresentation.elapsedSeconds(radio(phase = RadioPhase.CONNECTING, phaseSinceMs = now), now + 9_400))
    }

    // ------------------------------------------------------------ queued decisions

    @Test
    fun `send is queued unless a real link is ready (demo never queues)`() {
        assertFalse(RadioPresentation.shouldQueueLocally(demoMode = true, phase = RadioPhase.NONE))
        assertTrue(RadioPresentation.shouldQueueLocally(demoMode = false, phase = RadioPhase.NONE))
        assertTrue(RadioPresentation.shouldQueueLocally(demoMode = false, phase = RadioPhase.SCANNING))
        assertTrue(RadioPresentation.shouldQueueLocally(demoMode = false, phase = RadioPhase.ERROR))
        assertFalse(RadioPresentation.shouldQueueLocally(demoMode = false, phase = RadioPhase.READY))
        assertTrue(RadioPresentation.canSend(radio(phase = RadioPhase.READY), demoMode = false))
        assertTrue(RadioPresentation.canSend(radio(), demoMode = true))
        assertFalse(RadioPresentation.canSend(radio(), demoMode = false))
    }

    @Test
    fun `queued copy is grammatical for one and many`() {
        assertEquals("1 message queued — connect a radio to send", RadioPresentation.queuedCardText(1))
        assertEquals("2 messages queued — connect a radio to send", RadioPresentation.queuedCardText(2))
        assertEquals("2 queued", RadioPresentation.queuedChipLabel(2))
    }

    // ------------------------------------------------------------ other surfaces

    @Test
    fun `retry picks the transport from the last attempt`() {
        assertEquals(RetryTransport.USB, RadioPresentation.retryTransport("USB serial"))
        assertEquals(RetryTransport.BLE, RadioPresentation.retryTransport("BLE"))
        assertEquals(RetryTransport.BLE, RadioPresentation.retryTransport("none"))
    }

    @Test
    fun `nodes subtitle reflects the phase and demo mode`() {
        assertEquals("demo data (demo mode)", RadioPresentation.nodesSubtitle(radio(), demoMode = true))
        assertEquals("no radio connected", RadioPresentation.nodesSubtitle(radio(), demoMode = false))
        assertEquals(
            "listening via BLE",
            RadioPresentation.nodesSubtitle(radio(phase = RadioPhase.READY), demoMode = false),
        )
        assertEquals(
            "waiting for the radio…",
            RadioPresentation.nodesSubtitle(radio(phase = RadioPhase.SCANNING), demoMode = false),
        )
        assertEquals(
            "radio error — tap to fix",
            RadioPresentation.nodesSubtitle(radio(phase = RadioPhase.ERROR), demoMode = false),
        )
    }

    @Test
    fun `notification text is phase aware`() {
        assertEquals(
            "Demo mode · virtual mesh",
            RadioPresentation.notificationText(radio(), demoMode = true, knownNodes = 3, engineRunning = true),
        )
        assertEquals(
            "Radio ready · BLE · 3 nodes",
            RadioPresentation.notificationText(radio(phase = RadioPhase.READY), demoMode = false, knownNodes = 3, engineRunning = true),
        )
        assertEquals(
            "Radio not answering — tap to reconnect",
            RadioPresentation.notificationText(
                radio(phase = RadioPhase.READY, health = RadioHealth.STALE),
                demoMode = false,
                knownNodes = 1,
                engineRunning = true,
            ),
        )
        assertEquals(
            "Mesh running · no radio connected",
            RadioPresentation.notificationText(radio(), demoMode = false, knownNodes = 0, engineRunning = true),
        )
        assertEquals(
            "Mesh stopped",
            RadioPresentation.notificationText(radio(), demoMode = false, knownNodes = 0, engineRunning = false),
        )
        assertEquals(
            "Radio error — tap to fix",
            RadioPresentation.notificationText(radio(phase = RadioPhase.ERROR), demoMode = false, knownNodes = 0, engineRunning = true),
        )
    }

    @Test
    fun `error card is built from the stored beginner hints`() {
        val hint = org.openmesh.android.data.transport.RadioErrorHints.hintFor(LinkError.BLUETOOTH_OFF)
        val card = RadioPresentation.errorCard(
            radio(phase = RadioPhase.ERROR, lastError = "Bluetooth is turned off.", lastErrorHint = hint),
        )
        assertEquals(hint, card!!.hint)
    }
}
