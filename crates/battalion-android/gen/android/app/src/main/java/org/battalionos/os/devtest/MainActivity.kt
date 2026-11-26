package org.battalionos.os.devtest

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
  }

  // AND-3: the shell's channel to the app slots. Only the shell page (its own origin, the main frame) gets `bosSlots`; app pages
  // never load in this WebView on a phone (each runs in its own slot process).
  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    if (!WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) return
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
