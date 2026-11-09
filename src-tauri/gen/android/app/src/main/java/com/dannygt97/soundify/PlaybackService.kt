package com.dannygt97.soundify

import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.ForwardingPlayer
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService

/**
 * The music player. Media3 turns it into the notification with the controls
 * and keeps it a foreground service while it sounds, so playback goes on with
 * the screen off. The queue stays in the web page (`player.ts`): the player
 * has ONE file at a time, and «next» / «previous» from the notification or the
 * lock screen are handed back to the page (`SoundifyPlugin.skip`).
 */
class PlaybackService : MediaSessionService() {
  private var session: MediaSession? = null

  override fun onCreate() {
    super.onCreate()
    val exo = ExoPlayer.Builder(this)
      .setAudioAttributes(
        AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(),
        true, // takes the audio focus: a call or another player pauses it
      )
      .setHandleAudioBecomingNoisy(true) // unplugged headphones pause it
      .build()
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
    session = MediaSession.Builder(this, player).build()
  }

  override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = session

  override fun onDestroy() {
    session?.run {
      player.release()
      release()
    }
    session = null
    super.onDestroy()
  }
}
