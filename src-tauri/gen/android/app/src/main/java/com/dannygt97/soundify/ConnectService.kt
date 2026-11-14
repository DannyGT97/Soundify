package com.dannygt97.soundify

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder

/**
 * The notice that is on screen while Soundify Connect is on. The other devices reach this one through a server
 * that lives inside the app: an app that is only in the background is put to sleep by Android (and its network
 * closed to what arrives from outside), so a phone that is not in front would never be seen or controlled. A
 * foreground service keeps it awake; turning Connect off removes the notice.
 */
class ConnectService : Service() {
  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    val title = intent?.getStringExtra("title") ?: "Soundify Connect"
    val text = intent?.getStringExtra("text") ?: ""
    val manager = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      manager.createNotificationChannel(NotificationChannel(CHANNEL, "Soundify Connect", NotificationManager.IMPORTANCE_LOW))
    }
    val open = PendingIntent.getActivity(
      this, 2, Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
    val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) Notification.Builder(this, CHANNEL) else Notification.Builder(this)
    val notice = builder
      .setSmallIcon(applicationInfo.icon)
      .setContentTitle(title)
      .setContentText(text)
      .setContentIntent(open)
      .setOngoing(true)
      .setOnlyAlertOnce(true)
      .build()
    try {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        startForeground(ID, notice, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
      } else {
        startForeground(ID, notice)
      }
    } catch (e: Exception) {
      // Android may refuse to start one from the background: Connect still works while the app is in front.
      stopSelf()
    }
    return START_NOT_STICKY
  }

  companion object {
    private const val CHANNEL = "soundify-connect"
    private const val ID = 3
  }
}
