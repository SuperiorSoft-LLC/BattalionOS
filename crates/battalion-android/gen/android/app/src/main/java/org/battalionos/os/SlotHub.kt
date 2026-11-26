package org.battalionos.os

// AND-3: each BattalionOS app's page runs in its own Android process slot (design note bos-android-design-2026-11-25, section 5,
// option C). The manifest declares a fixed pool of slot activities, each with its own `android:process` (:app0..:app3). The hub
// lives in the OS's own process: it gives a slot to an app while the app is open, ends the slot's process before the slot serves
// another app, and tells the shell what happened. The OS knows an app by the slot it gave, never by what a page says.

import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Message
import android.os.Messenger
import android.os.RemoteException
import android.os.SystemClock
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject

object SlotHub {
  const val TAG = "BosSlots"
  // Messages between the hub and a slot process (Message.what).
  const val REGISTER = 1 // slot -> hub: arg1 = slot, arg2 = pid, replyTo = the slot's messenger
  const val REQUEST = 2 // slot -> hub: arg1 = slot, data "json" = {origin, data} from the app's frame
  const val LEFT = 3 // slot -> hub: arg1 = slot; the person went back to the desktop (the app keeps running)
  const val NATIVE_DONE = 4 // slot -> hub: arg1 = slot, data "json" = {req, ok, result?, error?} (AND-4)
  const val LOGIN = 5 // slot -> hub: arg1 = slot, data "json" = {url} or {closed: true}, the app's sign-in view (AND-6)
  const val REPLY = 10 // hub -> slot: data "json" = the answer for the app's frame
  const val FINISH = 11 // hub -> slot: end the slot (its task and its process)
  const val NATIVE = 12 // hub -> slot: data "json" = {req, call, args}, a call the host checked (AND-4)

  /** The pool declared in AndroidManifest.xml (the build maximum). */
  val SLOT_CLASSES: List<Class<*>> = listOf(AppSlot0::class.java, AppSlot1::class.java, AppSlot2::class.java, AppSlot3::class.java)

  /** How many slots may hold an app at once: the named setting `inUse` in the shared preferences "bos_slots", 1..pool size;
   *  absent = the whole pool. */
  fun slotsInUse(ctx: Context): Int =
    ctx.getSharedPreferences("bos_slots", Context.MODE_PRIVATE).getInt("inUse", SLOT_CLASSES.size).coerceIn(1, SLOT_CLASSES.size)

  /** AND-6: the shell's Settings set `inUse` (1..pool size). Apps already open keep their slots; the limit applies to the next open. */
  fun setSlotsInUse(ctx: Context, n: Int) {
    ctx.getSharedPreferences("bos_slots", Context.MODE_PRIVATE).edit().putInt("inUse", n.coerceIn(1, SLOT_CLASSES.size)).apply()
  }

  private class Slot {
    var app: String? = null
    var name: String = ""
    var shown = 0L
    var pid = 0
    var messenger: Messenger? = null
    var death: IBinder.DeathRecipient? = null
    /** Set while the hub itself ends the slot (a close or a reuse): its death is then not reported as a crash. */
    var ending: String? = null
    /** Run once the slot's old process is gone. */
    var pending: (() -> Unit)? = null
  }

  private val slots = List(SLOT_CLASSES.size) { Slot() }
  /** The page files of each app the shell opened (AND-3b), by app id. */
  private val pages = HashMap<String, PageFiles>()
  private val main = Handler(Looper.getMainLooper())

  /** Sends one event to the shell page (set by MainActivity once the shell's channel is up). */
  @Volatile var toShell: ((JSONObject) -> Unit)? = null

  private fun emit(ev: String, id: String, extra: JSONObject.() -> Unit = {}) {
    val o = JSONObject().put("ev", ev).put("id", id)
    o.extra()
    Log.i(TAG, o.toString().take(300))
    toShell?.invoke(o)
  }

  val messenger = Messenger(object : Handler(Looper.getMainLooper()) {
    override fun handleMessage(msg: Message) {
      val i = msg.arg1
      if (i !in slots.indices) return
      val s = slots[i]
      when (msg.what) {
        REGISTER -> {
          val m = msg.replyTo ?: return
          // A slot that Android started by itself (Recents after its process was ended) or that still shows an app the hub
          // gave away is ended: a slot runs only the app the hub gave it.
          val says = msg.data?.getString("app")
          if (s.app == null || says != s.app || s.ending != null) {
            try { m.send(Message.obtain(null, FINISH)) } catch (_: RemoteException) {}
            return
          }
          s.messenger = m
          s.pid = msg.arg2
          val d = IBinder.DeathRecipient { main.post { died(i, m) } }
          try {
            m.binder.linkToDeath(d, 0)
            s.death = d
          } catch (_: RemoteException) {
            main.post { died(i, m) }
            return
          }
          s.app?.let { id -> emit("opened", id) { put("slot", i); put("pid", s.pid) } }
        }
        REQUEST -> {
          // Only the slot's registered messenger speaks for it; the app is the slot's, whatever the message says.
          if (s.messenger == null || msg.replyTo?.binder != s.messenger?.binder) return
          val id = s.app ?: return
          val j = try { JSONObject(msg.data.getString("json") ?: return) } catch (_: Exception) { return }
          emit("request", id) { put("origin", j.optString("origin")); put("data", j.opt("data")) }
        }
        LOGIN -> {
          if (s.messenger == null || msg.replyTo?.binder != s.messenger?.binder) return
          val id = s.app ?: return
          val j = try { JSONObject(msg.data.getString("json") ?: return) } catch (_: Exception) { return }
          if (j.optBoolean("closed")) emit("login", id) { put("closed", true) }
          else j.optString("url").takeIf { it.startsWith("http://") || it.startsWith("https://") }?.let { u -> emit("login", id) { put("url", u) } }
        }
        LEFT -> if (msg.replyTo?.binder == s.messenger?.binder) s.app?.let { emit("left", it) }
        NATIVE_DONE -> {
          if (s.messenger == null || msg.replyTo?.binder != s.messenger?.binder) return
          val id = s.app ?: return
          val j = try { JSONObject(msg.data.getString("json") ?: return) } catch (_: Exception) { return }
          emit("native", id) {
            put("req", j.optString("req")); put("ok", j.optBoolean("ok"))
            if (j.has("result")) put("result", j.opt("result"))
            if (j.has("error")) put("error", j.optString("error"))
          }
        }
      }
    }
  })

  /** AND-3b: the files the slot serves for app `id` (from the OS's `os_app_page`): its version folder inside this app's private
   *  storage, the entry page's folder in it, and the page's CSP. `null` when they are not acceptable (then nothing is opened). */
  fun pageFiles(ctx: Context, id: String, origin: String, files: JSONObject?): PageFiles? {
    if (files == null || origin != PageFiles.originOf(id)) return null
    val root = try { java.io.File(files.optString("root")).canonicalFile } catch (_: Exception) { return null }
    val home = ctx.dataDir.canonicalFile
    if (!root.path.startsWith(home.path + java.io.File.separator) || !root.isDirectory) return null
    val csp = files.optString("csp")
    if (csp.isEmpty() || csp.any { it < ' ' }) return null
    return PageFiles(root.path, files.optString("pageDir"), csp)
  }

  /** The shell opens (or shows) app `id` with its page `url` at `origin`, served by its slot from `files` (AND-3b). */
  fun open(ctx: Context, id: String, url: String, origin: String, name: String, files: JSONObject? = null) {
    val pf = pageFiles(ctx, id, origin, files)
    if (pf == null) {
      emit("error", id) { put("text", "$name could not be opened: its page is not where BattalionOS keeps apps.") }
      return
    }
    pages[id] = pf
    val have = slots.indexOfFirst { it.app == id && it.ending == null }
    if (have >= 0) {
      slots[have].shown = SystemClock.elapsedRealtime()
      start(ctx, have, id, url, origin, name)
      return
    }
    val limit = slotsInUse(ctx)
    val free = (0 until limit).firstOrNull { slots[it].app == null && slots[it].pending == null }
    if (free != null) {
      take(ctx, free, id, url, origin, name)
      return
    }
    // All slots are busy: the one shown longest ago gives way (the shell is in front, so none of them is on screen).
    val i = (0 until limit).filter { slots[it].ending == null }.minByOrNull { slots[it].shown }
    if (i == null) {
      emit("error", id) { put("text", "No app slot is free yet; try again in a moment.") }
      return
    }
    val old = slots[i].app ?: return
    val oldName = slots[i].name
    emit("ended", old) {
      put("reason", "reused")
      put("text", "All $limit app slots were in use, so $oldName (the one used longest ago) was closed to open $name.")
    }
    end(i, "reused") { take(ctx, i, id, url, origin, name) }
  }

  private fun take(ctx: Context, i: Int, id: String, url: String, origin: String, name: String) {
    val s = slots[i]
    s.app = id
    s.name = name
    s.shown = SystemClock.elapsedRealtime()
    s.ending = null
    start(ctx, i, id, url, origin, name, fresh = true)
  }

  private fun start(ctx: Context, i: Int, id: String, url: String, origin: String, name: String, fresh: Boolean = false) {
    // A fresh start clears whatever task the slot had (an ended app's task must never come back with the new process).
    val it = Intent(ctx, SLOT_CLASSES[i])
      .putExtra("slot", i).putExtra("app", id).putExtra("url", url).putExtra("origin", origin).putExtra("name", name)
      .apply { pages[id]?.let { f -> putExtra("root", f.root).putExtra("pageDir", f.pageDir).putExtra("csp", f.csp) } }
      .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or (if (fresh) Intent.FLAG_ACTIVITY_CLEAR_TASK else 0))
    ctx.startActivity(it)
  }

  /** The shell closed app `id`: its slot's process ends. */
  fun close(id: String) {
    val i = slots.indexOfFirst { it.app == id && it.ending == null }
    if (i >= 0) end(i, "closed", null)
  }

  /** The shell's answer for the app's frame. */
  fun reply(id: String, json: String) {
    val s = slots.firstOrNull { it.app == id && it.ending == null } ?: return
    val m = Message.obtain(null, REPLY)
    m.data = Bundle().apply { putString("json", json) }
    try { s.messenger?.send(m) } catch (_: RemoteException) {}
  }

  /** AND-4: the shell hands app `id`'s slot a native call (already checked by the host). No slot = a plain refusal. */
  fun native(id: String, req: String, call: String, args: JSONObject, ctx: Context? = null) {
    val s = slots.firstOrNull { it.app == id && it.ending == null }
    // The code scanner answers in the process that asked, through its own activity, which lives in this (the OS's) process: so
    // the hub runs it here, for the app whose slot is open, and the page still gets only the text.
    if (call == "scan_code" && s?.messenger != null && ctx != null) {
      val i = slots.indexOf(s)
      NativeCalls.scan(ctx) { ok, v ->
        // The scanner ran in the OS's task: the app's slot comes back to the front (same process, same page).
        try { ctx.startActivity(Intent(ctx, SLOT_CLASSES[i]).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)) } catch (_: Exception) {}
        emit("native", id) { put("req", req); put("ok", ok); if (ok) put("result", v ?: JSONObject.NULL) else put("error", v) }
      }
      return
    }
    val m = s?.messenger
    if (m == null) {
      emit("native", id) { put("req", req); put("ok", false); put("error", "the app is not open") }
      return
    }
    val msg = Message.obtain(null, NATIVE)
    msg.data = Bundle().apply { putString("json", JSONObject().put("req", req).put("call", call).put("args", args).toString()) }
    try { m.send(msg) } catch (_: RemoteException) { emit("native", id) { put("req", req); put("ok", false); put("error", "the app is not open") } }
  }

  private fun end(i: Int, why: String, then: (() -> Unit)?) {
    val s = slots[i]
    s.ending = why
    s.pending = then
    val m = s.messenger
    if (m == null) {
      // Not registered yet (still starting): it registers into nothing and is ended then.
      died(i, null)
      return
    }
    try { m.send(Message.obtain(null, FINISH)) } catch (_: RemoteException) { died(i, m); return }
    // A slot that does not end by itself is ended by pid (the same app user, so Android allows it).
    val pid = s.pid
    main.postDelayed({ if (slots[i].messenger === m && pid > 0) android.os.Process.killProcess(pid) }, 1500)
  }

  private fun died(i: Int, m: Messenger?) {
    val s = slots[i]
    if (m != null && s.messenger !== m) return
    val id = s.app
    val why = s.ending
    try { s.death?.let { s.messenger?.binder?.unlinkToDeath(it, 0) } } catch (_: Exception) {}
    val next = s.pending
    val name = s.name
    s.app = null; s.messenger = null; s.death = null; s.pid = 0; s.ending = null; s.pending = null
    if (id != null && why == null) emit("ended", id) {
      put("reason", "ended")
      put("text", "$name was ended by Android (it crashed, or the phone needed the memory).")
    }
    next?.invoke()
  }

  /** The slots now (the shell's "slots" event), with the `inUse` setting and the pool size (AND-6, Settings). */
  fun state(ctx: Context? = null): JSONObject {
    val a = JSONArray()
    slots.forEachIndexed { i, s -> a.put(JSONObject().put("slot", i).put("app", s.app ?: JSONObject.NULL).put("pid", s.pid)) }
    val o = JSONObject().put("ev", "slots").put("id", "").put("slots", a).put("max", SLOT_CLASSES.size)
    if (ctx != null) o.put("inUse", slotsInUse(ctx))
    return o
  }
}

/** The hub's door for the slot processes (not exported: only this app's own processes bind it). */
class SlotHubService : Service() {
  override fun onBind(intent: Intent?): IBinder = SlotHub.messenger.binder
}
