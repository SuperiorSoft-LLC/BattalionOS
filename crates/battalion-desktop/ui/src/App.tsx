// SPDX-License-Identifier: MPL-2.0
// The shell: reads the installed list from the host, owns the model, wires the views. An app opens inside this window (os_open_app_window
// -> os://app-open): its page fills the app area above the taskbar, in a frame that is hidden, never reloaded, on minimize.
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import { Button, RefusalBanner } from "../kit";
import { closeAsks, closeNoticeSeen, CLOSE_NOTICE_TEXT } from "./closeNotice";
import { firstRunInstalled, firstRunNoticeText } from "./firstRunNotice";
import { Armory } from "./Armory";
import { AppFrames } from "./AppFrames";
import { Desktop } from "./Desktop";
import { PhoneSettings } from "./PhoneSettings";
import { send, slotChannel } from "./slots";
import { FirstRun } from "./FirstRun";
import { errText, hostPlatform, type ArmoryOp, type Os } from "./os";
import { backStep, layoutFor, useBackGesture, type Layout } from "./phone";
import { armoryHasUpdate, osReadyText, runOsUpdate, type OsPhase, type OsUpdateInfo } from "./osUpdate";
import { appView, barItems, needsStart, desktopIds, ARMORY_ICON, iconIds, iconOf, initial, labelOf, reduce, runInstall, runOp, showFirstRun } from "./store";
import { Taskbar } from "./Taskbar";
import { appAt, takeEarlyTap } from "./earlyTaps";
import { ResizeEdges, TitleBar } from "./TitleBar";

export function App({ os, platform = hostPlatform }: { os: Os; platform?: () => Promise<string | null> }) {
  const [m, dispatch] = useReducer(reduce, initial);
  const [armory, setArmory] = useState(false);
  const [start, setStart] = useState(false);
  // AND-UI-1: the layout follows the host's platform. AND-6: until it answers, the user agent decides (a phone opens with one tap
  // from the first moment) and the desktop shows a loading state that holds taps, so no early tap is lost.
  const [layout, setLayout] = useState<Layout>(() => layoutFor(null, navigator.userAgent));
  const [platformKnown, setPlatformKnown] = useState(false);
  useEffect(() => void platform().then((p) => { setLayout(layoutFor(p, navigator.userAgent)); setPlatformKnown(true); }), [platform]);
  const phone = layout === "phone";
  const [notice, setNotice] = useState<string | null>(null);
  const [firstRunIds, setFirstRunIds] = useState<string[]>([]);
  // BOS-AND-SELFUPDATE-1: the version whose "ready" notice was put off (it comes back for a newer one).
  const [osPutOff, setOsPutOff] = useState<string | null>(null);
  const [osInfo, setOsInfo] = useState<OsUpdateInfo | null>(null);
  const [osPhase, setOsPhase] = useState<OsPhase>({ kind: "idle" });

  const loadInstalled = useCallback(async () => {
    try {
      dispatch({ type: "installed-loaded", apps: await os.listApps() });
    } catch (e) {
      dispatch({ type: "installed-failed", error: errText(e) });
    }
  }, [os]);
  const loadCatalog = useCallback(async () => {
    dispatch({ type: "catalog-loading" });
    try {
      dispatch({ type: "catalog-loaded", catalog: await os.catalog() });
    } catch (e) {
      dispatch({ type: "catalog-failed", error: errText(e) });
    }
  }, [os]);

  useEffect(() => {
    void loadInstalled();
    void loadCatalog();
    const offChange = os.onChange(() => void loadInstalled());
    const offProgress = os.onProgress((p) => dispatch({ type: "progress", p }));
    const offOpen = os.onOpen((id) => dispatch({ type: "view-open", id }));
    // BOS-30: the OS's own update. A host without it (or a feed that is down) leaves the row hidden; nothing is shown as an error.
    void os.osUpdateState().then(setOsInfo, () => {});
    const offOs = os.onOsUpdate(setOsInfo);
    // BOS-DEFAULT-INF: the default apps the first open installed are named once (the host sends `done` on that run only).
    // The run may finish before this listens, so the host holds the ids until taken: ask now, and again on `done`.
    const takeNotice = () => void os.takeFirstRunNotice().then((ids) => { if (ids.length) { setFirstRunIds(ids); void loadInstalled(); } }, () => {});
    takeNotice();
    const offFirstRun = os.onFirstRun((e) => { if (firstRunInstalled(e).length) takeNotice(); });
    return () => {
      offFirstRun();
      offOs();
      offChange();
      offProgress();
      offOpen();
    };
  }, [os, loadInstalled, loadCatalog]);

  // An app that stops or crashes has no event of its own: while a view is open the list is read again every 2 seconds.
  const watching = m.views.length > 0;
  useEffect(() => {
    if (!watching) return;
    const t = setInterval(() => void loadInstalled(), 2000);
    return () => clearInterval(t);
  }, [watching, loadInstalled]);

  const open = useCallback(
    async (id: string) => {
      try {
        // The host checks the app and says `os://app-open`; the view shows "Starting…" while a stopped app starts.
        await os.openAppWindow(id);
        dispatch({ type: "view-open", id });
        // The list this callback holds can be older than the app (the first open of a new window, a fresh install): read it again.
        const fresh = m.installed.some((a) => a.id === id) ? m : reduce(m, { type: "installed-loaded", apps: await os.listApps() });
        if (needsStart(fresh, id)) {
          void loadInstalled();
          await os.startApp(id);
        }
        await loadInstalled();
      } catch (e) {
        setNotice(`Could not open ${id}: ${errText(e)}`);
        await loadInstalled();
      }
    },
    [os, m, loadInstalled],
  );
  // BOS-19: a start from outside (`BattalionOS --open <id>`, a Start menu shortcut): the app opens as by its icon; an app that is
  // not installed shows its place in the Armory.
  const takeRequest = useCallback(async () => {
    try {
      const id = await os.takeOpenRequest();
      if (!id) return;
      const apps = await os.listApps();
      if (apps.some((a) => a.id === id)) {
        await open(id);
      } else {
        setNotice(`${id} is not installed here. Install it from the Armory.`);
        setArmory(true);
      }
    } catch (e) {
      setNotice(`Could not open the requested app: ${errText(e)}`);
    }
  }, [os, open]);
  useEffect(() => {
    void takeRequest();
    return os.onOpenRequest(() => void takeRequest());
    // Once per window start and per request; `open` changes with the model and must not re-run a request.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [os]);
  const minimize = useCallback((id: string) => dispatch({ type: "view-minimize", id }), []);
  const closeView = useCallback((id: string) => dispatch({ type: "view-close", id }), []);
  const endedView = useCallback((id: string) => dispatch({ type: "view-ended", id }), []);
  // AND-6: phone Settings ("Apps open at once"); the host owns the value and reports it with the slots.
  const [settings, setSettings] = useState(false);
  const [slotLimit, setSlotLimit] = useState<{ inUse: number; max: number } | null>(null);
  const setInUse = useCallback((n: number) => { const c = slotChannel(); if (c) send(c, { op: "config", inUse: n }); }, []);
  const retry = useCallback(
    async (id: string) => {
      try {
        await os.startApp(id);
      } catch (e) {
        setNotice(`Could not start ${id}: ${errText(e)}`);
      }
      await loadInstalled();
    },
    [os, loadInstalled],
  );
  // A taskbar button: the view that is showing goes back to the desktop (it keeps its state); any other opens or shows.
  const task = useCallback((id: string) => (m.active === id ? minimize(id) : void open(id)), [m.active, minimize, open]);
  const install = useCallback((id: string) => void runInstall(os, dispatch, id, errText), [os]);
  const op = useCallback(
    (id: string, kind: ArmoryOp) => {
      const run = { update: (i: string) => os.update(i), rollback: os.rollback, remove: os.remove, install: (i: string) => os.install(i) }[kind];
      void runOp({ run: (i) => run(i), listApps: () => os.listApps() }, dispatch, id, kind, errText).then((ok) => ok && void loadCatalog());
    },
    [os, loadCatalog],
  );
  // UPDATE-LIVE-1: "Update now" (the app checkpoints at once) and "Later" (applies at its next start or idle moment).
  const updateNow = useCallback(
    (id: string) => void runOp({ run: (i) => os.update(i, true), listApps: () => os.listApps() }, dispatch, id, "update", errText).then((ok) => ok && void loadCatalog()),
    [os, loadCatalog],
  );
  const later = useCallback(
    (id: string) =>
      void os.updateLater(id).then(
        async () => {
          dispatch({ type: "op-dismissed", id });
          dispatch({ type: "installed-loaded", apps: await os.listApps() });
        },
        (e) => dispatch({ type: "op-failed", id, op: "update", error: errText(e) }),
      ),
    [os],
  );
  const exit = useCallback(() => void os.exit().catch((e) => setNotice(`Could not exit: ${errText(e)}`)), [os]);

  // BOS-34: X hides the window; the first time it says so first, with Exit beside it.
  const [closeNotice, setCloseNotice] = useState(false);
  const hideWindow = useCallback(() => { closeNoticeSeen(localStorage); setCloseNotice(false); void os.window.close(); }, [os]);
  const closeWindow = useCallback(() => (closeAsks(localStorage) ? setCloseNotice(true) : void os.window.close()), [os]);

  // The back gesture on a phone: the Start menu closes first, then the Armory, then an open app goes back to the desktop (it keeps its state).
  useBackGesture(phone, backStep(armory, m.active, start) !== null, () => {
    const step = backStep(armory, m.active, start);
    if (step === "close-start") setStart(false);
    else if (step === "close-armory") setArmory(false);
    else if (step === "to-desktop" && m.active) minimize(m.active);
  });

  const ids = useMemo(() => desktopIds(m), [m]);
  const icons = useMemo(() => iconIds(m), [m]);
  const openArmory = useCallback(() => { setArmory(true); void loadCatalog(); void os.osUpdateCheck().then(setOsInfo, () => {}); }, [loadCatalog, os]);
  // AND-6 (f4f965ad): once the desktop is drawn, a tap made earlier (splash, blank page, loading state) opens the icon under it.
  const ready = m.loaded && platformKnown;
  const openRef = useRef<(id: string) => void>(() => {});
  openRef.current = (id) => (id === ARMORY_ICON ? openArmory() : void open(id));
  useEffect(() => {
    if (!ready) return;
    void takeEarlyTap().then((t) => {
      if (!t || !phone) return;
      requestAnimationFrame(() => { const id = appAt(t.x, t.y); if (id) openRef.current(id); });
    });
  }, [ready]); // eslint-disable-line react-hooks/exhaustive-deps
  const view = useCallback((id: string) => appView(m, id), [m]);
  const label = useCallback((id: string) => (id === ARMORY_ICON ? "Armory" : labelOf(m, id)), [m]);
  const icon = useCallback((id: string) => iconOf(m, id), [m]);

  return (
    <div className="flex h-screen flex-col bg-background text-foreground" data-layout={layout} style={phone ? PHONE_INSETS : undefined}>
      {!phone && <ResizeEdges win={os.window} />}
      <TitleBar win={os.window} appName={m.active ? labelOf(m, m.active) : null} onClose={closeWindow} phone={phone} />
      {closeNotice && (
        <div role="alertdialog" aria-label="BattalionOS keeps running" className="mx-3 mt-2 flex flex-col gap-2 rounded-lg border bg-card p-3">
          <p className="text-sm">{CLOSE_NOTICE_TEXT}</p>
          <div className="flex gap-2">
            <Button size="sm" onClick={hideWindow}>Hide the window</Button>
            <Button size="sm" variant="ghost" onClick={() => { setCloseNotice(false); exit(); }}>Exit BattalionOS</Button>
          </div>
        </div>
      )}
      {firstRunIds.length > 0 && (
        <div role="status" className="mx-3 mt-2 flex items-center gap-2 rounded-lg border bg-card px-3 py-2">
          <p className="flex-1 text-sm">{firstRunNoticeText(firstRunIds.map(label))}</p>
          <Button size="sm" variant="ghost" onClick={openArmory}>Open Armory</Button>
          <Button size="sm" variant="ghost" aria-label="Dismiss" onClick={() => setFirstRunIds([])}>OK</Button>
        </div>
      )}
      {osReadyText(osInfo, osPutOff) && !armory && (
        <div role="status" data-os-ready className="mx-3 mt-2 flex items-center gap-2 rounded-lg border bg-card px-3 py-2">
          <p className="flex-1 text-sm">{osReadyText(osInfo, osPutOff)}</p>
          <Button size="sm" onClick={openArmory}>Update</Button>
          <Button size="sm" variant="ghost" aria-label="Later" onClick={() => setOsPutOff(osInfo?.available?.version ?? null)}>Later</Button>
        </div>
      )}
      <div className="px-3 pt-2 empty:hidden">
        <RefusalBanner text={notice ?? (m.listError ? `Could not read the installed apps: ${m.listError}` : null)} onDismiss={() => setNotice(null)} />
      </div>
      <Desktop ids={icons} view={view} label={label} icon={icon} tapOpens={phone} badge={(id) => (id === ARMORY_ICON ? (armoryHasUpdate(m.installed, osInfo) ? "↑" : null) : m.installed.find((a) => a.id === id)?.badge ?? null)} onOpen={(id) => (id === ARMORY_ICON ? openArmory() : void open(id))}>
        <AppFrames m={m} os={os} close={closeView} ended={endedView} onSlotLimit={setSlotLimit} minimize={minimize} retry={(id) => void retry(id)} notify={setNotice} />
        {!ready && <div className="absolute inset-0 z-10 grid place-items-center bg-background/80 text-sm text-muted-foreground" role="status" aria-busy="true">Loading…</div>}
        {showFirstRun(m) && !armory && <FirstRun m={m} install={install} openArmory={() => setArmory(true)} dismiss={() => dispatch({ type: "dismiss-first-run" })} />}
        {armory && <Armory m={m} osRow={{ info: osInfo, phase: osPhase, install: () => void runOsUpdate(os, setOsPhase, errText), dismiss: () => setOsPhase({ kind: "idle" }) }} a={{ install, update: (id) => op(id, "update"), updateNow, later, rollback: (id) => op(id, "rollback"), remove: (id) => op(id, "remove"), dismiss: (id) => dispatch({ type: "op-dismissed", id }), open: (id) => void open(id), reload: () => void loadCatalog(), close: () => setArmory(false) }} />}
      </Desktop>
      <Taskbar items={barItems(m)} installedIds={ids} label={label} icon={icon} onOpen={(id) => void open(id)} onTask={task} onArmory={openArmory} onExit={exit} start={start} onStart={setStart} phone={phone} onSettings={phone && slotChannel() ? () => setSettings(true) : undefined} />
      {settings && <PhoneSettings open={settings} onOpenChange={setSettings} limit={slotLimit} setInUse={setInUse} />}
    </div>
  );
}

/** A phone's system bars: the Android host pads its view by them already (MainActivity); where a WebView reports them as safe-area
 *  insets instead, the shell keeps clear of them itself. */
const PHONE_INSETS: React.CSSProperties = {
  paddingTop: "env(safe-area-inset-top)",
  paddingBottom: "env(safe-area-inset-bottom)",
  paddingLeft: "env(safe-area-inset-left)",
  paddingRight: "env(safe-area-inset-right)",
  boxSizing: "border-box",
};
