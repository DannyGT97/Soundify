package com.dannygt97.soundify

import android.content.ComponentName
import android.content.Context
import android.net.Uri
import android.os.Handler
import android.os.Looper
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import app.tauri.plugin.JSObject
import java.io.File

/**
 * The page's handle on the music player (`PlaybackService`): it plays ONE file
 * at a time, the queue is the page's. What happens in it goes back to the page
 * as events (`kind`: time, duration, ended, error, playing, skip), also what
 * the user does from the notification or the lock screen.
 *
 * Everything runs on the main thread. The controller connects to the service
 * asynchronously, so what arrives before it is ready waits in line.
 */
class NativePlayer(private val context: Context, private val emit: (JSObject) -> Unit) {
  private val main = Handler(Looper.getMainLooper())
  private var controller: MediaController? = null
  private var connecting = false
  private val waiting = ArrayList<(MediaController) -> Unit>()
  private var durationSent = false

  private val ticker = object : Runnable {
    override fun run() {
      val c = controller
      if (c != null && c.isPlaying) {
        emit(event("time").put("seconds", c.currentPosition / 1000.0))
        main.postDelayed(this, TICK_MS)
      }
    }
  }

  private val listener = object : Player.Listener {
    override fun onPlaybackStateChanged(state: Int) {
      val c = controller ?: return
      if (state == Player.STATE_READY && !durationSent && c.duration != C.TIME_UNSET) {
        durationSent = true
        emit(event("duration").put("seconds", c.duration / 1000.0))
      }
      if (state == Player.STATE_ENDED) emit(event("ended"))
    }

    override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
      emit(event("playing").put("playing", playWhenReady))
    }

    override fun onIsPlayingChanged(isPlaying: Boolean) {
      main.removeCallbacks(ticker)
      if (isPlaying) main.post(ticker)
    }

    override fun onPlayerError(error: PlaybackException) {
      emit(event("error").put("message", error.message ?: ""))
    }
  }

  fun load(path: String, title: String, subtitle: String, autoplay: Boolean) = withController { c ->
    durationSent = false
    val meta = MediaMetadata.Builder().setTitle(title).setArtist(subtitle).setAlbumTitle(subtitle).build()
    c.setMediaItem(MediaItem.Builder().setUri(Uri.fromFile(File(path))).setMediaMetadata(meta).build())
    c.playWhenReady = autoplay
    c.prepare()
  }

  fun play() = withController { it.play() }
  fun pause() = withController { it.pause() }
  fun seek(seconds: Double) = withController { it.seekTo((seconds * 1000).toLong()) }

  fun stop() = withController {
    it.stop()
    it.clearMediaItems()
  }

  /** Runs `work` on the main thread once the controller is connected. */
  private fun withController(work: (MediaController) -> Unit) {
    main.post {
      val c = controller
      if (c != null) {
        work(c)
        return@post
      }
      waiting.add(work)
      if (connecting) return@post
      connecting = true
      val token = SessionToken(context, ComponentName(context, PlaybackService::class.java))
      val future = MediaController.Builder(context, token).buildAsync()
      future.addListener({
        connecting = false
        try {
          val connected = future.get()
          connected.addListener(listener)
          controller = connected
          val queued = ArrayList(waiting)
          waiting.clear()
          queued.forEach { it(connected) }
        } catch (e: Exception) {
          waiting.clear()
          emit(event("error").put("message", "player: ${e.message}"))
        }
      }, main::post)
    }
  }

  private fun event(kind: String): JSObject = JSObject().put("kind", kind)

  companion object {
    private const val TICK_MS = 500L
  }
}
