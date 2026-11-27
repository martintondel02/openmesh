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
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
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

    private var manager: OpenMeshBleManager? = null
    private var scope = newScope()

    private fun newScope() = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    override suspend fun connect(): Result<Unit> = runCatching {
        if (_connected.value) return@runCatching
        if (!hasPermissions()) {
            error("Bluetooth permissions are required to connect to the radio")
        }
        val bluetoothAdapter = adapter ?: error("This device has no Bluetooth adapter")
        if (!bluetoothAdapter.isEnabled) error("Bluetooth is turned off")

        val device = scanForRadio(bluetoothAdapter)
            ?: error("No OpenMesh radio found. Power it on, enable BLE, and stay nearby.")

        // BleManager must be created and driven from the main thread (it uses a Handler).
        kotlinx.coroutines.withContext(Dispatchers.Main) {
            val bleManager = OpenMeshBleManager(
                context = context,
                onReady = {
                    _connected.value = true
                },
                onDisconnected = {
                    _connected.value = false
                },
                onData = { data -> _incoming.tryEmit(data) },
            )
            manager = bleManager
            bleManager.connect(device)
                .useAutoConnect(false)
                .timeout(15_000)
                .retry(3, 100)
                .enqueue()
        }
        // Wait for the GATT callback to report a usable NUS service.
        val ready = withTimeoutOrNull(25_000) {
            connected.first { it }
        }
        if (ready != true) error("BLE connection failed (radio out of range or busy)")
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

    private suspend fun scanForRadio(bluetoothAdapter: BluetoothAdapter): BluetoothDevice? =
        withTimeoutOrNull(20_000) {
            suspendCancellableCoroutine { continuation ->
                val scanner = bluetoothAdapter.bluetoothLeScanner
                    ?: return@suspendCancellableCoroutine continuation.resume(null)
                val filter = ScanFilter.Builder()
                    .setServiceUuid(ParcelUuid(OpenMeshBleManager.SERVICE_UUID))
                    .build()
                val settings = ScanSettings.Builder()
                    .setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY)
                    .build()
                var finished = false
                val callback = object : ScanCallback() {
                    override fun onScanResult(callbackType: Int, result: ScanResult) {
                        if (finished) return
                        Log.i(
                            "OpenMeshBle",
                            "scan result addr=${result.device.address} cached=${result.device.name} " +
                                "record=${result.scanRecord?.deviceName} rssi=${result.rssi} uuids=${result.scanRecord?.serviceUuids}",
                        )
                        // The platform filter above already matched the NUS service UUID, so this
                        // result is one of our radios. Do not gate on the advertised name: Android
                        // can return a stale cached BluetoothDevice.name (e.g. a product name from
                        // before a firmware rename) and filtered scan records do not always carry
                        // the scan-response name, which made the app report "No OpenMesh radio
                        // found" even though the radio was right there. First match wins.
                        finished = true
                        runCatching { scanner.stopScan(this) }
                        continuation.resume(result.device)
                    }

                    override fun onScanFailed(errorCode: Int) {
                        Log.w("OpenMeshBle", "scan failed errorCode=$errorCode")
                        if (!finished) {
                            finished = true
                            continuation.resume(null)
                        }
                    }
                }
                Log.i("OpenMeshBle", "scan start prefix='$deviceNamePrefix' filter=${OpenMeshBleManager.SERVICE_UUID}")
                scanner.startScan(listOf(filter), settings, callback)
                continuation.invokeOnCancellation { runCatching { scanner.stopScan(callback) } }
            }
        }

    override suspend fun disconnect() {
        _connected.value = false
        runCatching { manager?.disconnect()?.enqueue() }
        manager = null
        scope.cancel()
        scope = newScope()
    }

    override suspend fun write(bytes: ByteArray) {
        val bleManager = manager ?: error("BLE transport is not connected")
        MeshLog.i("ble write bytes=${bytes.size}")
        try {
            bleManager.write(bytes)
        } catch (e: Exception) {
            MeshLog.w("ble write failed bytes=${bytes.size}: ${e.message}", e)
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

        override fun getGattCallback(): BleManagerGattCallback = object : BleManagerGattCallback() {
            override fun isRequiredServiceSupported(gatt: BluetoothGatt): Boolean {
                val service = gatt.getService(SERVICE_UUID) ?: return false
                rxCharacteristic = service.getCharacteristic(RX_UUID)
                txCharacteristic = service.getCharacteristic(TX_UUID)
                return rxCharacteristic != null && txCharacteristic != null
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
