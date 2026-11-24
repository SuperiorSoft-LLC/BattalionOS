// SPDX-License-Identifier: Apache-2.0
// The demo page's script: an external file, because the page server's policy allows no inline script.
  const out = {};
  const show = () => (document.getElementById("out").textContent = JSON.stringify(out, null, 2));
  let n = 0;
  const pending = new Map();
  addEventListener("message", (e) => {
    if (e.source !== parent) return;
    const p = pending.get(e.data && e.data.id);
    if (p) (pending.delete(e.data.id), e.data.ok ? p.ok(e.data.result) : p.fail(e.data.error));
  });
  const call = (cmd, args) =>
    new Promise((ok, fail) => {
      const id = "r" + ++n;
      pending.set(id, { ok, fail });
      parent.postMessage({ bos: 1, id, cmd, args }, "*");
      setTimeout(() => fail("no answer"), 8000);
    });
  const note = (k, v) => ((out[k] = v), show());
  document.getElementById("desktop").onclick = () => call("os_show_desktop").catch((e) => note("desktop", String(e)));
  document.getElementById("badge").onclick = () => call("os_badge", { count: 3 }).then(() => note("badge", "set"), (e) => note("badge", String(e)));
  document.getElementById("close").onclick = () => call("os_close_app_window").catch((e) => note("close", String(e)));
  document.getElementById("steal").onclick = () => call("os_app_status", { id: "some-other-app" }).then((r) => note("other", r), (e) => note("other", "refused: " + e));
  document.getElementById("tauri").onclick = () => {
    note("tauriGlobal", typeof (window.__TAURI__ || window.__TAURI_INTERNALS__));
    const t = window.__TAURI_INTERNALS__ || (window.__TAURI__ && window.__TAURI__.core);
    if (!t) return;
    Promise.race([t.invoke ? t.invoke("os_list_apps") : Promise.reject("no invoke"), new Promise((_, no) => setTimeout(() => no("no answer: the frame has no window command rights"), 4000))]).then((r) => note("os_list_apps", "ALLOWED " + JSON.stringify(r).slice(0, 80)), (e) => note("os_list_apps", "refused: " + String(e).slice(0, 120)));
  };
  (async () => {
    try {
      note("window", await call("os_window_info"));
      const st = await call("os_app_status");
      note("status", { state: st.state, url: st.url });
      note("origin", location.origin);
      note("flag", new URLSearchParams(location.search).get("bos-app"));
      const s = document.createElement("script");
      s.src = st.url.replace(/\/$/, "") + "/extra.js";
      s.onload = () => note("extra", window.__demoExtra);
      s.onerror = () => note("extra", "blocked");
      document.head.appendChild(s);
      const r = await fetch(st.url.replace(/\/$/, "") + "/api/origin");
      note("app", r.ok ? "reached (" + r.status + ")" : "refused (" + r.status + ")");
      note("savedSetting", (localStorage.demoVisits = String(Number(localStorage.demoVisits || 0) + 1)));
    } catch (e) {
      note("error", String(e));
    }
  })();
