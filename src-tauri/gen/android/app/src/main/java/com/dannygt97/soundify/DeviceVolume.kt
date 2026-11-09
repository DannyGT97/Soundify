package com.dannygt97.soundify

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.database.ContentObserver
import android.media.AudioManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.util.Log
import app.tauri.plugin.JSObject

/**
 * The phone's music volume, which is the only one that matters on Android: the
 * app's slider moves it and the device's buttons move the slider. Reported as
 * 0..1; every change, whoever made it, goes out as a `volume` event.
 */
class DeviceVolume(context: Context, private val emit: (JSObject) -> Unit) {
  private val audio = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
  private val main = Handler(Looper.getMainLooper())
  private var last = -1

  private val receiver = object : BroadcastReceiver() {
    override fun onReceive(context: Context?, intent: Intent?) {
      if (intent?.getIntExtra(STREAM_EXTRA, -1) == AudioManager.STREAM_MUSIC) report()
    }
  }

  private val observer = object : ContentObserver(main) {
    override fun onChange(selfChange: Boolean) = report()
  }

  init {
    val filter = IntentFilter(VOLUME_CHANGED)
    if (Build.VERSION.SDK_INT >= 33) context.registerReceiver(receiver, filter, Context.RECEIVER_EXPORTED)
    else context.registerReceiver(receiver, filter)
    context.contentResolver.registerContentObserver(Settings.System.CONTENT_URI, true, observer)
  }

  /** 0..1. */
  fun get(): Double {
    val max = audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC)
    return if (max <= 0) 0.0 else audio.getStreamVolume(AudioManager.STREAM_MUSIC).toDouble() / max
  }

  /** Moves the device's volume to the nearest step; the change comes back as an event like any other. */
  fun set(fraction: Double) {
    val max = audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC)
    val step = Math.round(fraction.coerceIn(0.0, 1.0) * max).toInt()
    // Without FLAG_SHOW_UI: the app's slider is the feedback.
    audio.setStreamVolume(AudioManager.STREAM_MUSIC, step, 0)
    report()
  }

  private fun report() {
    val now = audio.getStreamVolume(AudioManager.STREAM_MUSIC)
    if (now == last) return
    last = now
    Log.d("SoundifyVolume", "music volume is now $now of ${audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC)}")
    emit(JSObject().put("kind", "volume").put("volume", get()))
  }

  companion object {
    // Not in the public API but sent by every Android version for a stream's volume change.
    private const val VOLUME_CHANGED = "android.media.VOLUME_CHANGED_ACTION"
    private const val STREAM_EXTRA = "android.media.EXTRA_VOLUME_STREAM_TYPE"
  }
}
