package org.battalionos.os.devtest

// AND-3: one app slot. It runs in its own process (android:process in the manifest), so the app's page has its own renderer and
// its own web storage (WebView data directory suffix = the app id). No Tauri here: the slot shows an OS-owned host page that holds
// the app's page in one frame at the app's own origin; the app still talks to the OS with window.parent.postMessage, and the host
// page passes those messages to the shell through the hub (frames.ts answers, as on the desktop).

import android.annotation.SuppressLint
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.graphics.Color
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Message
import android.os.Messenger
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.activity.OnBackPressedCallback
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.webkit.JavaScriptReplyProxy
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import org.json.JSONObject

/** The host page's own origin: answered by the slot itself (no network, no port). */
private const val HOST = "http://bos-slot.localhost"

/** The WebView data folder suffix of an app: its id, kept to the characters a folder name may hold. */
fun slotDataSuffix(app: String): String = "app-" + app.replace(Regex("[^A-Za-z0-9._-]"), "_")

abstract class AppSlot(private val index: Int) : AppCompatActivity() {
  private var hub: Messenger? = null
  private var proxy: JavaScriptReplyProxy? = null
  private val queue = ArrayList<String>()

  private fun endSelf() {
    finishAndRemoveTask()
    android.os.Process.killProcess(android.os.Process.myPid())
  }

  private val inbox = Messenger(object : Handler(Looper.getMainLooper()) {
    override fun handleMessage(msg: Message) {
      when (msg.what) {
        SlotHub.REPLY -> msg.data.getString("json")?.let { j -> proxy?.postMessage(j) }
        SlotHub.FINISH -> endSelf()
      }
    }
  })

  private val conn = object : ServiceConnection {
    override fun onServiceConnected(name: ComponentName?, b: IBinder?) {
      hub = Messenger(b)
      send(Message.obtain(null, SlotHub.REGISTER, index, android.os.Process.myPid()).apply { data = Bundle().apply { putString("app", intent.getStringExtra("app")) } })
      queue.forEach { post(it) }
      queue.clear()
    }

    // The OS's own process ended: a slot never runs without it.
    override fun onServiceDisconnected(name: ComponentName?) = endSelf()
  }

  private fun send(m: Message) {
    m.arg1 = index
    m.replyTo = inbox
    try { hub?.send(m) } catch (_: Exception) {}
  }

  private fun post(json: String) {
    if (hub == null) { queue.add(json); return }
    send(Message.obtain(null, SlotHub.REQUEST).apply { data = Bundle().apply { putString("json", json) } })
  }

  @SuppressLint("SetJavaScriptEnabled")
  override fun onCreate(savedInstanceState: Bundle?) {
    val app = intent.getStringExtra("app")
    // Before any WebView in this process: this app's cookies, localStorage and cache live in their own folder. A process serves
    // one app only (the hub ends it before giving the slot to another), so the suffix is set once per process.
    if (app != null && !suffixSet) {
      WebView.setDataDirectorySuffix(slotDataSuffix(app))
      suffixSet = true
    }
    enableEdgeToEdge(SystemBarStyle.dark(Color.TRANSPARENT), SystemBarStyle.dark(Color.TRANSPARENT))
    super.onCreate(savedInstanceState)
    // A slot with no app ends; one the hub did not give (Android brought it back on its own) is ended by the hub at REGISTER.
    if (app == null) { endSelf(); return }
    val url = intent.getStringExtra("url") ?: ""
    val origin = intent.getStringExtra("origin") ?: ""
    // AND-3b: the app's page files, served below at the app's own host (no port); given by the hub, which checked them.
    val files = intent.getStringExtra("root")?.let { r -> PageFiles(r, intent.getStringExtra("pageDir") ?: "", intent.getStringExtra("csp") ?: "") }
    val appHost = PageFiles.hostOf(app)
    @Suppress("DEPRECATION")
    setTaskDescription(android.app.ActivityManager.TaskDescription(intent.getStringExtra("name") ?: app))
    val wv = WebView(this)
    wv.setBackgroundColor(Color.parseColor("#0e0f12"))
    wv.settings.javaScriptEnabled = true
    wv.settings.domStorageEnabled = true
    wv.settings.allowFileAccess = false
    wv.settings.allowContentAccess = false
    // AND-UI-2: a WebView ignores its own padding (the page still draws under the bars), so the slot pads its content frame instead,
    // with the same insets as MainActivity: status, gesture/navigation bar, cutout and the keyboard.
    setContentView(wv)
    val root = findViewById<android.view.View>(android.R.id.content)
    root.setBackgroundColor(Color.parseColor("#0e0f12"))
    ViewCompat.setOnApplyWindowInsetsListener(root) { v, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout() or WindowInsetsCompat.Type.ime())
      v.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.CONSUMED
    }
    if (!WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
      wv.loadData("<body style='background:#0e0f12;color:#ddd;font:16px sans-serif;padding:24px'>This phone's WebView is too old to run BattalionOS apps. Update Android System WebView.</body>", "text/html", "utf-8")
    } else {
      // Only the host page (its own origin, the main frame) gets the channel; the app's frame is another origin and never sees it.
      WebViewCompat.addWebMessageListener(wv, "bosHost", setOf(HOST)) { _, message, source, isMainFrame, reply ->
        if (!isMainFrame || source.toString().trimEnd('/') != HOST) return@addWebMessageListener
        proxy = reply
        val d = message.data ?: return@addWebMessageListener
        if (d != HELLO) post(d)
      }
      wv.webViewClient = object : WebViewClient() {
        override fun shouldInterceptRequest(view: WebView, req: WebResourceRequest): WebResourceResponse? {
          if (req.url.scheme == "http" && req.url.host == "bos-slot.localhost") {
            return if (req.url.path == "/" || req.url.path.isNullOrEmpty()) WebResourceResponse("text/html", "utf-8", hostPage(url, origin).byteInputStream())
            else WebResourceResponse("text/plain", "utf-8", 404, "Not Found", emptyMap(), "".byteInputStream())
          }
          val host = req.url.host ?: return null
          // The app's own page. Any other `*.bos.localhost` (another app's) is never served here, and never reaches the network.
          if (req.url.scheme == "http" && host == appHost && files != null) return files.answer(req.method, req.url.encodedPath ?: "/")
          if (host == "bos.localhost" || host.endsWith(".bos.localhost")) return WebResourceResponse("text/plain", "utf-8", 404, "Not Found", emptyMap(), "".byteInputStream())
          return null
        }

        // The top of the slot never leaves the host page; the app's frame stays in its frame.
        override fun shouldOverrideUrlLoading(view: WebView, req: WebResourceRequest): Boolean = req.isForMainFrame && req.url.toString() != "$HOST/"
      }
      wv.loadUrl("$HOST/")
    }
    bindService(Intent(this, SlotHubService::class.java), conn, Context.BIND_AUTO_CREATE)
    // Back: the app keeps running in its slot; the shell comes to the front and shows the desktop (as AND-UI-1's back does).
    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        send(Message.obtain(null, SlotHub.LEFT))
        startActivity(Intent(this@AppSlot, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT))
      }
    })
  }

  override fun onDestroy() {
    try { unbindService(conn) } catch (_: Exception) {}
    super.onDestroy()
  }

  companion object {
    private var suffixSet = false
    private const val HELLO = "{\"hello\":1}"

    /** The host page: the app's page in one frame; messages from that frame (its window, its origin) go to the hub, answers back. */
    fun hostPage(url: String, origin: String): String {
      val u = JSONObject.quote(url)
      val o = JSONObject.quote(origin)
      return """<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<style>html,body{margin:0;height:100%;background:#0e0f12}iframe{border:0;display:block;width:100%;height:100%}</style></head>
<body><iframe id="f" allow="clipboard-read; clipboard-write"></iframe><script>
const URL_ = $u, ORIGIN = $o, f = document.getElementById("f");
f.src = URL_;
addEventListener("message", (e) => { if (e.source !== f.contentWindow || e.origin !== ORIGIN) return; bosHost.postMessage(JSON.stringify({ origin: e.origin, data: e.data })); });
bosHost.onmessage = (e) => { try { f.contentWindow.postMessage(JSON.parse(e.data), ORIGIN); } catch (_) {} };
bosHost.postMessage('$HELLO');
</script></body></html>"""
    }
  }
}

class AppSlot0 : AppSlot(0)
class AppSlot1 : AppSlot(1)
class AppSlot2 : AppSlot(2)
class AppSlot3 : AppSlot(3)
