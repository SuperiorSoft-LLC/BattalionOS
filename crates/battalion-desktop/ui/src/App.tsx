// SPDX-License-Identifier: MPL-2.0
// The shell: reads the installed list from the host, owns the model, wires the views. An app opens inside this window (os_open_app_window
// -> os://app-open): its page fills the app area above the taskbar, in a frame that is hidden, never reloaded, on minimize.
import { useCallback, useEffect, useMemo, useReducer, useState } from "react";
import { RefusalBanner } from "../kit";
import { Armory } from "./Armory";
import { AppFrames } from "./AppFrames";
import { Desktop } from "./Desktop";
import { FirstRun } from "./FirstRun";
import { errText, type ArmoryOp, type Os } from "./os";
import { appView, barItems, desktopIds, iconOf, initial, labelOf, reduce, runInstall, runOp, showFirstRun, titleBarOwner } from "./store";
import { Taskbar } from "./Taskbar";
import { OsTitleBar, ResizeEdges } from "./TitleBar";

export function App({ os }: { os: Os }) {
  const [m, dispatch] = useReducer(reduce, initial);
  const [armory, setArmory] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  // BOS-27: apps whose page said its own title bar is there (reset by AppFrames at every new page).
  const [said, setSaid] = useState<Record<string, boolean>>({});
  const bar = useCallback((id: string, on: boolean) => setSaid((s) => (s[id] === on || (!on && s[id] === undefined) ? s : { ...s, [id]: on })), []);

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
    return () => {
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
        const v = appView(m, id);
        if (v.kind === "installed" && !v.running) {
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
      const run = { update: os.update, rollback: os.rollback, remove: os.remove, install: os.install }[kind];
      void runOp({ run: (i) => run(i), listApps: () => os.listApps() }, dispatch, id, kind, errText).then((ok) => ok && void loadCatalog());
    },
    [os, loadCatalog],
  );
  const exit = useCallback(() => void os.exit().catch((e) => setNotice(`Could not exit: ${errText(e)}`)), [os]);

  const ids = useMemo(() => desktopIds(m), [m]);
  const view = useCallback((id: string) => appView(m, id), [m]);
  const label = useCallback((id: string) => labelOf(m, id), [m]);
  const icon = useCallback((id: string) => iconOf(m, id), [m]);

  return (
    <div className="flex h-screen flex-col bg-background text-foreground">
      <ResizeEdges win={os.window} />
      {titleBarOwner(m, said) === null && <OsTitleBar win={os.window} />}
      <div className="px-3 pt-2 empty:hidden">
        <RefusalBanner text={notice ?? (m.listError ? `Could not read the installed apps: ${m.listError}` : null)} onDismiss={() => setNotice(null)} />
      </div>
      <Desktop ids={ids} view={view} label={label} icon={icon} badge={(id) => m.installed.find((a) => a.id === id)?.badge ?? null} onOpen={(id) => void open(id)}>
        <AppFrames m={m} os={os} close={closeView} minimize={minimize} retry={(id) => void retry(id)} bar={bar} />
        {!m.loaded && <div className="absolute inset-0 grid place-items-center text-sm text-muted-foreground" role="status">Loading…</div>}
        {showFirstRun(m) && !armory && <FirstRun m={m} install={install} openArmory={() => setArmory(true)} dismiss={() => dispatch({ type: "dismiss-first-run" })} />}
        {armory && <Armory m={m} a={{ install, update: (id) => op(id, "update"), rollback: (id) => op(id, "rollback"), remove: (id) => op(id, "remove"), dismiss: (id) => dispatch({ type: "op-dismissed", id }), open: (id) => void open(id), reload: () => void loadCatalog(), close: () => setArmory(false) }} />}
      </Desktop>
      <Taskbar items={barItems(m)} installedIds={ids} label={label} icon={icon} onOpen={(id) => void open(id)} onTask={task} onArmory={() => { setArmory(true); void loadCatalog(); }} onExit={exit} />
    </div>
  );
}
