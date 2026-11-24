// SPDX-License-Identifier: MPL-2.0
// The shell: reads the installed list from the host, owns the model, wires the views. An app opens in its own window (os_open_app_window).
import { useCallback, useEffect, useMemo, useReducer, useState } from "react";
import { RefusalBanner } from "../kit";
import { Armory } from "./Armory";
import { Desktop } from "./Desktop";
import { FirstRun } from "./FirstRun";
import { errText, type ArmoryOp, type Os } from "./os";
import { appView, desktopIds, iconOf, initial, labelOf, reduce, runInstall, runOp, showFirstRun, taskItems } from "./store";
import { Taskbar } from "./Taskbar";

export function App({ os }: { os: Os }) {
  const [m, dispatch] = useReducer(reduce, initial);
  const [armory, setArmory] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

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
    return () => {
      offChange();
      offProgress();
    };
  }, [os, loadInstalled, loadCatalog]);

  const open = useCallback(
    async (id: string) => {
      try {
        const v = appView(m, id);
        if (v.kind === "installed" && !v.running) await os.startApp(id);
        await os.openAppWindow(id);
        await loadInstalled();
      } catch (e) {
        setNotice(`Could not open ${id}: ${errText(e)}`);
      }
    },
    [os, m, loadInstalled],
  );
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
      <div className="px-3 pt-2 empty:hidden">
        <RefusalBanner text={notice ?? (m.listError ? `Could not read the installed apps: ${m.listError}` : null)} onDismiss={() => setNotice(null)} />
      </div>
      <Desktop ids={ids} view={view} label={label} icon={icon} badge={(id) => m.installed.find((a) => a.id === id)?.badge ?? null} onOpen={(id) => void open(id)}>
        {!m.loaded && <div className="absolute inset-0 grid place-items-center text-sm text-muted-foreground" role="status">Loading…</div>}
        {showFirstRun(m) && !armory && <FirstRun m={m} install={install} openArmory={() => setArmory(true)} dismiss={() => dispatch({ type: "dismiss-first-run" })} />}
        {armory && <Armory m={m} a={{ install, update: (id) => op(id, "update"), rollback: (id) => op(id, "rollback"), remove: (id) => op(id, "remove"), dismiss: (id) => dispatch({ type: "op-dismissed", id }), open: (id) => void open(id), reload: () => void loadCatalog(), close: () => setArmory(false) }} />}
      </Desktop>
      <Taskbar items={taskItems(m)} installedIds={ids} label={label} icon={icon} onOpen={(id) => void open(id)} onArmory={() => setArmory(true)} onExit={exit} />
    </div>
  );
}
