package org.battalionos.os

// BOS-AND-SELFUPDATE-1: BattalionOS installs its own update, in the OS process. The host (selfupdate.rs) already downloaded the APK
// into the OS's private storage and checked it against the feed's sha256; the shell hands its path here (`bosUpdate` op
// `osInstall`). This step checks again, then lets Android install it:
//   1. the file is inside this app's own storage and its sha256 is the one the host checked;
//   2. it is this app (same package) and a newer versionCode;
//   3. its signing certificate is the installed app's (refused otherwise, in a plain sentence);
//   4. BattalionOS may install apps ("Install unknown apps"); if not, Android's settings screen for it opens;
//   5. a PackageInstaller session; Android shows its own confirm once. The answer goes back to the shell (`ev` "osInstall").
// Android answers through an activity PendingIntent on MainActivity: after it replaced the app, that answer opens BattalionOS again
// (a broadcast receiver may not start an activity from the background on current Android).

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.util.Log
import org.json.JSONObject
import java.io.File
import java.security.MessageDigest

object OsUpdate {
  const val TAG = "BosUpdate"
  const val ACTION_STATUS = "org.battalionos.os.OS_UPDATE_STATUS"

  /** Sends one answer to the shell page (set by MainActivity once the shell's channel is up). */
  @Volatile var toShell: ((JSONObject) -> Unit)? = null

  private fun answer(ok: Boolean, error: String? = null, pending: Boolean = false) {
    val o = JSONObject().put("ev", "osInstall").put("ok", ok)
    if (error != null) o.put("error", error)
    if (pending) o.put("pending", true)
    Log.i(TAG, o.toString())
    toShell?.invoke(o)
  }

  fun sha256(f: File): String {
    val md = MessageDigest.getInstance("SHA-256")
    f.inputStream().use { s ->
      val buf = ByteArray(64 * 1024)
      while (true) {
        val n = s.read(buf)
        if (n <= 0) break
        md.update(buf, 0, n)
      }
    }
    return md.digest().joinToString("") { "%02x".format(it) }
  }

  @Suppress("DEPRECATION")
  private fun signers(p: PackageInfo?): Set<String> {
    val si = p?.signingInfo ?: return emptySet()
    val certs = if (si.hasMultipleSigners()) si.apkContentsSigners else si.signingCertificateHistory
    return certs.orEmpty().map { c -> MessageDigest.getInstance("SHA-256").digest(c.toByteArray()).joinToString("") { "%02x".format(it) } }.toSet()
  }

  @Suppress("DEPRECATION")
  private fun versionCode(p: PackageInfo): Long = if (Build.VERSION.SDK_INT >= 28) p.longVersionCode else p.versionCode.toLong()

  /** The checks 1-3; null when the APK may be installed, else the reason in plain words. */
  fun refusal(ctx: Context, apk: File, sha: String): String? {
    val home = File(ctx.applicationInfo.dataDir).canonicalFile
    val f = apk.canonicalFile
    if (!f.path.startsWith(home.path + File.separator) || !f.isFile) return "The update file is not in BattalionOS's own storage."
    if (sha.isEmpty() || !sha256(f).equals(sha, ignoreCase = true)) return "The downloaded update does not match the update feed, so it was not installed."
    val pm = ctx.packageManager
    val flags = PackageManager.GET_SIGNING_CERTIFICATES
    val new = pm.getPackageArchiveInfo(f.path, flags) ?: return "The downloaded update is not a valid Android app."
    if (new.packageName != ctx.packageName) return "The downloaded update is a different app (${new.packageName}), so it was not installed."
    val mine = pm.getPackageInfo(ctx.packageName, flags)
    val newCerts = signers(new)
    if (newCerts.isEmpty() || newCerts.intersect(signers(mine)).isEmpty()) {
      return "This update is not signed by the same publisher as the BattalionOS on this phone, so it was refused."
    }
    if (versionCode(new) <= versionCode(mine)) return "The downloaded update is not newer than this BattalionOS (${mine.versionName})."
    return null
  }

  fun install(ctx: Context, apkPath: String, sha: String) {
    val apk = File(apkPath)
    val why = try { refusal(ctx, apk, sha) } catch (e: Exception) { "The update could not be checked: ${e.message}" }
    if (why != null) return answer(false, why)
    val pm = ctx.packageManager
    if (Build.VERSION.SDK_INT >= 26 && !pm.canRequestPackageInstalls()) {
      try {
        ctx.startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${ctx.packageName}")).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
      } catch (_: Exception) {}
      return answer(false, "BattalionOS needs Android's permission to install its update. In the screen that opened, turn on \"Allow from this source\", come back and press Update again.")
    }
    try {
      val installer = pm.packageInstaller
      val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
      params.setAppPackageName(ctx.packageName)
      params.setSize(apk.length())
      val id = installer.createSession(params)
      installer.openSession(id).use { s ->
        s.openWrite("BattalionOS.apk", 0, apk.length()).use { out ->
          apk.inputStream().use { it.copyTo(out) }
          s.fsync(out)
        }
        val i = Intent(ctx, MainActivity::class.java).setAction(ACTION_STATUS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        val piFlags = PendingIntent.FLAG_UPDATE_CURRENT or (if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0)
        val opts = if (Build.VERSION.SDK_INT >= 34) {
          android.app.ActivityOptions.makeBasic().setPendingIntentCreatorBackgroundActivityStartMode(android.app.ActivityOptions.MODE_BACKGROUND_ACTIVITY_START_ALLOWED).toBundle()
        } else null
        val pi = PendingIntent.getActivity(ctx, id, i, piFlags, opts)
        s.commit(pi.intentSender)
      }
      answer(false, pending = true)
    } catch (e: Exception) {
      answer(false, "Android could not start the install: ${e.message}")
    }
  }

  /** PackageInstaller's answer for the session. */
  fun status(ctx: Context, i: Intent) {
    val st = i.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)
    val msg = i.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE)
    when (st) {
      PackageInstaller.STATUS_PENDING_USER_ACTION -> {
        @Suppress("DEPRECATION")
        val confirm = if (Build.VERSION.SDK_INT >= 33) i.getParcelableExtra(Intent.EXTRA_INTENT, Intent::class.java) else i.getParcelableExtra(Intent.EXTRA_INTENT)
        if (confirm == null) return answer(false, "Android did not show its install confirm.")
        ctx.startActivity(confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        answer(false, pending = true)
      }
      PackageInstaller.STATUS_SUCCESS -> answer(true)
      PackageInstaller.STATUS_FAILURE_ABORTED -> answer(false, "You cancelled the update; BattalionOS stays as it is.")
      PackageInstaller.STATUS_FAILURE_CONFLICT, PackageInstaller.STATUS_FAILURE_INCOMPATIBLE ->
        answer(false, "Android refused the update: it does not fit the BattalionOS on this phone (${msg ?: "conflict"}).")
      PackageInstaller.STATUS_FAILURE_STORAGE -> answer(false, "There is not enough free space on the phone to install the update.")
      PackageInstaller.STATUS_FAILURE_BLOCKED -> answer(false, "Android blocked the update (${msg ?: "blocked"}).")
      else -> answer(false, "Android did not install the update (${msg ?: "status $st"}).")
    }
  }
}

/** After Android replaced the app. Current Android lets no app open itself from the background, so (when the person allows
 *  BattalionOS's notices) one notice says the update is done; a tap opens BattalionOS. On older Android it opens at once. */
class OsUpdatedReceiver : android.content.BroadcastReceiver() {
  override fun onReceive(ctx: Context, i: Intent) {
    if (i.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
    val open = Intent(ctx, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    if (Build.VERSION.SDK_INT < 29) {
      try { ctx.startActivity(open); return } catch (_: Exception) {}
    }
    val version = try { ctx.packageManager.getPackageInfo(ctx.packageName, 0).versionName } catch (_: Exception) { "" }
    val nm = ctx.getSystemService(android.app.NotificationManager::class.java)
    val channel = "bos-os-update"
    if (Build.VERSION.SDK_INT >= 26 && nm.getNotificationChannel(channel) == null) {
      nm.createNotificationChannel(android.app.NotificationChannel(channel, "BattalionOS updates", android.app.NotificationManager.IMPORTANCE_HIGH))
    }
    if (!androidx.core.app.NotificationManagerCompat.from(ctx).areNotificationsEnabled()) return
    val pi = PendingIntent.getActivity(ctx, 0, open, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    val n = androidx.core.app.NotificationCompat.Builder(ctx, channel)
      .setSmallIcon(ctx.applicationInfo.icon)
      .setContentTitle("BattalionOS $version is installed")
      .setContentText("Tap to open BattalionOS.")
      .setContentIntent(pi)
      .setAutoCancel(true)
      .setPriority(androidx.core.app.NotificationCompat.PRIORITY_HIGH)
      .build()
    try { nm.notify(7001, n) } catch (_: SecurityException) {}
  }
}
