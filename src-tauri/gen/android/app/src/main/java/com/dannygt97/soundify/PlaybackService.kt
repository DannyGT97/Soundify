package com.dannygt97.soundify

import android.app.PendingIntent
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.ForwardingPlayer
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.CommandButton
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import androidx.media3.session.SessionCommand
import androidx.media3.session.SessionResult
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture

/**
 * The music player. Media3 turns it into the notification with the controls
 * and keeps it a foreground service while it sounds, so playback goes on with
 * the screen off. The queue stays in the web page (`player.ts`): the player
 * has ONE file at a time, and «next» / «previous» from the notification or the
 * lock screen are handed back to the page (`SoundifyPlugin.skip`).
 *
 * Besides those, the notification carries shuffle (far left) and repeat (far
 * right) as custom buttons: they too are the page's (`SoundifyPlugin.remote`),
 * and the page tells the service which state to draw (`ACTION_MODES`).
 */
class PlaybackService : MediaSessionService() {
  private var session: MediaSession? = null
  private val main = Handler(Looper.getMainLooper())

  // What the two custom buttons show (the page's `ui.shuffle` and `ui.repeat`: off, all, one).
  private var shuffle = false
  private var repeat = "off"

  private val shuffleCommand = SessionCommand(ACTION_SHUFFLE, Bundle.EMPTY)
  private val repeatCommand = SessionCommand(ACTION_REPEAT, Bundle.EMPTY)
  private val modesCommand = SessionCommand(ACTION_MODES, Bundle.EMPTY)

  /** The queue's last file ended and nothing came: after a while the notice stops holding the app up. */
  private val releaseAfterEnd = Runnable { session?.player?.let { if (it.playbackState == Player.STATE_ENDED) it.pause() } }

  override fun onCreate() {
    super.onCreate()
    val exo = ExoPlayer.Builder(this)
      .setAudioAttributes(
        AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(),
        true, // takes the audio focus: a call or another player pauses it
      )
      .setHandleAudioBecomingNoisy(true) // unplugged headphones pause it
      .build()
    exo.addListener(object : Player.Listener {
      override fun onPlaybackStateChanged(state: Int) {
        main.removeCallbacks(releaseAfterEnd)
        if (state == Player.STATE_ENDED) main.postDelayed(releaseAfterEnd, END_GRACE_MS)
      }
    })
    val player = object : ForwardingPlayer(exo) {
      override fun getAvailableCommands(): Player.Commands = super.getAvailableCommands().buildUpon()
        .add(Player.COMMAND_SEEK_TO_NEXT)
        .add(Player.COMMAND_SEEK_TO_PREVIOUS)
        .add(Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM)
        .add(Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM)
        .build()

      override fun isCommandAvailable(command: Int): Boolean = availableCommands.contains(command)
      override fun hasNextMediaItem(): Boolean = true
      override fun hasPreviousMediaItem(): Boolean = true
      override fun seekToNext() = SoundifyPlugin.skip("next")
      override fun seekToNextMediaItem() = SoundifyPlugin.skip("next")
      override fun seekToPrevious() = SoundifyPlugin.skip("previous")
      override fun seekToPreviousMediaItem() = SoundifyPlugin.skip("previous")
    }
    // Tapping the notification (or the system's media controls) opens the app on Reproduciendo.
    val open = PendingIntent.getActivity(
      this, 1,
      Intent(this, MainActivity::class.java)
        .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_NEW_TASK)
        .putExtra(SoundifyPlugin.EXTRA_OPEN, "now-playing"),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
    session = MediaSession.Builder(this, player)
      .setSessionActivity(open)
      .setCallback(object : MediaSession.Callback {
        override fun onConnect(session: MediaSession, controller: MediaSession.ControllerInfo): MediaSession.ConnectionResult {
          val commands = MediaSession.ConnectionResult.DEFAULT_SESSION_COMMANDS.buildUpon()
            .add(shuffleCommand).add(repeatCommand).add(modesCommand).build()
          return MediaSession.ConnectionResult.AcceptedResultBuilder(session)
            .setAvailableSessionCommands(commands)
            .setCustomLayout(layout())
            .build()
        }

        override fun onCustomCommand(
          session: MediaSession,
          controller: MediaSession.ControllerInfo,
          customCommand: SessionCommand,
          args: Bundle,
        ): ListenableFuture<SessionResult> {
          when (customCommand.customAction) {
            ACTION_SHUFFLE -> SoundifyPlugin.remote("shuffle")
            ACTION_REPEAT -> SoundifyPlugin.remote("repeat")
            ACTION_MODES -> {
              shuffle = args.getBoolean("shuffle", false)
              repeat = args.getString("repeat") ?: "off"
              session.setCustomLayout(layout())
            }
          }
          return Futures.immediateFuture(SessionResult(SessionResult.RESULT_SUCCESS))
        }
      })
      .build()
    session?.setCustomLayout(layout())
  }

  /** Shuffle first, repeat second: the system's media controls put them at the left and right ends. */
  private fun layout(): ImmutableList<CommandButton> {
    val shuffleButton = CommandButton.Builder(if (shuffle) CommandButton.ICON_SHUFFLE_ON else CommandButton.ICON_SHUFFLE_OFF)
      .setDisplayName("Shuffle")
      .setSessionCommand(shuffleCommand)
      .build()
    val repeatButton = CommandButton.Builder(
      when (repeat) {
        "one" -> CommandButton.ICON_REPEAT_ONE
        "all" -> CommandButton.ICON_REPEAT_ALL
        else -> CommandButton.ICON_REPEAT_OFF
      },
    )
      .setDisplayName("Repeat")
      .setSessionCommand(repeatCommand)
      .build()
    return ImmutableList.of(shuffleButton, repeatButton)
  }

  /**
   * Media3 takes the service out of the foreground when the file ends (the player is no longer «ongoing»),
   * and the next file, started by the page a moment later while the app is in the background, could not put
   * it back: Android refuses to start a foreground service from there. The notification then never came
   * back although the music played. So while the player is meant to be playing and has a file, it stays.
   */
  override fun onUpdateNotification(session: MediaSession, startInForegroundRequired: Boolean) {
    val player = session.player
    val keep = player.mediaItemCount > 0 && player.playWhenReady
    super.onUpdateNotification(session, startInForegroundRequired || keep)
  }

  override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = session

  override fun onDestroy() {
    main.removeCallbacks(releaseAfterEnd)
    session?.run {
      player.release()
      release()
    }
    session = null
    super.onDestroy()
  }

  companion object {
    const val ACTION_SHUFFLE = "soundify.shuffle"
    const val ACTION_REPEAT = "soundify.repeat"
    const val ACTION_MODES = "soundify.modes"

    /** How long the end of the queue holds the notification up before it lets go. */
    private const val END_GRACE_MS = 20_000L
  }
}
