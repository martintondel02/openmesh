package org.openmesh.android.data.transport

import android.annotation.SuppressLint
import android.util.Log

import android.Manifest
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothManager
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.ParcelUuid
import androidx.core.content.ContextCompat
import org.openmesh.android.util.MeshLog
import java.util.UUID
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import no.nordicsemi.android.ble.BleManager
import kotlin.coroutines.resume

/**
 * BLE companion transport using a Nordic-UART-like service (NUS UUIDs), matching the
 * companion API's stream framing. Scanning uses the platform [android.bluetooth.le.BluetoothLeScanner];
 * the GATT connection uses the Nordic Android-BLE-Library because it handles MTU negotiation,
 * notification queues and automatic write splitting correctly.
 *
 * UX: publishes a [LinkProgress] stream (scanning → connecting → handshaking) and maps every
 * failure (Bluetooth off, permissions, nothing found, GATT 133/timeout, missing service, lost
 * link) to a [LinkError] so the repository can show a persistent, plain-language error card.
 *
 * Service/characteristic UUIDs and MTU guidance: docs/architecture/COMPANION-API.md §1.
 */
@SuppressLint("MissingPermission")
class BleTransport(
    private val context: Context,
    private val deviceNamePrefix: String = "OpenMesh",
) : CompanionTransport {

    override val label: String = "BLE"

    private val bluetoothManager: BluetoothManager? =
        context.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager
    private val adapter: BluetoothAdapter? get() = bluetoothManager?.adapter

    private val _connected = MutableStateFlow(false)
    override val connected: StateFlow<Boolean> = _connected.asStateFlow()

    private val _incoming = MutableSharedFlow<ByteArray>(extraBufferCapacity = 64)
    override val incoming: Flow<ByteArray> = _incoming.asSharedFlow()

    private val _progress = MutableStateFlow(LinkProgress())
    override val progress: StateFlow<LinkProgress> = _progress.asStateFlow()

    private var manager: OpenMeshBleManager? = null
    private var scope = newScope()
    private var attempt = 0
    /** True between [disconnect] and the next [connect]: suppresses spurious "link lost" errors. */
    private var userClosing = false

    private fun newScope() = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    companion object {
        /** How long to listen for advertising radios before picking the strongest one. */
        const val SCAN_WINDOW_MS: Long = 5_000L
    }

    override suspend fun connect(): Result<Unit> = runCatching {
        connectInternal()
    }.onFailure { error ->
        MeshLog.w("ble connect failed: ${error.message}")
        // If the failure did not come with a more specific reason, surface a generic error card.
        if (_progress.value.phase != RadioPhase.ERROR) {
            setProgress(
                RadioPhase.ERROR,
                error.message ?: "Could not connect to the radio.",
                error = LinkError.UNKNOWN,
            )
        }
    }

    private suspend fun connectInternal() {
        if (_connected.value) return
        userClosing = false
        attempt += 1
        if (!hasPermissions()) {
            throw failure(
                LinkError.BLUETOOTH_PERMISSION,
                "Bluetooth permission is needed to find the radio.",
            )
        }
        val bluetoothAdapter = adapter
            ?: throw failure(LinkError.UNKNOWN, "This phone has no Bluetooth adapter.")
        if (!bluetoothAdapter.isEnabled) {
            throw failure(LinkError.BLUETOOTH_OFF, "Bluetooth is turned off.")
        }

        setProgress(RadioPhase.SCANNING, "Looking for OpenMesh radios…", devicesFound = 0)
        val scan = scanForRadio(bluetoothAdapter)
        if (scan.devices.isEmpty()) {
            if (scan.failed) {
                throw failure(LinkError.SCAN_FAILED, "Bluetooth scanning failed. Turn Bluetooth off and on again.")
            } else {
                throw failure(LinkError.NO_RADIO_FOUND, "No OpenMesh radio found. Check it is powered on and nearby.")
            }
        }
        // Strongest RSSI wins if more than one radio is advertising.
        val best = scan.devices.maxByOrNull { it.rssi } ?: scan.devices.first()
        // Prefer the fresh scan-record name: BluetoothDevice.name can be a stale cached product
        // name (e.g. "Seeed SenseCAP T1000-E") while the radio advertises "OpenMesh T1000-E".
        val name = best.scanRecord?.deviceName?.takeIf { it.isNotBlank() }
            ?: best.device.name?.takeIf { it.isNotBlank() }
            ?: "OpenMesh radio"
        val address = best.device.address
        val rssi = best.rssi
        MeshLog.i("ble scan: ${scan.devices.size} radio(s); best=$name $address rssi=$rssi")
        setProgress(
            RadioPhase.CONNECTING,
            "Connecting to $name…",
            deviceName = name,
            deviceAddress = address,
            rssi = rssi,
            devicesFound = scan.devices.size,
        )

        // BleManager must be created and driven from the main thread (it uses a Handler).
        val outcome = CompletableDeferred<Result<Unit>>()
        withContext(Dispatchers.Main) {
            val bleManager = OpenMeshBleManager(
                context = context,
                onReady = {
                    _connected.value = true
                    setProgress(
                        RadioPhase.HANDSHAKING,
                        "Radio found. Finishing the secure setup…",
                        deviceName = name,
                        deviceAddress = address,
                        rssi = rssi,
                        devicesFound = scan.devices.size,
                    )
                    outcome.complete(Result.success(Unit))
                },
                onDisconnected = {
                    _connected.value = false
                    if (!outcome.isCompleted) {
                        outcome.complete(
                            Result.failure(IllegalStateException("The BLE connection ended before the radio was ready.")),
                        )
                    } else if (!userClosing) {
                        setProgress(
                            RadioPhase.ERROR,
                            "Lost connection to the radio. Move closer and try again.",
                            deviceName = name,
                            deviceAddress = address,
                            rssi = rssi,
                            error = LinkError.LINK_LOST,
                        )
                    }
                },
                onData = { data -> _incoming.tryEmit(data) },
            )
            manager = bleManager
            bleManager.connect(best.device)
                .useAutoConnect(false)
                .timeout(15_000)
                .retry(3, 100)
                .fail { _, status ->
                    val error = if (bleManager.serviceSupported) LinkError.GATT_FAILED else LinkError.SERVICE_MISSING
                    val message = when (error) {
                        LinkError.SERVICE_MISSING ->
                            "This device does not expose the OpenMesh radio service (firmware mismatch?)."
                        else ->
                            "The radio did not accept the connection (code $status). Move closer and try again."
                    }
                    Log.w("OpenMeshBle", "gatt connect failed status=$status serviceSupported=${bleManager.serviceSupported}")
                    setProgress(
                        RadioPhase.ERROR,
                        message,
                        deviceName = name,
                        deviceAddress = address,
                        rssi = rssi,
                        error = error,
                    )
                    outcome.complete(Result.failure(IllegalStateException(message)))
                }
                .enqueue()
        }
        val result = withTimeoutOrNull(30_000) { outcome.await() }
            ?: throw failure(LinkError.GATT_FAILED, "The radio did not answer in time. Move closer and try again.")
        result.getOrThrow()
    }

    private fun failure(kind: LinkError, message: String): IllegalStateException {
        setProgress(
            RadioPhase.ERROR,
            message,
            error = kind,
            deviceName = _progress.value.deviceName,
            deviceAddress = _progress.value.deviceAddress,
            rssi = _progress.value.rssi,
        )
        return IllegalStateException(message)
    }

    private fun setProgress(
        phase: RadioPhase,
        message: String,
        deviceName: String? = _progress.value.deviceName,
        deviceAddress: String? = _progress.value.deviceAddress,
        rssi: Int? = _progress.value.rssi,
        devicesFound: Int = _progress.value.devicesFound,
        error: LinkError? = null,
    ) {
        val previous = _progress.value
        _progress.value = LinkProgress(
            phase = phase,
            message = message,
            deviceName = deviceName,
            deviceAddress = deviceAddress,
            rssi = rssi,
            devicesFound = devicesFound,
            attempt = attempt,
            error = error,
            sinceMs = if (previous.phase == phase && previous.sinceMs > 0L) previous.sinceMs else System.currentTimeMillis(),
        )
    }

    private fun hasPermissions(): Boolean {
        val needed = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            listOf(Manifest.permission.BLUETOOTH_SCAN, Manifest.permission.BLUETOOTH_CONNECT)
        } else {
            listOf(Manifest.permission.ACCESS_FINE_LOCATION)
        }
        return needed.all {
            ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED
        }
    }

    private data class ScanOutcome(val devices: List<ScanResult>, val failed: Boolean)

    /**
     * Collects every NUS radio in range for up to [SCAN_WINDOW_MS] so the strongest one is picked,
     * instead of connecting to whichever device the stack reports first.
     */
    private suspend fun scanForRadio(bluetoothAdapter: BluetoothAdapter): ScanOutcome =
        withTimeoutOrNull(20_000) {
            suspendCancellableCoroutine { continuation ->
                val scanner = bluetoothAdapter.bluetoothLeScanner
                    ?: return@suspendCancellableCoroutine continuation.resume(ScanOutcome(emptyList(), failed = true))
                val filter = ScanFilter.Builder()
                    .setServiceUuid(ParcelUuid(OpenMeshBleManager.SERVICE_UUID))
                    .build()
                val settings = ScanSettings.Builder()
                    .setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY)
                    .build()
                val lock = Any()
                val results = LinkedHashMap<String, ScanResult>()
                var finished = false
                var timer: Job? = null

                fun snapshot(): ScanOutcome = synchronized(lock) {
                    ScanOutcome(results.values.toList(), failed = false)
                }

                var callback: ScanCallback? = null
                fun finish(outcome: ScanOutcome) {
                    val shouldResume = synchronized(lock) {
                        if (finished) {
                            false
                        } else {
                            finished = true
                            true
                        }
                    }
                    if (!shouldResume) return
                    timer?.cancel()
                    runCatching { callback?.let { scanner.stopScan(it) } }
                    continuation.resume(outcome)
                }

                val scanCallback = object : ScanCallback() {
                    override fun onScanResult(callbackType: Int, result: ScanResult) {
                        val count = synchronized(lock) {
                            if (finished) return
                            results[result.device.address] = result
                            results.size
                        }
                        Log.i(
                            "OpenMeshBle",
                            "scan result addr=${result.device.address} cached=${result.device.name} " +
                                "record=${result.scanRecord?.deviceName} rssi=${result.rssi} uuids=${result.scanRecord?.serviceUuids}",
                        )
                        // The platform filter already matched the NUS service UUID; do not gate on
                        // the advertised name (Android can return a stale cached name, and filtered
                        // scan records do not always carry the scan-response name).
                        _progress.value = _progress.value.copy(
                            message = "Looking for OpenMesh radios… ($count found)",
                            sinceMs = _progress.value.sinceMs,
                        )
                    }

                    override fun onScanFailed(errorCode: Int) {
                        Log.w("OpenMeshBle", "scan failed errorCode=$errorCode")
                        finish(ScanOutcome(emptyList(), failed = true))
                    }
                }
                callback = scanCallback
                Log.i("OpenMeshBle", "scan start prefix='$deviceNamePrefix' filter=${OpenMeshBleManager.SERVICE_UUID}")
                scanner.startScan(listOf(filter), settings, scanCallback)
                timer = scope.launch {
                    delay(SCAN_WINDOW_MS)
                    finish(snapshot())
                }
                continuation.invokeOnCancellation {
                    timer?.cancel()
                    synchronized(lock) { finished = true }
                    callback?.let { runCatching { scanner.stopScan(it) } }
                }
            }
        } ?: ScanOutcome(emptyList(), failed = true)

    override suspend fun disconnect() {
        userClosing = true
        _connected.value = false
        runCatching { manager?.disconnect()?.enqueue() }
        manager = null
        scope.cancel()
        scope = newScope()
        _progress.value = LinkProgress()
    }

    override suspend fun write(bytes: ByteArray) {
        val bleManager = manager ?: error("BLE transport is not connected")
        MeshLog.i("ble write bytes=${bytes.size}")
        try {
            bleManager.write(bytes)
        } catch (e: Exception) {
            MeshLog.w("ble write failed bytes=${bytes.size}: ${e.message}", e)
            setProgress(
                RadioPhase.ERROR,
                "Lost connection to the radio. Reconnect to keep sending.",
                error = LinkError.LINK_LOST,
            )
            throw e
        }
    }

    /** GATT client for the companion NUS service. */
    private class OpenMeshBleManager(
        context: Context,
        private val onReady: () -> Unit,
        private val onDisconnected: () -> Unit,
        private val onData: (ByteArray) -> Unit,
    ) : BleManager(context) {

        companion object {
            val SERVICE_UUID: UUID = UUID.fromString("6E400001-B5A3-F393-E0A9-E50E24DCCA9E")
            val RX_UUID: UUID = UUID.fromString("6E400002-B5A3-F393-E0A9-E50E24DCCA9E")
            val TX_UUID: UUID = UUID.fromString("6E400003-B5A3-F393-E0A9-E50E24DCCA9E")
        }

        private var rxCharacteristic: BluetoothGattCharacteristic? = null
        private var txCharacteristic: BluetoothGattCharacteristic? = null

        /** True once a GATT connection reached service discovery and found the NUS service. */
        var serviceSupported: Boolean = false
            private set

        override fun getGattCallback(): BleManagerGattCallback = object : BleManagerGattCallback() {
            override fun isRequiredServiceSupported(gatt: BluetoothGatt): Boolean {
                val service = gatt.getService(SERVICE_UUID) ?: return false
                rxCharacteristic = service.getCharacteristic(RX_UUID)
                txCharacteristic = service.getCharacteristic(TX_UUID)
                serviceSupported = rxCharacteristic != null && txCharacteristic != null
                return serviceSupported
            }

            override fun initialize() {
                val tx = txCharacteristic ?: return
                setNotificationCallback(tx).with { _, data ->
                    data.value?.let(onData)
                }
                enableNotifications(tx).enqueue()
                requestMtu(247).enqueue()
                onReady()
            }

            override fun onServicesInvalidated() {
                rxCharacteristic = null
                txCharacteristic = null
            }

            override fun onDeviceDisconnected() {
                onDisconnected()
            }
        }

        fun write(bytes: ByteArray) {
            val rx = rxCharacteristic ?: return
            writeCharacteristic(rx, bytes).split().enqueue()
        }
    }
}
