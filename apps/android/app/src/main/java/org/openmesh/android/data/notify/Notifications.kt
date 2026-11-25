package org.openmesh.android.data.notify

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.app.NotificationCompat
import org.openmesh.android.MainActivity
import org.openmesh.android.R

/**
 * Notification channels: `messages` for incoming message previews (tap → thread) and `service`
 * for the persistent foreground-service notification. Previews never contain media bytes or
 * secrets — only sender/channel name + text preview (docs/design/android-ga.md §2.5).
 */
object Notifications {
    const val CHANNEL_SERVICE = "openmesh_service"
    const val CHANNEL_MESSAGES = "openmesh_messages"
    const val SERVICE_ID = 4711
    private const val EXTRA_PEER = "openmesh.extra.peer"
    private const val EXTRA_CHANNEL = "openmesh.extra.channel"

    fun ensureChannels(context: Context) {
        if (Build.VERSION.SDK_INT < 26) return
        val manager = context.getSystemService(NotificationManager::class.java) ?: return
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_SERVICE,
                context.getString(R.string.notification_channel_link),
                NotificationManager.IMPORTANCE_LOW,
            ).apply { description = context.getString(R.string.service_description) },
        )
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_MESSAGES,
                context.getString(R.string.notification_channel_messages),
                NotificationManager.IMPORTANCE_DEFAULT,
            ).apply { description = "New mesh messages; tap to open the conversation." },
        )
    }

    fun serviceNotification(context: Context, text: String): Notification {
        ensureChannels(context)
        val contentIntent = PendingIntent.getActivity(
            context,
            0,
            Intent(context, MainActivity::class.java),
            pendingFlags(),
        )
        return NotificationCompat.Builder(context, CHANNEL_SERVICE)
            .setSmallIcon(R.drawable.ic_launcher)
            .setContentTitle(context.getString(R.string.app_name))
            .setContentText(text)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setContentIntent(contentIntent)
            .build()
    }

    fun messageNotification(
        context: Context,
        peerId: String,
        sender: String,
        body: String,
        isChannel: Boolean,
    ): Notification {
        ensureChannels(context)
        val intent = Intent(context, MainActivity::class.java).apply {
            action = Intent.ACTION_VIEW
            putExtra(EXTRA_PEER, peerId)
            putExtra(EXTRA_CHANNEL, isChannel)
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
        }
        val contentIntent = PendingIntent.getActivity(
            context,
            peerId.hashCode(),
            intent,
            pendingFlags(),
        )
        val title = if (isChannel) "New message in $sender" else sender
        return NotificationCompat.Builder(context, CHANNEL_MESSAGES)
            .setSmallIcon(R.drawable.ic_launcher)
            .setContentTitle(title)
            .setContentText(previewText(body))
            .setStyle(NotificationCompat.BigTextStyle().bigText(previewText(body)))
            .setAutoCancel(true)
            .setGroup(if (isChannel) "channel:$peerId" else "direct")
            .setContentIntent(contentIntent)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .build()
    }

    fun messageId(peerId: String, isChannel: Boolean): Int =
        (peerId + if (isChannel) ":channel" else ":direct").hashCode()

    fun showMessage(context: Context, peerId: String, sender: String, body: String, isChannel: Boolean) {
        val manager = context.getSystemService(NotificationManager::class.java) ?: return
        manager.notify(messageId(peerId, isChannel), messageNotification(context, peerId, sender, body, isChannel))
    }

    /** Pure helper used by unit tests: no secrets, no media, bounded length. */
    fun previewText(body: String, limit: Int = 140): String {
        val clean = body.replace('\n', ' ').trim()
        if (clean.isEmpty()) return "New message"
        return if (clean.length <= limit) clean else clean.take(limit - 1).trimEnd() + "…"
    }

    fun peerFromIntent(intent: Intent?): String? = intent?.getStringExtra(EXTRA_PEER)
    fun channelFromIntent(intent: Intent?): Boolean = intent?.getBooleanExtra(EXTRA_CHANNEL, false) ?: false

    private fun pendingFlags(): Int =
        PendingIntent.FLAG_UPDATE_CURRENT or
            if (Build.VERSION.SDK_INT >= 23) PendingIntent.FLAG_IMMUTABLE else 0
}
