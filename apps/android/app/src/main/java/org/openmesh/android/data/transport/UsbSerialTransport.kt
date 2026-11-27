package org.openmesh.android.data.transport

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbManager
import androidx.core.content.ContextCompat
import com.hoho.android.usbserial.driver.UsbSerialDriver
import com.hoho.android.usbserial.driver.UsbSerialPort
import com.hoho.android.usbserial.driver.UsbSerialProber
import java.io.IOException
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
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import kotlin.coroutines.resume

/**
 * USB CDC-ACM/CP210x/CH340/FTDI companion transport using usb-serial-for-android.
 *
 * The radio runs the companion API over its USB serial port at 115200 8N1. The Android USB
 * host permission prompt is handled here; the app's foreground service keeps the connection
 * alive. Frame boundaries are recovered by [SlipFraming.Decoder] in [FramedCompanionClient].
 *
 * UX: publishes [LinkProgress] (CONNECTING while the port opens, HANDSHAKING once it is open and
 * the companion handshake is pending) and maps no-device / denied-permission / port-busy /
 * cable-pulled failures to a [LinkError] for the persistent error card.
 */
class UsbSerialTransport(
    private val context: Context,
    private val preferredDeviceName: String? = null,
) : CompanionTransport {

    override val label: String = "USB serial"

    private val manager: UsbManager =
        context.getSystemService(Context.USB_SERVICE) as UsbManager

    private val _connected = MutableStateFlow(false)
    override val connected: StateFlow<Boolean> = _connected.asStateFlow()

    private val _incoming = MutableSharedFlow<ByteArray>(extraBufferCapacity = 64)
    override val incoming: Flow<ByteArray> = _incoming.asSharedFlow()

    private val _progress = MutableStateFlow(LinkProgress())
    override val progress: StateFlow<LinkProgress> = _progress.asStateFlow()

    private var port: UsbSerialPort? = null
    private var readJob: Job? = null
    private var scope = newScope()
    /** True while [disconnect] is closing the port, so the read loop does not report a cable loss. */
    private var userClosing = false

    private fun newScope() = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    override suspend fun connect(): Result<Unit> = runCatching {
        if (_connected.value) return@runCatching
        userClosing = false
        setProgress(RadioPhase.CONNECTING, "Opening the USB serial port…")
        val driver = findDriver() ?: throw failure(
            LinkError.USB_NO_DEVICE,
            "No USB serial radio found. Plug in an OpenMesh radio and grant USB access.",
        )
        val deviceName = driver.device.productName
        if (!manager.hasPermission(driver.device)) {
            setProgress(
                RadioPhase.CONNECTING,
                "Waiting for USB permission…",
                deviceName = deviceName,
            )
            val granted = requestPermission(driver.device)
            if (!granted) {
                throw failure(
                    LinkError.USB_PERMISSION,
                    "USB permission was denied for ${deviceName ?: "the radio"}.",
                    deviceName = deviceName,
                )
            }
        }
        val connection = manager.openDevice(driver.device) ?: throw failure(
            LinkError.USB_PORT,
            "Could not open the USB radio (another app may be using it).",
            deviceName = deviceName,
        )

        val serialPort = driver.ports.first()
        serialPort.open(connection)
        serialPort.setParameters(
            115200,
            8,
            UsbSerialPort.STOPBITS_1,
            UsbSerialPort.PARITY_NONE,
        )
        runCatching { serialPort.setDTR(true) }
        port = serialPort
        _connected.value = true
        setProgress(
            RadioPhase.HANDSHAKING,
            "USB radio opened. Waiting for the handshake…",
            deviceName = deviceName,
        )
        startReadLoop(serialPort)
    }

    private fun failure(kind: LinkError, message: String, deviceName: String? = null): IllegalStateException {
        setProgress(RadioPhase.ERROR, message, deviceName = deviceName, error = kind)
        return IllegalStateException(message)
    }

    private fun setProgress(
        phase: RadioPhase,
        message: String,
        deviceName: String? = _progress.value.deviceName,
        error: LinkError? = null,
    ) {
        val previous = _progress.value
        _progress.value = LinkProgress(
            phase = phase,
            message = message,
            deviceName = deviceName,
            attempt = 1,
            error = error,
            sinceMs = if (previous.phase == phase && previous.sinceMs > 0L) previous.sinceMs else System.currentTimeMillis(),
        )
    }

    private fun findDriver(): UsbSerialDriver? {
        val drivers = UsbSerialProber.getDefaultProber().findAllDrivers(manager)
        if (drivers.isEmpty()) return null
        if (preferredDeviceName != null) {
            drivers.firstOrNull { it.device.productName == preferredDeviceName }?.let { return it }
        }
        return drivers.first()
    }

    private suspend fun requestPermission(device: UsbDevice): Boolean =
        withTimeoutOrNull(20_000) {
            suspendCancellableCoroutine { continuation ->
                val action = "${context.packageName}.USB_PERMISSION"
                val receiver = object : BroadcastReceiver() {
                    override fun onReceive(ctx: Context, intent: Intent) {
                        if (intent.action != action) return
                        ctx.unregisterReceiver(this)
                        continuation.resume(intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false))
                    }
                }
                ContextCompat.registerReceiver(
                    context,
                    receiver,
                    IntentFilter(action),
                    ContextCompat.RECEIVER_NOT_EXPORTED,
                )
                val flags = if (android.os.Build.VERSION.SDK_INT >= 31) {
                    PendingIntent.FLAG_MUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
                } else {
                    PendingIntent.FLAG_UPDATE_CURRENT
                }
                val intent = Intent(action).setPackage(context.packageName)
                val pending = PendingIntent.getBroadcast(context, 0, intent, flags)
                manager.requestPermission(device, pending)
                continuation.invokeOnCancellation { runCatching { context.unregisterReceiver(receiver) } }
            }
        } ?: false

    private fun startReadLoop(serialPort: UsbSerialPort) {
        readJob?.cancel()
        readJob = scope.launch {
            val buffer = ByteArray(4096)
            try {
                while (isActive && _connected.value) {
                    val count = try {
                        serialPort.read(buffer, 1000)
                    } catch (e: IOException) {
                        break
                    }
                    if (count > 0) {
                        _incoming.tryEmit(buffer.copyOf(count))
                    }
                }
            } finally {
                if (_connected.value) {
                    _connected.value = false
                    if (!userClosing) {
                        setProgress(
                            RadioPhase.ERROR,
                            "Lost connection to the USB radio. Check the cable, then reconnect.",
                            error = LinkError.LINK_LOST,
                        )
                    }
                }
            }
        }
    }

    override suspend fun disconnect() {
        userClosing = true
        _connected.value = false
        readJob?.cancel()
        readJob = null
        withContext(Dispatchers.IO) {
            runCatching { port?.close() }
            port = null
        }
        scope.cancel()
        scope = newScope()
        _progress.value = LinkProgress()
    }

    override suspend fun write(bytes: ByteArray) {
        val serialPort = port ?: error("USB transport is not connected")
        try {
            withContext(Dispatchers.IO) { serialPort.write(bytes, 2000) }
        } catch (e: IOException) {
            if (!userClosing) {
                setProgress(
                    RadioPhase.ERROR,
                    "Lost connection to the USB radio. Check the cable, then reconnect.",
                    error = LinkError.LINK_LOST,
                )
            }
            throw e
        }
    }
}
