package com.dannygt97.soundify

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.Settings
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.yausername.ffmpeg.FFmpeg
import com.yausername.youtubedl_android.YoutubeDL
import java.io.File

@InvokeArg
class MulticastArgs {
  var on: Boolean = false
}

@InvokeArg
class BusyArgs {
  var busy: Boolean = false
  var title: String = ""
  var text: String = ""
  /** 0–100, or -1 while the progress is not known. */
  var progress: Int = -1
}

@InvokeArg
class ModesArgs {
  var shuffle: Boolean = false
  /** off, all or one. */
  var repeat: String = "off"
}

@InvokeArg
class PlayerLoadArgs {
  var path: String = ""
  var title: String = ""
  var subtitle: String = ""
  var autoplay: Boolean = false
  /** The track's thumbnail (a file), for the notification; empty when there is none yet. */
  var artwork: String = ""
}

@InvokeArg
class PathArgs {
  var path: String = ""
}

@InvokeArg
class VolumeArgs {
  var volume: Double = 0.0
}

@InvokeArg
class SeekArgs {
  var seconds: Double = 0.0
}

/**
 * What the Rust side cannot do on Android: unpack the tools that travel in the
 * APK (Python, yt-dlp, ffmpeg and QuickJS, from youtubedl-android), ask for
 * access to the phone's storage, pick a folder and keep the app alive while it
 * downloads. Every command resolves with plain JSON; the logic stays in `core/`.
 */
@TauriPlugin
class SoundifyPlugin(private val activity: Activity) : Plugin(activity) {

  // === TOOLS ===

  /**
   * Unpacks the runtime (the first time, or when the APK brings another
   * Python) and says how to start yt-dlp: the program, the arguments that go
   * before yt-dlp's own, and the environment. Slow the first time, so it goes
   * on a thread of its own.
   */
  @Command
  fun prepareRuntime(invoke: Invoke) {
    Thread {
      try {
        val context = activity.applicationContext
        YoutubeDL.init(context)
        FFmpeg.getInstance().init(context)
        invoke.resolve(runtimeDescription(context))
      } catch (e: Throwable) {
        // Some exceptions carry no message: the type and the top of the stack say what failed.
        val where = e.stackTrace.take(6).joinToString(" <- ") { "${it.className}.${it.methodName}:${it.lineNumber}" }
        invoke.reject("runtime: $e (cause: ${e.cause}) at $where", e as? Exception)
      }
    }.start()
  }

  /**
   * Replaces the yt-dlp that came in the APK with the newest stable one. The one
   * in the APK ages with it, and YouTube changes often: an old yt-dlp got only the
   * first 100 videos of a long playlist. The library checks the version itself and
   * downloads only when GitHub has another; the new one lives in the app's folder
   * and survives updating the APK. Answers `{updated}`.
   */
  @Command
  fun updateYtDlp(invoke: Invoke) {
    Thread {
      try {
        val status = YoutubeDL.updateYoutubeDL(activity.applicationContext, YoutubeDL.UpdateChannel.STABLE)
        invoke.resolve(JSObject().put("updated", status == YoutubeDL.UpdateStatus.DONE))
      } catch (e: Throwable) {
        invoke.reject("update: ${e.message}", e as? Exception)
      }
    }.start()
  }

  // === STORAGE ===

  /** Whether the app can write in the shared folders, and where the music goes by default. */
  @Command
  fun storageInfo(invoke: Invoke) {
    val out = JSObject()
    val allFiles = hasAllFilesAccess()
    // The default folder has to exist for the first download to land in it.
    if (allFiles) try { defaultMusicDir().mkdirs() } catch (_: Exception) {}
    out.put("allFiles", allFiles)
    out.put("musicDir", defaultMusicDir().absolutePath)
    out.put("sdk", Build.VERSION.SDK_INT)
    invoke.resolve(out)
  }

  /**
   * Opens the system screen where "All files access" is granted (Android 11+),
   * or asks for the storage permission (before it). Resolves when the user
   * comes back, with what they decided.
   */
  @Command
  fun requestAllFiles(invoke: Invoke) {
    if (hasAllFilesAccess()) {
      invoke.resolve(JSObject().put("granted", true))
      return
    }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
      val intent = Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:${activity.packageName}"))
      try {
        startActivityForResult(invoke, intent, "allFilesDone")
      } catch (e: Exception) {
        startActivityForResult(invoke, Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION), "allFilesDone")
      }
    } else {
      activity.requestPermissions(
        arrayOf(
          android.Manifest.permission.READ_EXTERNAL_STORAGE,
          android.Manifest.permission.WRITE_EXTERNAL_STORAGE,
        ),
        STORAGE_REQUEST,
      )
      // The answer to that dialog does not come back through here: the front
      // asks again with `storageInfo` when the app is back in the foreground.
      invoke.resolve(JSObject().put("granted", false))
    }
  }

  @ActivityCallback
  fun allFilesDone(invoke: Invoke, result: ActivityResult) {
    invoke.resolve(JSObject().put("granted", hasAllFilesAccess()))
  }

  /** The system's folder picker; resolves with the folder's real path, or nothing. */
  @Command
  fun pickFolder(invoke: Invoke) {
    val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
    startActivityForResult(invoke, intent, "folderPicked")
  }

  @ActivityCallback
  fun folderPicked(invoke: Invoke, result: ActivityResult) {
    val uri = result.data?.data
    val out = JSObject()
    if (result.resultCode == Activity.RESULT_OK && uri != null) {
      val path = treeToPath(uri)
      if (path != null) out.put("path", path) else out.put("unsupported", uri.toString())
    }
    invoke.resolve(out)
  }

  /**
   * The system's file picker for the cookies. The chosen file is a `content://`
   * URI yt-dlp cannot open, so it is copied to the app's own folder (writable:
   * yt-dlp saves the session back into it) and the copy's path comes back.
   */
  @Command
  fun pickCookies(invoke: Invoke) {
    val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*")
    startActivityForResult(invoke, intent, "cookiesPicked")
  }

  @ActivityCallback
  fun cookiesPicked(invoke: Invoke, result: ActivityResult) {
    val uri = result.data?.data
    val out = JSObject()
    if (result.resultCode == Activity.RESULT_OK && uri != null) {
      try {
        val target = File(activity.filesDir, "cookies.txt")
        val input = activity.contentResolver.openInputStream(uri) ?: throw java.io.IOException("cannot open $uri")
        input.use { source -> target.outputStream().use { source.copyTo(it) } }
        out.put("path", target.absolutePath)
      } catch (e: Exception) {
        invoke.reject("cookies: ${e.message}", e)
        return
      }
    }
    invoke.resolve(out)
  }

  /**
   * The system's picker for a cover. Like the cookies, the chosen image is a
   * `content://` URI the Rust side cannot read as a file, so it is copied to the
   * app's cache (each pick with its own name, so the webview never shows an old one)
   * and the copy's path comes back.
   */
  @Command
  fun pickImage(invoke: Invoke) {
    val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("image/*")
    startActivityForResult(invoke, intent, "imagePicked")
  }

  @ActivityCallback
  fun imagePicked(invoke: Invoke, result: ActivityResult) {
    val uri = result.data?.data
    val out = JSObject()
    if (result.resultCode == Activity.RESULT_OK && uri != null) {
      try {
        val mime = activity.contentResolver.getType(uri)
        val ext = android.webkit.MimeTypeMap.getSingleton().getExtensionFromMimeType(mime) ?: "jpg"
        activity.cacheDir.listFiles { f -> f.name.startsWith("picked-cover-") }?.forEach { it.delete() }
        val target = File(activity.cacheDir, "picked-cover-${System.currentTimeMillis()}.$ext")
        val input = activity.contentResolver.openInputStream(uri) ?: throw java.io.IOException("cannot open $uri")
        input.use { source -> target.outputStream().use { source.copyTo(it) } }
        out.put("path", target.absolutePath)
      } catch (e: Exception) {
        invoke.reject("image: ${e.message}", e)
        return
      }
    }
    invoke.resolve(out)
  }

  /**
   * Shows a folder of the main storage in the system's file browser. The browser
   * (DocumentsUI) opens a directory document; if the phone has no app for that,
   * its folder picker opens at the folder instead.
   */
  @Command
  fun openFolder(invoke: Invoke) {
    val args = invoke.parseArgs(PathArgs::class.java)
    val root = Environment.getExternalStorageDirectory().absolutePath
    if (args.path != root && !args.path.startsWith("$root/")) {
      invoke.reject("that folder is not in the main storage")
      return
    }
    val docId = "primary:" + args.path.removePrefix(root).trim('/')
    val authority = "com.android.externalstorage.documents"
    try {
      val view = Intent(Intent.ACTION_VIEW)
        .setDataAndType(DocumentsContract.buildDocumentUri(authority, docId), DocumentsContract.Document.MIME_TYPE_DIR)
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_GRANT_READ_URI_PERMISSION)
      try {
        activity.startActivity(view)
      } catch (e: android.content.ActivityNotFoundException) {
        val tree = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
          .putExtra(DocumentsContract.EXTRA_INITIAL_URI, DocumentsContract.buildDocumentUri(authority, docId))
        activity.startActivity(tree)
      }
      invoke.resolve()
    } catch (e: Exception) {
      invoke.reject("open folder: ${e.message}", e)
    }
  }

  // === UPDATING THE APP ===

  /** Can this app hand an APK to the installer? (Android 8+ asks the user once: "Install unknown apps".) */
  private fun mayInstall(): Boolean =
    Build.VERSION.SDK_INT < Build.VERSION_CODES.O || activity.packageManager.canRequestPackageInstalls()

  private fun openInstallSettings() {
    try {
      activity.startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${activity.packageName}")))
    } catch (e: Exception) {
      // No such screen on this phone: the front still says what to allow.
    }
  }

  /**
   * Asked BEFORE downloading an update, so nobody downloads ~50 MB to then be
   * sent to a settings screen: if the install is not allowed yet, that screen
   * opens and `needsPermission` comes back.
   */
  @Command
  fun allowInstall(invoke: Invoke) {
    val needs = !mayInstall()
    if (needs) openInstallSettings()
    invoke.resolve(JSObject().put("needsPermission", needs))
  }

  /**
   * Hands the downloaded (and already verified) APK to the system's installer.
   * Only a file of the app's own `updates/` folder is accepted.
   */
  @Command
  fun installApk(invoke: Invoke) {
    val args = invoke.parseArgs(PathArgs::class.java)
    val updates = File(activity.filesDir, "updates").canonicalFile
    val apk = File(args.path).canonicalFile
    if (!apk.path.startsWith(updates.path + File.separator) || !apk.isFile || !apk.name.endsWith(".apk")) {
      invoke.reject("that file is not a downloaded update")
      return
    }
    if (!mayInstall()) {
      openInstallSettings()
      invoke.resolve(JSObject().put("needsPermission", true))
      return
    }
    try {
      val uri = androidx.core.content.FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", apk)
      val install = Intent(Intent.ACTION_VIEW)
        .setDataAndType(uri, "application/vnd.android.package-archive")
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
      activity.startActivity(install)
      invoke.resolve(JSObject().put("needsPermission", false))
    } catch (e: Exception) {
      invoke.reject("install: ${e.message}", e)
    }
  }

  // === KEEPING THE APP ALIVE ===

  /** Starts or stops the foreground notice that keeps Android from killing the app while it downloads. */
  @Command
  fun setBusy(invoke: Invoke) {
    val args = invoke.parseArgs(BusyArgs::class.java)
    val context = activity.applicationContext
    val intent = Intent(context, BusyService::class.java)
    try {
      if (args.busy) {
        intent.putExtra("title", args.title).putExtra("text", args.text).putExtra("progress", args.progress)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) context.startForegroundService(intent) else context.startService(intent)
      } else {
        context.stopService(intent)
      }
    } catch (e: Exception) {
      // Starting a service from the background can be refused; the download goes on anyway.
    }
    invoke.resolve()
  }

  // === SOUNDIFY CONNECT ===

  private var multicast: android.net.wifi.WifiManager.MulticastLock? = null

  /**
   * Android drops the multicast packets mDNS lives on (to save battery) unless the app holds this lock.
   * Held only while Soundify Connect is on.
   */
  @Command
  fun multicastLock(invoke: Invoke) {
    val on = invoke.parseArgs(MulticastArgs::class.java).on
    try {
      if (on) {
        if (multicast == null) {
          val wifi = activity.applicationContext.getSystemService(Context.WIFI_SERVICE) as android.net.wifi.WifiManager
          multicast = wifi.createMulticastLock("soundify-connect").apply { setReferenceCounted(false) }
        }
        multicast?.acquire()
      } else {
        multicast?.takeIf { it.isHeld }?.release()
      }
    } catch (e: Exception) {
      // Without the lock the devices are found by address instead of by name; nothing else is lost.
    }
    invoke.resolve()
  }

  // === PLAYER ===

  private val player = NativePlayer(activity.applicationContext) { event -> trigger("player", event) }

  private val volume = DeviceVolume(activity.applicationContext) { event -> trigger("player", event) }

  init {
    current = this
  }

  /** Loads a file (leaving it paused or playing) and shows it in the notification. */
  @Command
  fun playerLoad(invoke: Invoke) {
    val args = invoke.parseArgs(PlayerLoadArgs::class.java)
    askForNotifications()
    player.load(args.path, args.title, args.subtitle, args.autoplay, args.artwork)
    invoke.resolve()
  }

  @Command
  fun playerPlay(invoke: Invoke) {
    player.play()
    invoke.resolve()
  }

  @Command
  fun playerPause(invoke: Invoke) {
    player.pause()
    invoke.resolve()
  }

  @Command
  fun playerSeek(invoke: Invoke) {
    player.seek(invoke.parseArgs(SeekArgs::class.java).seconds)
    invoke.resolve()
  }

  @Command
  fun playerStop(invoke: Invoke) {
    player.stop()
    invoke.resolve()
  }

  /** The screen the music notification asked for, once: the page goes there and it is forgotten. */
  @Command
  fun playerPendingOpen(invoke: Invoke) {
    val target = pendingOpen
    pendingOpen = null
    invoke.resolve(JSObject().put("target", target ?: ""))
  }

  /** What the notification's shuffle and repeat buttons should show (the page's state). */
  @Command
  fun playerSetModes(invoke: Invoke) {
    val args = invoke.parseArgs(ModesArgs::class.java)
    player.setModes(args.shuffle, args.repeat)
    invoke.resolve()
  }

  /**
   * Where the sound goes now: headphones or a Bluetooth speaker if one is connected (with the name it
   * announces, "AirPods de Dani"), otherwise this phone's own speaker (with the name the user gave it).
   */
  @Command
  fun playerOutputDevice(invoke: Invoke) {
    val audio = activity.getSystemService(Context.AUDIO_SERVICE) as android.media.AudioManager
    val outputs = audio.getDevices(android.media.AudioManager.GET_DEVICES_OUTPUTS)
    val bluetooth = intArrayOf(
      android.media.AudioDeviceInfo.TYPE_BLUETOOTH_A2DP, android.media.AudioDeviceInfo.TYPE_BLE_HEADSET,
      android.media.AudioDeviceInfo.TYPE_BLE_SPEAKER, android.media.AudioDeviceInfo.TYPE_HEARING_AID,
    )
    val wired = intArrayOf(
      android.media.AudioDeviceInfo.TYPE_WIRED_HEADSET, android.media.AudioDeviceInfo.TYPE_WIRED_HEADPHONES,
      android.media.AudioDeviceInfo.TYPE_USB_HEADSET, android.media.AudioDeviceInfo.TYPE_USB_DEVICE,
      android.media.AudioDeviceInfo.TYPE_HDMI,
    )
    val out = JSObject()
    val bt = outputs.firstOrNull { it.type in bluetooth }
    val cable = outputs.firstOrNull { it.type in wired }
    when {
      bt != null -> out.put("kind", "bluetooth").put("name", bt.productName?.toString() ?: "")
      cable != null -> out.put("kind", "headphones").put("name", cable.productName?.toString() ?: "")
      else -> {
        val given = try { android.provider.Settings.Global.getString(activity.contentResolver, "device_name") } catch (e: Exception) { null }
        out.put("kind", "this").put("name", given?.takeIf { it.isNotBlank() } ?: Build.MODEL)
      }
    }
    invoke.resolve(out)
  }

  /** The phone's music volume (0..1), as it is now. */
  @Command
  fun playerGetVolume(invoke: Invoke) {
    invoke.resolve(JSObject().put("volume", volume.get()))
  }

  /** Moves the phone's music volume; the new value also comes back as a `volume` event. */
  @Command
  fun playerSetVolume(invoke: Invoke) {
    volume.set(invoke.parseArgs(VolumeArgs::class.java).volume)
    invoke.resolve(JSObject().put("volume", volume.get()))
  }

  /** The notification only shows if they allowed it (Android 13+); the music plays either way. */
  private fun askForNotifications() {
    if (Build.VERSION.SDK_INT < 33 || notificationsAsked) return
    notificationsAsked = true
    if (activity.checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
      activity.requestPermissions(arrayOf(android.Manifest.permission.POST_NOTIFICATIONS), NOTIFICATION_REQUEST)
    }
  }

  // === HELPERS ===

  private fun hasAllFilesAccess(): Boolean =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) Environment.isExternalStorageManager()
    else activity.checkSelfPermission(android.Manifest.permission.WRITE_EXTERNAL_STORAGE) == android.content.pm.PackageManager.PERMISSION_GRANTED

  companion object {
    private const val STORAGE_REQUEST = 4711
    private const val NOTIFICATION_REQUEST = 4712
    private var notificationsAsked = false

    @Volatile
    private var current: SoundifyPlugin? = null

    /** «Next» / «previous» from the notification or the lock screen: the queue is the page's, so it decides. */
    fun skip(direction: String) {
      current?.trigger("player", JSObject().put("kind", "skip").put("direction", direction))
    }

    const val EXTRA_OPEN = "soundify_open"

    @Volatile
    private var pendingOpen: String? = null

    /**
     * The app was opened (or brought forward) from the music notification: `EXTRA_OPEN` says which
     * screen. The page may not be there yet (a cold start), so it is kept until the page asks for it.
     */
    fun openFrom(intent: Intent?) {
      val target = intent?.getStringExtra(EXTRA_OPEN) ?: return
      intent.removeExtra(EXTRA_OPEN)
      pendingOpen = target
      current?.trigger("player", JSObject().put("kind", "open"))
    }

    /** Shuffle / repeat pressed in the notification: the page owns both, so it toggles them. */
    fun remote(mode: String) {
      current?.trigger("player", JSObject().put("kind", "mode").put("mode", mode))
    }

    /** `Music/Soundify` in the phone's shared storage. */
    fun defaultMusicDir(): File = File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_MUSIC), "Soundify")

    /**
     * `content://.../tree/primary:Music/Soundify` becomes `/storage/emulated/0/Music/Soundify`.
     * Only the phone's own storage and SD cards have a path; a cloud provider's
     * folder does not, and gets null.
     */
    fun treeToPath(uri: Uri): String? {
      if (uri.authority != "com.android.externalstorage.documents") return null
      val id = try { DocumentsContract.getTreeDocumentId(uri) } catch (e: Exception) { return null }
      val split = id.indexOf(':')
      if (split < 0) return null
      val volume = id.substring(0, split)
      val relative = id.substring(split + 1)
      val base = if (volume == "primary") Environment.getExternalStorageDirectory().absolutePath else "/storage/$volume"
      return if (relative.isEmpty()) base else File(base, relative).absolutePath
    }

    /** Where everything unpacked lives, laid out the way youtubedl-android does it. */
    fun runtimeDescription(context: Context): JSObject {
      val native = context.applicationInfo.nativeLibraryDir
      val base = File(context.noBackupFilesDir, YoutubeDL.baseName)
      val python = File(File(base, "packages"), "python")
      val ffmpeg = File(File(base, "packages"), "ffmpeg")
      val aria = File(File(base, "packages"), "aria2c")
      val ytdlp = File(File(base, YoutubeDL.ytdlpDirName), YoutubeDL.ytdlpBin)
      val ssl = File(python, "usr/etc/tls/cert.pem").absolutePath
      val home = File(python, "usr").absolutePath
      val path = (System.getenv("PATH") ?: "") + ":" + native
      val env = JSObject()
      env.put("LD_LIBRARY_PATH", "${python.absolutePath}/usr/lib:${ffmpeg.absolutePath}/usr/lib:${aria.absolutePath}/usr/lib")
      env.put("SSL_CERT_FILE", ssl)
      env.put("PATH", path)
      env.put("PYTHONHOME", home)
      env.put("HOME", home)
      env.put("TMPDIR", context.cacheDir.absolutePath)
      val args = JSArray()
      args.put(ytdlp.absolutePath)
      args.put("--js-runtimes").put("quickjs:$native/libqjs.so")
      args.put("--ffmpeg-location").put("$native/libffmpeg.so")
      args.put("--cache-dir").put(File(base, "cache").absolutePath)
      val out = JSObject()
      out.put("program", "$native/libpython.so")
      out.put("args", args)
      out.put("env", env)
      out.put("ffmpeg", "$native/libffmpeg.so")
      out.put("ytdlp", ytdlp.absolutePath)
      return out
    }
  }
}
