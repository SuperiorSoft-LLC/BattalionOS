package org.battalionos.os.devtest

// AND-3b: an app's page is served by its slot through request interception at `http://<id>.bos.localhost/`, with no port: any other
// Android app can reach a loopback port, so the OS starts no page server on a phone. The rules are the desktop page server's
// (battalion-desktop serve.rs `resolve`): GET/HEAD only, only files under the app's version folder (no `..`, backslash, colon, NUL or
// double encoding; the real path must stay inside), a file missing at the root retried in the entry page's folder, and every answer
// carries the page's CSP, nosniff and no-store.

import android.webkit.WebResourceResponse
import java.io.ByteArrayInputStream
import java.io.File
import java.io.FileInputStream

data class PageFiles(val root: String, val pageDir: String, val csp: String) {
  companion object {
    /** The page origin of app `id` (the OS's `intercept_origin`). */
    fun originOf(id: String): String = "http://$id.bos.localhost"

    /** The host name of app `id`'s page. */
    fun hostOf(id: String): String = "$id.bos.localhost"

    private fun pctDecode(s: String): String? {
      val out = java.io.ByteArrayOutputStream()
      var i = 0
      while (i < s.length) {
        val c = s[i]
        if (c == '%') {
          if (i + 3 > s.length) return null
          out.write(s.substring(i + 1, i + 3).toIntOrNull(16) ?: return null)
          i += 3
        } else {
          out.write(c.toString().toByteArray(Charsets.UTF_8))
          i++
        }
      }
      val b = out.toByteArray()
      val d = Charsets.UTF_8.newDecoder()
      return try { d.decode(java.nio.ByteBuffer.wrap(b)).toString() } catch (_: Exception) { null }
    }

    /** The file under `root` for request path `path`, or null (refused or missing). */
    fun resolve(root: File, path: String): File? {
      val dec = pctDecode(path) ?: return null
      if (dec.contains('%') || dec.contains('\\') || dec.contains('\u0000') || dec.contains(':')) return null
      var f = root
      for (seg in dec.trimStart('/').split('/')) {
        when (seg) {
          "", "." -> {}
          ".." -> return null
          else -> f = File(f, seg)
        }
      }
      val real = try { f.canonicalFile } catch (_: Exception) { return null }
      val realRoot = try { root.canonicalFile } catch (_: Exception) { return null }
      if (real.path != realRoot.path && !real.path.startsWith(realRoot.path + File.separator)) return null
      return if (real.isFile) real else null
    }

    fun mime(name: String): String = when (name.substringAfterLast('.', "").lowercase()) {
      "html", "htm" -> "text/html"
      "js", "mjs" -> "text/javascript"
      "css" -> "text/css"
      "json" -> "application/json"
      "svg" -> "image/svg+xml"
      "png" -> "image/png"
      "jpg", "jpeg" -> "image/jpeg"
      "gif" -> "image/gif"
      "webp" -> "image/webp"
      "ico" -> "image/x-icon"
      "woff" -> "font/woff"
      "woff2" -> "font/woff2"
      "ttf" -> "font/ttf"
      "wasm" -> "application/wasm"
      "txt" -> "text/plain"
      "map" -> "application/json"
      else -> "application/octet-stream"
    }
  }

  private fun headers() = mapOf("Content-Security-Policy" to csp, "X-Content-Type-Options" to "nosniff", "Cache-Control" to "no-store")

  private fun plain(code: Int, reason: String) =
    WebResourceResponse("text/plain", "utf-8", code, reason, headers(), ByteArrayInputStream(reason.toByteArray()))

  /** The answer for one request to the app's own host. */
  fun answer(method: String, path: String): WebResourceResponse {
    if (method != "GET" && method != "HEAD") return plain(405, "Method Not Allowed")
    val root = File(this.root)
    val f = resolve(root, path) ?: (if (pageDir.isNotEmpty()) resolve(File(root, pageDir), path) else null) ?: return plain(404, "Not Found")
    // The fallback folder must itself be inside the root (it is the entry page's folder, named by the manifest).
    if (!f.canonicalPath.startsWith(root.canonicalPath + File.separator)) return plain(404, "Not Found")
    val type = mime(f.name)
    val body = if (method == "HEAD") ByteArrayInputStream(ByteArray(0)) else FileInputStream(f)
    return WebResourceResponse(type, if (type.startsWith("text/") || type == "application/json") "utf-8" else null, 200, "OK", headers(), body)
  }
}
