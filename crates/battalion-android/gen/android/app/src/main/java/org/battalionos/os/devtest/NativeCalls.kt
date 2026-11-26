package org.battalionos.os.devtest

// AND-4: the native calls of an app's page on a phone, run in the app's own slot (its activity is on screen when the page asks).
// The host already checked each call against the app's installed app.json (`os_frame_phone_call`) and cleaned its arguments; this
// file only does the Android part: the document picker (Storage Access Framework), a link intent, a notification on the app's own
// channel, and Google's code scanner (QR only; the page gets the text, never the camera). One call at a time per slot.

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import com.google.mlkit.common.sdkinternal.MlKitContext
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning
import org.json.JSONObject

class NativeCalls(private val act: ComponentActivity, private val answer: (JSONObject) -> Unit) {
  private var req: String? = null
  private var notice: JSONObject? = null

  private fun done(result: Any?) {
    val r = req ?: return
    req = null
    answer(JSONObject().put("req", r).put("ok", true).put("result", result ?: JSONObject.NULL))
  }

  private fun fail(text: String) {
    val r = req ?: return
    req = null
    answer(JSONObject().put("req", r).put("ok", false).put("error", text))
  }

  private val openDoc = act.registerForActivityResult(ActivityResultContracts.OpenDocument()) { uri -> done(keep(uri, false)) }
  private val openTree = act.registerForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri -> done(keep(uri, true)) }
  private val askNotify = act.registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
    val n = notice
    notice = null
    if (granted && n != null) post(n) else fail("Notifications are turned off for BattalionOS; turn them on in Android's settings to get notices from this app.")
  }

  /** A picked document keeps its read access after this slot ends (a folder: read and write). */
  private fun keep(uri: Uri?, tree: Boolean): String? {
    if (uri == null) return null
    val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or (if (tree) Intent.FLAG_GRANT_WRITE_URI_PERMISSION else 0)
    try { act.contentResolver.takePersistableUriPermission(uri, flags) } catch (_: Exception) {}
    return uri.toString()
  }

  fun run(j: JSONObject) {
    val r = j.optString("req")
    if (req != null) {
      answer(JSONObject().put("req", r).put("ok", false).put("error", "another request of this app is still open"))
      return
    }
    req = r
    val args = j.optJSONObject("args") ?: JSONObject()
    try {
      when (j.optString("call")) {
        "pick_file" -> openDoc.launch(arrayOf("*/*"))
        "pick_folder" -> openTree.launch(null)
        "open_external" -> openLink(args.optString("url"))
        "notify" -> notify(args)
        // scan_code is run by the hub in the OS process (its result comes back there).
        else -> fail("'${j.optString("call")}' is not available on this device")
      }
    } catch (e: Exception) {
      fail(e.message ?: "the call failed")
    }
  }

  private fun openLink(url: String) {
    // The host allowed http, https and mailto only; checked again here, so no other scheme ever leaves the slot.
    val u = Uri.parse(url)
    val it = when (u.scheme?.lowercase()) {
      "http", "https" -> Intent(Intent.ACTION_VIEW, u).addCategory(Intent.CATEGORY_BROWSABLE)
      "mailto" -> Intent(Intent.ACTION_SENDTO, u)
      else -> return fail("only http, https and mailto links open")
    }
    try {
      act.startActivity(it.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
      done(null)
    } catch (_: ActivityNotFoundException) {
      fail("No app on this phone opens that link.")
    }
  }

  private fun notify(args: JSONObject) {
    if (Build.VERSION.SDK_INT >= 33 && ContextCompat.checkSelfPermission(act, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
      notice = args
      askNotify.launch(Manifest.permission.POST_NOTIFICATIONS)
      return
    }
    post(args)
  }

  private fun post(args: JSONObject) {
    val app = act.intent.getStringExtra("app") ?: return fail("the app is not open")
    val name = act.intent.getStringExtra("name") ?: app
    val channel = "bos-app-$app"
    val nm = act.getSystemService(NotificationManager::class.java)
    if (Build.VERSION.SDK_INT >= 26 && nm.getNotificationChannel(channel) == null) {
      nm.createNotificationChannel(NotificationChannel(channel, name, NotificationManager.IMPORTANCE_DEFAULT))
    }
    if (!NotificationManagerCompat.from(act).areNotificationsEnabled()) {
      return fail("Notifications are turned off for BattalionOS; turn them on in Android's settings to get notices from this app.")
    }
    val n = NotificationCompat.Builder(act, channel)
      .setSmallIcon(act.applicationInfo.icon)
      .setContentTitle(args.optString("title"))
      .setContentText(args.optString("text"))
      .setSubText(name)
      .setAutoCancel(true)
      .build()
    try {
      NotificationManagerCompat.from(act).notify(channel, (System.currentTimeMillis() and 0x7fffffff).toInt(), n)
      done(null)
    } catch (_: SecurityException) {
      fail("Notifications are turned off for BattalionOS; turn them on in Android's settings to get notices from this app.")
    }
  }

  companion object {
    /** Google's code scanner, QR only, run in the OS's own process (see SlotHub.native); `done(ok, text-or-error)`. */
    fun scan(ctx: android.content.Context, done: (Boolean, String?) -> Unit) {
      MlKitContext.initializeIfNeeded(ctx.applicationContext)
      val opts = GmsBarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build()
      GmsBarcodeScanning.getClient(ctx, opts).startScan()
        .addOnSuccessListener { b -> done(true, b.rawValue) }
        .addOnCanceledListener { done(true, null) }
        // QA-NIGHT-4 (af8518bd): Back on the open scanner can end as a failure; that is a cancel (OK null, as os_pick_file's).
        // Only a scanner that could not start (module unavailable) says "not available".
        .addOnFailureListener { e ->
          val code = (e as? com.google.mlkit.common.MlKitException)?.errorCode
          when (code) {
            com.google.mlkit.common.MlKitException.UNAVAILABLE -> done(false, "The code scanner is not available on this phone (${e.message ?: "Google Play services"}).")
            else -> done(true, null)
          }
        }
    }
  }

}
