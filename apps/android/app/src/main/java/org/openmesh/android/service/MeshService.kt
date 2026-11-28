package org.openmesh.android.service

import android.Manifest
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.hardware.usb.UsbManager
import android.os.Build
import android.os.IBinder
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.launch
import org.openmesh.android.data.notify.Notifications
import org.openmesh.android.data.repository.CompanionKind
import org.openmesh.android.data.repository.MeshGraph
import org.openmesh.android.ui.radio.RadioPresentation
import org.openmesh.android.util.MeshLog

/**
 * Foreground service that keeps the mesh alive while the app is backgrounded.
 *
 * The actual engine loop (ingestFrame / tick 250 ms / outbound → radio) lives in
 * [org.openmesh.android.data.repository.MeshRepository]; this service only:
 *  * upgrades the process to a `connectedDevice` foreground service (Android 14 requirement),
 *  * starts/stops the companion link on user action,
 *  * keeps the persistent notification in sync with engine/radio state.
 *
 * The service is started automatically once onboarding is complete and by the radio screen.
 */
class MeshService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private var observer: Job? = null

    override fun onCreate() {
        super.onCreate()
        Notifications.ensureChannels(this)
        MeshLog.i("MeshService: onCreate; starting foreground service")
        try {
            startForegroundCompat(Notifications.serviceNotification(this, "Starting mesh…"))
        } catch (se: SecurityException) {
            // Android 14+ requires a granted Bluetooth or USB permission for a `connectedDevice`
            // foreground service. Without one, startForeground throws and would crash the app;
            // stay stopped and let the UI ask the user to connect a radio first.
            MeshLog.e("MeshService: cannot start foreground service (${se.message}); stopping", se)
            stopSelf()
            return
        }
        MeshGraph.repository().ensureStarted()
        observer = scope.launch {
            // Phase-aware notification: scanning/connecting/handshake/ready/error/demo, so a
            // backgrounded app still tells the user what the radio link is doing.
            combine(
                MeshGraph.repository().engineStatus,
                MeshGraph.repository().radio,
            ) { status, radio -> status to radio }.collectLatest { (status, radio) ->
                val demoMode = MeshGraph.repository().settings.value.demoMode
                val text = RadioPresentation.notificationText(
                    radio = radio,
                    demoMode = demoMode,
                    knownNodes = status.knownNodes,
                    engineRunning = status.running,
                )
                val manager = getSystemService(NotificationManager::class.java)
                manager?.notify(Notifications.SERVICE_ID, Notifications.serviceNotification(this@MeshService, text))
            }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> {
                MeshGraph.repository().ensureStarted()
                MeshGraph.repository().connectCompanion(CompanionKind.NONE)
            }

            ACTION_CONNECT_USB -> {
                MeshGraph.repository().ensureStarted()
                MeshGraph.repository().connectCompanion(CompanionKind.USB)
            }

            ACTION_CONNECT_BLE -> {
                MeshGraph.repository().ensureStarted()
                MeshGraph.repository().connectCompanion(CompanionKind.BLE)
            }

            ACTION_DISCONNECT -> {
                MeshGraph.repository().disconnectCompanion()
            }

            ACTION_STOP -> {
                MeshLog.i("MeshService: stop requested")
                MeshGraph.repository().stop()
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            }
            else -> MeshGraph.repository().ensureStarted()
        }
        return START_STICKY
    }

    override fun onDestroy() {
        // Persist the engine state before the service (and possibly the process) goes away.
        runCatching { MeshGraph.repository().persistStateOnLifecycleStop() }
        observer?.cancel()
        scope.cancel()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun startForegroundCompat(notification: android.app.Notification) {
        if (Build.VERSION.SDK_INT < 34) {
            startForeground(Notifications.SERVICE_ID, notification)
            return
        }
        try {
            startForeground(
                Notifications.SERVICE_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE,
            )
        } catch (se: SecurityException) {
            // Android 14+ demands a granted Bluetooth/USB permission for `connectedDevice`. Until a
            // radio is paired we fall back to the declared `specialUse` type so the engine/notification
            // keep running instead of the process dying with a SecurityException.
            MeshLog.w("MeshService: connectedDevice unavailable (${se.message}); using specialUse")
            startForeground(
                Notifications.SERVICE_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE,
            )
        }
    }

    companion object {
        const val ACTION_START = "org.openmesh.android.action.START_MESH"
        const val ACTION_CONNECT_USB = "org.openmesh.android.action.CONNECT_USB"
        const val ACTION_CONNECT_BLE = "org.openmesh.android.action.CONNECT_BLE"
        const val ACTION_DISCONNECT = "org.openmesh.android.action.DISCONNECT"
        const val ACTION_STOP = "org.openmesh.android.action.STOP_MESH"

        /**
         * Android 14+ (API 34) refuses to promote a `connectedDevice` foreground service unless the
         * app already holds a Bluetooth or USB device permission. A fresh install has neither, so
         * starting the service after onboarding would throw SecurityException and kill the app.
         * Defer the service until a radio is actually being paired (which grants one of them).
         */
        fun canStartConnectedDeviceService(context: Context): Boolean {
            if (Build.VERSION.SDK_INT < 34) return true
            val bt = ContextCompat.checkSelfPermission(context, Manifest.permission.BLUETOOTH_CONNECT) == PackageManager.PERMISSION_GRANTED ||
                ContextCompat.checkSelfPermission(context, Manifest.permission.BLUETOOTH_SCAN) == PackageManager.PERMISSION_GRANTED
            if (bt) return true
            val usb = context.getSystemService(Context.USB_SERVICE) as? UsbManager
            val usbGranted = usb?.deviceList?.values?.any { usb.hasPermission(it) } == true
            return usbGranted
        }

        fun start(context: Context, action: String = ACTION_START): Boolean {
            val intent = Intent(context, MeshService::class.java).setAction(action)
            if (Build.VERSION.SDK_INT >= 26) context.startForegroundService(intent) else context.startService(intent)
            return true
        }

        fun stop(context: Context) {
            context.startService(Intent(context, MeshService::class.java).setAction(ACTION_STOP))
        }

        fun disconnect(context: Context) = start(context, ACTION_DISCONNECT)
    }
}
