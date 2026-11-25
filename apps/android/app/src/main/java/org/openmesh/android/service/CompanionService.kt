package org.openmesh.android.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import org.openmesh.android.MainActivity
import org.openmesh.android.R
import org.openmesh.android.data.repository.CompanionKind
import org.openmesh.android.data.repository.MeshGraph

/**
 * Foreground service that owns the companion link so the radio connection survives the app
 * going to the background (Android 14 requires a typed foreground service for connected
 * devices). The service is a thin lifecycle wrapper around [MeshRepository]; all protocol and
 * UI state stays in the repository.
 */
class CompanionService : Service() {

    override fun onCreate() {
        super.onCreate()
        createChannels()
        startForegroundCompat(buildNotification("Connecting to radio…"))
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_CONNECT_USB -> {
                MeshGraph.repository().connectCompanion(CompanionKind.USB)
                notify("USB companion link active")
            }
            ACTION_CONNECT_BLE -> {
                MeshGraph.repository().connectCompanion(CompanionKind.BLE)
                notify("Bluetooth companion link active")
            }
            ACTION_DISCONNECT -> {
                MeshGraph.repository().disconnectCompanion()
                stopSelf()
            }
            else -> Unit
        }
        return START_STICKY
    }

    override fun onDestroy() {
        MeshGraph.repository().disconnectCompanion()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun notify(text: String) {
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        manager.notify(NOTIFICATION_ID, buildNotification(text))
    }

    private fun buildNotification(text: String): Notification {
        val open = Intent(this, MainActivity::class.java)
        val pendingFlags = if (Build.VERSION.SDK_INT >= 23) {
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        } else {
            PendingIntent.FLAG_UPDATE_CURRENT
        }
        val contentIntent = PendingIntent.getActivity(this, 0, open, pendingFlags)
        return NotificationCompat.Builder(this, CHANNEL_LINK)
            .setSmallIcon(R.drawable.ic_launcher)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(text)
            .setOngoing(true)
            .setContentIntent(contentIntent)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .build()
    }

    private fun startForegroundCompat(notification: Notification) {
        if (Build.VERSION.SDK_INT >= 34) {
            startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE,
            )
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun createChannels() {
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_LINK,
                getString(R.string.notification_channel_link),
                NotificationManager.IMPORTANCE_LOW,
            ).apply { description = getString(R.string.service_description) },
        )
        val messages = NotificationChannel(
            CHANNEL_MESSAGES,
            getString(R.string.notification_channel_messages),
            NotificationManager.IMPORTANCE_DEFAULT,
        )
        manager.createNotificationChannel(messages)
    }

    companion object {
        const val CHANNEL_LINK = "openmesh_link"
        const val CHANNEL_MESSAGES = "openmesh_messages"
        const val NOTIFICATION_ID = 4711

        const val ACTION_CONNECT_USB = "org.openmesh.android.action.CONNECT_USB"
        const val ACTION_CONNECT_BLE = "org.openmesh.android.action.CONNECT_BLE"
        const val ACTION_DISCONNECT = "org.openmesh.android.action.DISCONNECT"

        fun start(context: Context, action: String) {
            val intent = Intent(context, CompanionService::class.java).setAction(action)
            if (Build.VERSION.SDK_INT >= 26) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        }

        fun stop(context: Context) {
            context.startService(
                Intent(context, CompanionService::class.java).setAction(ACTION_DISCONNECT),
            )
        }
    }
}
