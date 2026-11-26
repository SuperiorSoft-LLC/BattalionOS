package org.battalionos.os

import android.graphics.Color
import android.os.Bundle
import android.view.View
import android.webkit.WebView
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import org.json.JSONObject
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // The shell is dark: light status and navigation bar icons.
    enableEdgeToEdge(SystemBarStyle.dark(Color.TRANSPARENT), SystemBarStyle.dark(Color.TRANSPARENT))
    super.onCreate(savedInstanceState)
    // AND-UI-1: nothing of the shell is drawn under the status bar, the gesture or navigation bar, or a display cutout. The content
    // view is padded by those insets; the bars show over the OS's own background colour.
    val root = findViewById<View>(android.R.id.content)
    root.setBackgroundColor(Color.parseColor("#0e0f12"))
    ViewCompat.setOnApplyWindowInsetsListener(root) { v, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout() or WindowInsetsCompat.Type.ime())
      v.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.CONSUMED
    }
    statusOf(intent)
  }

  // BOS-AND-SELFUPDATE-1: Android's installer answers here (OsUpdate.kt): its confirm, a failure, or, after the update, success,
  // which is also what opens BattalionOS again.
  override fun onNewIntent(intent: android.content.Intent) {
    super.onNewIntent(intent)
    statusOf(intent)
  }

  private fun statusOf(i: android.content.Intent?) {
    if (i?.action == OsUpdate.ACTION_STATUS) OsUpdate.status(this, i)
  }

  // AND-6 (f4f965ad): taps before the shell has drawn (the native splash, the still blank WebView) are recorded here in CSS px of
  // the WebView, and handed to the shell once over `bosBoot` (op "taps"), which replays the newest on the desktop.
  private val earlyTaps = org.json.JSONArray()
  @Volatile private var shellReady = false
  private var shellView: WebView? = null

  override fun dispatchTouchEvent(ev: android.view.MotionEvent): Boolean {
    if (!shellReady && ev.actionMasked == android.view.MotionEvent.ACTION_UP) {
      val at = IntArray(2)
      shellView?.getLocationOnScreen(at) ?: findViewById<View>(android.R.id.content).let { c ->
        c.getLocationOnScreen(at); at[0] += c.paddingLeft; at[1] += c.paddingTop
      }
      val d = resources.displayMetrics.density
      synchronized(earlyTaps) {
        earlyTaps.put(JSONObject().put("x", (ev.rawX - at[0]) / d).put("y", (ev.rawY - at[1]) / d).put("t", System.currentTimeMillis()))
      }
    }
    return super.dispatchTouchEvent(ev)
  }

  // AND-3: the shell's channel to the app slots. Only the shell page (its own origin, the main frame) gets `bosSlots`; app pages
  // never load in this WebView on a phone (each runs in its own slot process).
  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    shellView = webView
    // AND-6: the page area is the shell's dark colour from the start, not a blank white page before the HTML paints.
    webView.setBackgroundColor(Color.parseColor("#0e0f12"))
    if (!WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) return
    WebViewCompat.addWebMessageListener(webView, "bosBoot", setOf("http://tauri.localhost", "https://tauri.localhost")) { _, message, _, isMainFrame, reply ->
      if (!isMainFrame || message.data?.contains("\"taps\"") != true) return@addWebMessageListener
      shellReady = true
      val taps = synchronized(earlyTaps) { earlyTaps.toString().also { while (earlyTaps.length() > 0) earlyTaps.remove(0) } }
      reply.postMessage("{\"taps\":$taps}")
    }
    // BOS-AND-SELFUPDATE-1: the shell's channel to the OS's own update (OsUpdate.kt): op "osInstall" {apk, sha256}.
    WebViewCompat.addWebMessageListener(webView, "bosUpdate", setOf("http://tauri.localhost", "https://tauri.localhost")) { _, message, _, isMainFrame, reply ->
      if (!isMainFrame) return@addWebMessageListener
      OsUpdate.toShell = { o -> runOnUiThread { try { reply.postMessage(o.toString()) } catch (_: Exception) {} } }
      val m = try { JSONObject(message.data ?: return@addWebMessageListener) } catch (_: Exception) { return@addWebMessageListener }
      if (m.optString("op") == "osInstall") {
        val ctx = applicationContext
        Thread { OsUpdate.install(ctx, m.optString("apk"), m.optString("sha256")) }.start()
      }
    }
    WebViewCompat.addWebMessageListener(webView, "bosSlots", setOf("http://tauri.localhost", "https://tauri.localhost")) { _, message, _, isMainFrame, reply ->
      if (!isMainFrame) return@addWebMessageListener
      SlotHub.toShell = { o -> runOnUiThread { try { reply.postMessage(o.toString()) } catch (_: Exception) {} } }
      val m = try { JSONObject(message.data ?: return@addWebMessageListener) } catch (_: Exception) { return@addWebMessageListener }
      val id = m.optString("id")
      when (m.optString("op")) {
        "hello" -> reply.postMessage(SlotHub.state(this).toString())
        // AND-6: Settings, "Apps open at once".
        "config" -> {
          SlotHub.setSlotsInUse(this, m.optInt("inUse", SlotHub.SLOT_CLASSES.size))
          reply.postMessage(SlotHub.state(this).toString())
        }
        "open" -> SlotHub.open(this, id, m.optString("url"), m.optString("origin"), m.optString("name", id), m.optJSONObject("files"))
        "close" -> SlotHub.close(id)
        "reply" -> SlotHub.reply(id, m.optJSONObject("message")?.toString() ?: return@addWebMessageListener)
        // AND-4: a native call the host already checked against the app's manifest; the app's own slot runs it.
        "native" -> SlotHub.native(id, m.optString("req"), m.optString("call"), m.optJSONObject("args") ?: JSONObject(), applicationContext)
      }
    }
  }
}
