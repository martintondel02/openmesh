package org.openmesh.android.data.model

/**
 * Deterministic demo content so every screen is reviewable without a radio or bridge.
 *
 * The repository prefers real companion/bridge state when connected; demo data is only used
 * when no transport is available (the default on a fresh install, matching the web client's
 * demo mode). All ids/timestamps are synthetic.
 */
object DemoData {
    private const val BASE_MS = 1_760_000_000_000L

    val selfId = "a1b2c3d4e5f60718"
    val contacts: List<Contact> = listOf(
        Contact("11aa22bb33cc44dd", "Alice", "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=", BASE_MS, verified = true),
        Contact("55ee66ff77880099", "Bob", "ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8=", BASE_MS, verified = false),
        Contact("c0ffee00c0ffee00", "Ridge repeater", "QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1xdXl8=", BASE_MS, verified = true),
        Contact("deadbeefdeadbeef", "NomadNet bridge", "YGFiY2RlZmdoaWprbG1ub3BxcnN0dXZ3eHl6e3x9fn8=", BASE_MS, bridged = true),
    )

    val nodes: List<NodeInfo> = listOf(
        NodeInfo(selfId, "openmesh-android", NodeClass.CLIENT, setOf("RELAY", "USB", "BLE", "GPS"),
            hops = 0, lastSeenMs = BASE_MS, rssi = -48, snr = 9.5f, batteryPct = 86,
            position = Position(59.9137, 10.7522, altM = 45, accuracyM = 8, timestampMs = BASE_MS), isContact = true),
        NodeInfo("11aa22bb33cc44dd", "Alice", NodeClass.CLIENT, setOf("RELAY", "BLE", "GPS"),
            hops = 1, lastSeenMs = BASE_MS - 90_000, rssi = -87, snr = 6.25f, batteryPct = 64,
            position = Position(59.9210, 10.7405, altM = 120, accuracyM = 12, timestampMs = BASE_MS), isContact = true),
        NodeInfo("c0ffee00c0ffee00", "Ridge repeater", NodeClass.REPEATER, setOf("RELAY", "STORE", "SOLAR", "GPS"),
            hops = 2, lastSeenMs = BASE_MS - 600_000, rssi = -104, snr = 2.0f, batteryPct = 100,
            position = Position(60.0120, 10.8100, altM = 612, accuracyM = 20, timestampMs = BASE_MS)),
        NodeInfo("55ee66ff77880099", "Bob", NodeClass.CLIENT, setOf("RELAY", "GPS"),
            hops = 2, lastSeenMs = BASE_MS - 1_800_000, rssi = -112, snr = -3.5f, batteryPct = 41,
            position = Position(59.8800, 10.7000, altM = 20, accuracyM = 30, timestampMs = BASE_MS)),
        NodeInfo("0badc0de0badc0de", "Valley gateway", NodeClass.GATEWAY, setOf("RELAY", "WIFI", "ETHERNET", "STORE"),
            hops = 1, lastSeenMs = BASE_MS - 3_600_000, rssi = -76, snr = 8.0f, batteryPct = null),
        NodeInfo("deadbeefdeadbeef", "NomadNet bridge", NodeClass.GATEWAY, setOf("RELAY", "ETHERNET"),
            hops = 3, lastSeenMs = BASE_MS - 7_200_000, rssi = -118, snr = -6.0f),
    )

    val channels: List<Channel> = listOf(
        Channel("ff00ff00ff00ff00", "family", unreadCount = 2),
        Channel("abc123abc123abc1", "trail crew", unreadCount = 0),
        Channel("5a5a5a5a5a5a5a5a", "openmesh-dev", unreadCount = 7),
    )

    val messages: Map<String, List<Message>> = mapOf(
        "11aa22bb33cc44dd" to listOf(
            Message(
                id = 1001, peerId = "11aa22bb33cc44dd", fromId = "11aa22bb33cc44dd", toId = selfId,
                kind = MessageKind.TEXT, text = "At the trailhead. Repeater on the ridge is alive.",
                sentAtMs = BASE_MS - 900_000, receivedAtMs = BASE_MS - 899_400, delivery = DeliveryState.DELIVERED,
            ),
            Message(
                id = 1002, peerId = "11aa22bb33cc44dd", fromId = selfId, toId = "11aa22bb33cc44dd",
                kind = MessageKind.TEXT, text = "Copy. I'll be there in ~40 min. Bringing the solar node.",
                sentAtMs = BASE_MS - 840_000, outgoing = true, delivery = DeliveryState.DELIVERED, read = true,
            ),
            Message(
                id = 1003, peerId = "11aa22bb33cc44dd", fromId = "11aa22bb33cc44dd", toId = selfId,
                kind = MessageKind.POSITION, text = "Shared a position",
                sentAtMs = BASE_MS - 600_000, receivedAtMs = BASE_MS - 599_000,
                position = Position(59.9210, 10.7405, altM = 120, accuracyM = 12, timestampMs = BASE_MS - 600_000),
                delivery = DeliveryState.DELIVERED,
            ),
            Message(
                id = 1004, peerId = "11aa22bb33cc44dd", fromId = "11aa22bb33cc44dd", toId = selfId,
                kind = MessageKind.ATTACHMENT, text = "Ridge from the north side",
                sentAtMs = BASE_MS - 300_000, receivedAtMs = BASE_MS - 240_000,
                attachment = Attachment("att_demo01", "image/jpeg", 5120, durationMs = null, name = "ridge.jpg"),
                delivery = DeliveryState.DELIVERED,
            ),
            Message(
                id = 1005, peerId = "11aa22bb33cc44dd", fromId = "11aa22bb33cc44dd", toId = selfId,
                kind = MessageKind.TEXT, text = "Heads up: the repeater held Bob's message for two days.",
                sentAtMs = BASE_MS - 120_000, receivedAtMs = BASE_MS - 60_000, viaStore = true,
                delivery = DeliveryState.DELIVERED,
            ),
        ),
        "55ee66ff77880099" to listOf(
            Message(
                id = 2001, peerId = "55ee66ff77880099", fromId = selfId, toId = "55ee66ff77880099",
                kind = MessageKind.TEXT, text = "Sending you the water source pin once I have a fix.",
                sentAtMs = BASE_MS - 7_200_000, outgoing = true, delivery = DeliveryState.STORED_AT_RELAY,
            ),
            Message(
                id = 2002, peerId = "55ee66ff77880099", fromId = "55ee66ff77880099", toId = selfId,
                kind = MessageKind.TEXT, text = "Got it. Signal is poor here, sending from the ridge tomorrow.",
                sentAtMs = BASE_MS - 86_400_000, receivedAtMs = BASE_MS - 82_800_000,
                delivery = DeliveryState.DELIVERED, viaStore = true,
            ),
        ),
        "deadbeefdeadbeef" to listOf(
            Message(
                id = 3001, peerId = "deadbeefdeadbeef", fromId = selfId, toId = "deadbeefdeadbeef",
                kind = MessageKind.TEXT, text = "Bridge test: hello from the OpenMesh side.",
                sentAtMs = BASE_MS - 3_600_000, outgoing = true, delivery = DeliveryState.DELIVERED, bridged = true,
            ),
            Message(
                id = 3002, peerId = "deadbeefdeadbeef", fromId = "deadbeefdeadbeef", toId = selfId,
                kind = MessageKind.TEXT, text = "Received via LXMF. The NomadNet page fetch is queued.",
                sentAtMs = BASE_MS - 3_500_000, receivedAtMs = BASE_MS - 3_400_000,
                delivery = DeliveryState.DELIVERED, bridged = true,
            ),
        ),
        "c0ffee00c0ffee00" to listOf(
            Message(
                id = 4001, peerId = "c0ffee00c0ffee00", fromId = selfId, toId = "c0ffee00c0ffee00",
                kind = MessageKind.TELEMETRY, text = "Repeater telemetry",
                sentAtMs = BASE_MS - 600_000, outgoing = true, delivery = DeliveryState.SENT,
            ),
        ),
    )

    val channelMessages: Map<String, List<Message>> = mapOf(
        "ff00ff00ff00ff00" to listOf(
            Message(
                id = 5001, peerId = "ff00ff00ff00ff00", fromId = "11aa22bb33cc44dd", toId = "ff00ff00ff00ff00",
                kind = MessageKind.TEXT, text = "Trail is clear up to the lake. Meeting at the cabin at 18:00.",
                sentAtMs = BASE_MS - 1_200_000, receivedAtMs = BASE_MS - 1_190_000, delivery = DeliveryState.DELIVERED,
            ),
            Message(
                id = 5002, peerId = "ff00ff00ff00ff00", fromId = selfId, toId = "ff00ff00ff00ff00",
                kind = MessageKind.TEXT, text = "+1, I'll bring the first aid kit.",
                sentAtMs = BASE_MS - 1_100_000, outgoing = true, delivery = DeliveryState.DELIVERED, read = true,
            ),
            Message(
                id = 5003, peerId = "ff00ff00ff00ff00", fromId = "55ee66ff77880099", toId = "ff00ff00ff00ff00",
                kind = MessageKind.TEXT, text = "Running late, don't wait for dinner.",
                sentAtMs = BASE_MS - 300_000, receivedAtMs = BASE_MS - 290_000, delivery = DeliveryState.DELIVERED,
            ),
        ),
    )

    val settings: AppSettings = AppSettings(
        nodeName = "openmesh-android",
        profile = "MEDIUM_FAST",
        region = "EU868",
        announceIntervalSec = 1800,
        positionSharing = true,
        storeForward = true,
        autoConnect = true,
        preferUsb = true,
        showAirtimeEstimates = true,
        demoMode = true,
    )

    val radioStatus: RadioStatus = RadioStatus(
        connected = false,
        transportName = "demo mode",
        firmwareVersion = "0.1.0-demo",
        nodeId = selfId,
        region = "EU868",
        profile = "MEDIUM_FAST",
        txQueue = 0,
        airtimePct = 0.4f,
        rssi = -87,
        snr = 6.5f,
    )
}
