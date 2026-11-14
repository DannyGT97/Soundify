package com.dannygt97.soundify

import android.os.Looper
import android.os.SystemClock
import androidx.annotation.OptIn
import androidx.media3.common.C
import androidx.media3.common.DeviceInfo
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.UnstableApi
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture

/** What another device (the one being controlled over Soundify Connect) is playing, as it is told to the notification. */
data class RemoteInfo(
  /** «En "PC"»: where it sounds. */
  val device: String,
  val title: String,
  val playlist: String,
  val playing: Boolean,
  val positionMs: Long,
  val durationMs: Long,
  val shuffle: Boolean,
  /** off, all or one. */
  val repeat: String,
  /** 0 to 1: the volume of the other device. The phone's volume buttons move THIS, not the phone's own. */
  val volume: Double,
)

/**
 * A player that plays nothing: it shows what ANOTHER device plays (title, where it sounds, position) in the media
 * notification and lock screen, and what is pressed there is asked of that device (`act`), not done here.
 * The state it shows is only what the other device last said; the position moves on by itself while it plays.
 */
@OptIn(UnstableApi::class)
class RemoteControlPlayer(looper: Looper, private val act: (action: String, seconds: Double) -> Unit) : SimpleBasePlayer(looper) {
  private var info = RemoteInfo("", "", "", false, 0, 0, false, "off", 1.0)
  private var stamp = SystemClock.elapsedRealtime()

  private var volumeSetAt = 0L

  /**
   * What the other device said. Must be called on the thread of the looper. It answers every volume it is asked for, and
   * with the buttons pressed again and again each answer is already out of date when it lands: right after this phone asked
   * for a volume, the one it asked for stays (otherwise the next press starts from the old value and the volume stalls).
   */
  fun show(next: RemoteInfo) {
    val recent = volumeSetAt != 0L && SystemClock.elapsedRealtime() - volumeSetAt < VOLUME_ECHO_MS
    render(if (recent) next.copy(volume = info.volume) else next)
  }

  private fun render(next: RemoteInfo) {
    info = next
    stamp = SystemClock.elapsedRealtime()
    invalidateState()
  }

  override fun getState(): State {
    val now = info
    if (now.title.isEmpty()) {
      return State.Builder().setAvailableCommands(Player.Commands.EMPTY).setPlaybackState(Player.STATE_IDLE).build()
    }
    val metadata = MediaMetadata.Builder().setTitle(now.title).setArtist(now.device).setAlbumTitle(now.playlist).build()
    val item = MediaItem.Builder().setMediaId(MEDIA_ID).setMediaMetadata(metadata).build()
    val data = MediaItemData.Builder(MEDIA_ID)
      .setMediaItem(item)
      .setDurationUs(if (now.durationMs > 0) now.durationMs * 1000 else C.TIME_UNSET)
      .setIsSeekable(now.durationMs > 0)
      .build()
    val since = stamp
    val position = PositionSupplier {
      if (!now.playing) now.positionMs
      else {
        val moved = now.positionMs + (SystemClock.elapsedRealtime() - since)
        if (now.durationMs > 0) Math.min(moved, now.durationMs) else moved
      }
    }
    val commands = Player.Commands.Builder()
      .addAll(
        Player.COMMAND_PLAY_PAUSE,
        Player.COMMAND_SEEK_TO_NEXT,
        Player.COMMAND_SEEK_TO_PREVIOUS,
        Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
        Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
        Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM,
        Player.COMMAND_SET_SHUFFLE_MODE,
        Player.COMMAND_SET_REPEAT_MODE,
        Player.COMMAND_GET_CURRENT_MEDIA_ITEM,
        Player.COMMAND_GET_METADATA,
        Player.COMMAND_GET_TIMELINE,
        Player.COMMAND_GET_DEVICE_VOLUME,
        Player.COMMAND_SET_DEVICE_VOLUME_WITH_FLAGS,
        Player.COMMAND_ADJUST_DEVICE_VOLUME_WITH_FLAGS,
      )
      .build()
    return State.Builder()
      .setAvailableCommands(commands)
      .setPlayWhenReady(now.playing, Player.PLAY_WHEN_READY_CHANGE_REASON_REMOTE)
      .setPlaybackState(Player.STATE_READY)
      .setPlaylist(ImmutableList.of(data))
      .setCurrentMediaItemIndex(0)
      .setContentPositionMs(position)
      // A remote playback: the system sends the phone's volume buttons here instead of changing the phone's volume.
      .setDeviceInfo(DeviceInfo.Builder(DeviceInfo.PLAYBACK_TYPE_REMOTE).setMinVolume(0).setMaxVolume(VOLUME_STEPS).build())
      .setDeviceVolume(Math.round(now.volume * VOLUME_STEPS).toInt())
      .setIsDeviceMuted(false)
      .setShuffleModeEnabled(now.shuffle)
      .setRepeatMode(
        when (now.repeat) {
          "one" -> Player.REPEAT_MODE_ONE
          "all" -> Player.REPEAT_MODE_ALL
          else -> Player.REPEAT_MODE_OFF
        },
      )
      .build()
  }

  override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
    act(if (playWhenReady) "play" else "pause", 0.0)
    // Shown at once; the other device's own word comes after and corrects it if it differs.
    show(info.copy(positionMs = currentPositionNow(), playing = playWhenReady))
    return Futures.immediateVoidFuture()
  }

  override fun handleSeek(mediaItemIndex: Int, positionMs: Long, seekCommand: Int): ListenableFuture<*> {
    when (seekCommand) {
      Player.COMMAND_SEEK_TO_NEXT, Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM -> act("next", 0.0)
      Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM -> act("previous", 0.0)
      else -> {
        act("seek", positionMs / 1000.0)
        show(info.copy(positionMs = positionMs))
      }
    }
    return Futures.immediateVoidFuture()
  }

  override fun handleSetShuffleModeEnabled(shuffleModeEnabled: Boolean): ListenableFuture<*> {
    act("shuffle", 0.0)
    return Futures.immediateVoidFuture()
  }

  override fun handleSetRepeatMode(repeatMode: Int): ListenableFuture<*> {
    act("repeat", 0.0)
    return Futures.immediateVoidFuture()
  }

  override fun handleSetDeviceVolume(deviceVolume: Int, flags: Int): ListenableFuture<*> {
    val volume = Math.max(0, Math.min(VOLUME_STEPS, deviceVolume)) / VOLUME_STEPS.toDouble()
    act("volume", volume)
    volumeSetAt = SystemClock.elapsedRealtime()
    render(info.copy(positionMs = currentPositionNow(), volume = volume))
    return Futures.immediateVoidFuture()
  }

  override fun handleIncreaseDeviceVolume(flags: Int): ListenableFuture<*> =
    handleSetDeviceVolume(Math.round(info.volume * VOLUME_STEPS).toInt() + 1, flags)

  override fun handleDecreaseDeviceVolume(flags: Int): ListenableFuture<*> =
    handleSetDeviceVolume(Math.round(info.volume * VOLUME_STEPS).toInt() - 1, flags)

  private fun currentPositionNow(): Long {
    val now = info
    if (!now.playing) return now.positionMs
    val moved = now.positionMs + (SystemClock.elapsedRealtime() - stamp)
    return if (now.durationMs > 0) Math.min(moved, now.durationMs) else moved
  }

  companion object {
    private const val MEDIA_ID = "soundify-remote"
    /** The volume buttons move it in steps of 1/20. */
    private const val VOLUME_STEPS = 20
    private const val VOLUME_ECHO_MS = 1000L
  }
}
