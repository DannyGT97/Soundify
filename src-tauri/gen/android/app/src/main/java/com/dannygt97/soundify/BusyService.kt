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
 * The notice that is on screen while Soundify downloads. Without a foreground
 * service Android freezes the app a minute after the screen goes off, and the
 * download dies with it.
 */
class BusyService : Service() {
  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    val title = intent?.getStringExtra("title") ?: "Soundify"
    val text = intent?.getStringExtra("text") ?: ""
    val progress = intent?.getIntExtra("progress", -1) ?: -1
    val manager = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      manager.createNotificationChannel(NotificationChannel(CHANNEL, "Soundify", NotificationManager.IMPORTANCE_LOW))
    }
    val open = PendingIntent.getActivity(
      this, 0, Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
    val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) Notification.Builder(this, CHANNEL) else Notification.Builder(this)
    val notice = builder
      .setSmallIcon(applicationInfo.icon)
      .setContentTitle(title)
      .setContentText(text)
      .setContentIntent(open)
      .setOngoing(true)
      // Each change of progress re-posts it: it must not buzz or pop up again every time.
      .setOnlyAlertOnce(true)
      // How far it is, or a bar that goes back and forth while that is not known.
      .setProgress(100, progress.coerceIn(0, 100), progress < 0)
      .build()
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
      startForeground(ID, notice, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
    } else {
      startForeground(ID, notice)
    }
    return START_NOT_STICKY
  }

  companion object {
    private const val CHANNEL = "soundify-busy"
    private const val ID = 1
  }
}
