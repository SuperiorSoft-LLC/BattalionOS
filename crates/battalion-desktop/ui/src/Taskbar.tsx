// SPDX-License-Identifier: MPL-2.0
// The taskbar: Start on the left, one button per live app (running dot, badge when it failed). The Start menu lists the installed
// apps, opens the Armory and exits BattalionOS. On a phone (AND-UI-1) the bar is the app switcher: it scrolls sideways, the buttons are
// finger sized, and Start spans the screen's width.
import { Boxes, LayoutGrid, Power, Settings, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Badge, Button, Separator } from "../kit";
import { AppIcon } from "./AppIcon";
import type { BarItem, BarWindow } from "./store";

/** WM-3: what a click on a window in the taskbar asks the WM for (`os_win_focus` restores a minimized one first). */
export type WinOp = "focus" | "close";

/** WM-3: the windows of one app, above its button: title, icon, badge and a close X. A minimized window is always here. */
export function WindowList({ app, appIcon, windows, phone, onWin }: { app: string; appIcon: string | null; windows: BarWindow[]; phone: boolean; onWin: (wid: string, op: WinOp) => void }) {
  return (
    <div role="menu" aria-label={`${app} windows`} className={`absolute z-20 flex flex-col gap-1 rounded-lg border bg-popover p-2 shadow-lg ${phone ? "inset-x-2 bottom-16 max-h-[60vh] overflow-y-auto" : "bottom-12 w-64"}`}>
      {windows.map((w) => (
        <div key={w.wid} className="flex items-center gap-1" data-wid={w.wid}>
          <Button variant="ghost" size={phone ? "lg" : "sm"} className={`min-w-0 flex-1 justify-start ${w.attention ? "motion-safe:animate-pulse" : ""}`} role="menuitem" data-attention={w.attention || undefined} onClick={() => onWin(w.wid, "focus")} aria-label={`${w.title || app}${w.minimized ? " (minimized)" : ""}${w.mark ? ` (${w.mark})` : ""}${w.attention ? " (needs attention)" : ""}`}>
            <AppIcon icon={w.icon ?? appIcon} className="size-4" />
            <span className={`truncate ${w.minimized ? "text-muted-foreground" : ""}`}>{w.title || app}</span>
            {w.mark && <Badge variant="destructive" data-badge={w.mark}>{w.mark}</Badge>}
          </Button>
          <Button variant="ghost" size="icon" className="shrink-0" aria-label={`Close ${w.title || app}`} onClick={() => onWin(w.wid, "close")}>
            <X />
          </Button>
        </div>
      ))}
    </div>
  );
}

export function Taskbar({ items, installedIds, label, icon, onOpen, onTask, onArmory, onExit, start, onStart: setStart, phone = false, onSettings, onWin }: { onWin?: (id: string, wid: string, op: WinOp) => void; items: BarItem[]; installedIds: string[]; label: (id: string) => string; icon: (id: string) => string | null; onOpen: (id: string) => void; onTask: (id: string) => void; onArmory: () => void; onExit: () => void; start: boolean; onStart: (open: boolean) => void; phone?: boolean; onSettings?: () => void }) {
  const menu = useRef<HTMLDivElement>(null);
  // WM-3: the app whose window list is open (one at a time; Start closes it).
  const [list, setList] = useState<string | null>(null);
  const listed = list ? items.find((t) => t.key === list && t.windows.length > 1) : undefined;
  useEffect(() => {
    if (!start && !list) return;
    const shut = () => { setStart(false); setList(null); };
    const away = (e: PointerEvent) => !menu.current?.contains(e.target as Node) && shut();
    const esc = (e: KeyboardEvent) => e.key === "Escape" && shut();
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("pointerdown", away);
      document.removeEventListener("keydown", esc);
    };
  }, [start, list, setStart]);
  const winOp = (id: string, wid: string, op: WinOp) => {
    if (op === "focus") { setList(null); onTask(id); }
    onWin?.(id, wid, op);
  };
  const click = (t: BarItem) => {
    if (t.windows.length > 1) return setList(list === t.key ? null : t.key);
    setList(null);
    onTask(t.id);
    // One window (or an ungrouped button): a click restores and focuses it through the WM.
    const w = t.windows[0];
    if (w && (t.wid || w.minimized)) onWin?.(t.id, w.wid, "focus");
  };
  const pick = (f: () => void) => () => {
    setStart(false);
    f();
  };

  return (
    <div ref={menu} data-layout={phone ? "phone" : "desktop"} className={`relative flex shrink-0 items-center gap-1 border-t bg-panel px-2 ${phone ? "h-14" : "h-12"}`}>
      {listed && !start && <WindowList app={label(listed.id)} appIcon={icon(listed.id)} windows={listed.windows} phone={phone} onWin={(wid, op) => winOp(listed.id, wid, op)} />}
      {start && (
        <div role="menu" aria-label="Start" className={`absolute z-20 flex flex-col gap-1 rounded-lg border bg-popover p-2 shadow-lg ${phone ? "inset-x-2 bottom-16 max-h-[70vh] overflow-y-auto" : "bottom-12 left-2 w-64"}`}>
          <Button variant="ghost" className="justify-start" role="menuitem" onClick={pick(onArmory)}>
            <Boxes /> Armory
          </Button>
          <Separator />
          <div className="px-2 pt-1 text-[11px] uppercase tracking-wide text-muted-foreground">Apps</div>
          {installedIds.length === 0 && <div className="px-2 py-1 text-xs text-muted-foreground">Nothing installed yet.</div>}
          {installedIds.map((id) => (
            <Button key={id} variant="ghost" className="justify-start" role="menuitem" onClick={pick(() => onOpen(id))}>
              <AppIcon icon={icon(id)} className="size-4" /> {label(id)}
            </Button>
          ))}
          <Separator />
          {onSettings && (
            <Button variant="ghost" className="justify-start" role="menuitem" onClick={pick(onSettings)}>
              <Settings /> Settings
            </Button>
          )}
          <Button variant="ghost" className="justify-start" role="menuitem" onClick={pick(onExit)}>
            <Power /> Exit BattalionOS
          </Button>
        </div>
      )}
      <Button variant={start ? "secondary" : "ghost"} size={phone ? "lg" : "sm"} className="shrink-0" aria-haspopup="menu" aria-expanded={start} onClick={() => { setList(null); setStart(!start); }}>
        <LayoutGrid /> Start
      </Button>
      <Separator orientation="vertical" className="mx-1 h-6" />
      <div className={phone ? "flex min-w-0 flex-1 items-center gap-1 overflow-x-auto" : "contents"}>
      {items.map((t) => {
        const title = t.wid ? t.windows[0]?.title || label(t.id) : label(t.id);
        const count = t.windows.length > 1 ? t.windows.length : 0;
        return (
        <Button key={t.key} variant={t.active ? "secondary" : "ghost"} size={phone ? "lg" : "sm"} className={`relative flex-col gap-0 pb-1 ${phone ? "max-w-40 shrink-0" : ""} ${t.attention ? "motion-safe:animate-pulse" : ""}`} data-active={t.active} data-attention={t.attention || undefined} aria-haspopup={count ? "menu" : undefined} aria-expanded={count ? list === t.key : undefined} onClick={() => click(t)} data-ended={t.ended || undefined} aria-label={`${title}${count ? ` (${count} windows)` : ""}${t.ended ? " (ended, tap to open again)" : ""}${t.running ? " (running)" : ""}${t.badge ? " (needs attention)" : ""}${t.mark ? ` (${t.mark})` : ""}`}>
          <span className="flex items-center gap-1.5">
            <AppIcon icon={(t.wid && t.windows[0]?.icon) || icon(t.id)} className={`size-4 ${t.ended ? "opacity-50" : ""}`} /> <span className={`truncate ${t.ended ? "text-muted-foreground" : ""}`}>{title}</span>
            {count > 0 && <span className="text-[10px] text-muted-foreground" data-count={count}>{count}</span>}
            {t.ended && <span className="text-[10px] text-muted-foreground">ended</span>}
            {t.badge && <Badge variant="destructive">!</Badge>}
            {!t.badge && t.mark && <Badge variant="destructive" data-badge={t.mark}>{t.mark}</Badge>}
          </span>
          {t.running && <span className="absolute inset-x-3 bottom-0 h-0.5 rounded-full bg-ok" data-running="true" aria-hidden />}
        </Button>
        );
      })}
      </div>
    </div>
  );
}
