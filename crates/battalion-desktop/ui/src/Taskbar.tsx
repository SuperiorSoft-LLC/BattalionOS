// SPDX-License-Identifier: MPL-2.0
// The taskbar: Start on the left, one button per live app (running dot, badge when it failed). The Start menu lists the installed
// apps, opens the Armory and exits BattalionOS. On a phone (AND-UI-1) the bar is the app switcher: it scrolls sideways, the buttons are
// finger sized, and Start spans the screen's width.
import { Boxes, LayoutGrid, Power } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Badge, Button, Separator } from "../kit";
import { AppIcon } from "./AppIcon";
import type { BarItem } from "./store";

export function Taskbar({ items, installedIds, label, icon, onOpen, onTask, onArmory, onExit, phone = false }: { items: BarItem[]; installedIds: string[]; label: (id: string) => string; icon: (id: string) => string | null; onOpen: (id: string) => void; onTask: (id: string) => void; onArmory: () => void; onExit: () => void; phone?: boolean }) {
  const [start, setStart] = useState(false);
  const menu = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!start) return;
    const away = (e: PointerEvent) => !menu.current?.contains(e.target as Node) && setStart(false);
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setStart(false);
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("pointerdown", away);
      document.removeEventListener("keydown", esc);
    };
  }, [start]);
  const pick = (f: () => void) => () => {
    setStart(false);
    f();
  };

  return (
    <div ref={menu} data-layout={phone ? "phone" : "desktop"} className={`relative flex shrink-0 items-center gap-1 border-t bg-panel px-2 ${phone ? "h-14" : "h-12"}`}>
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
          <Button variant="ghost" className="justify-start" role="menuitem" onClick={pick(onExit)}>
            <Power /> Exit BattalionOS
          </Button>
        </div>
      )}
      <Button variant={start ? "secondary" : "ghost"} size={phone ? "lg" : "sm"} className="shrink-0" aria-haspopup="menu" aria-expanded={start} onClick={() => setStart(!start)}>
        <LayoutGrid /> Start
      </Button>
      <Separator orientation="vertical" className="mx-1 h-6" />
      <div className={phone ? "flex min-w-0 flex-1 items-center gap-1 overflow-x-auto" : "contents"}>
      {items.map((t) => (
        <Button key={t.id} variant={t.active ? "secondary" : "ghost"} size={phone ? "lg" : "sm"} className={`relative flex-col gap-0 pb-1 ${phone ? "max-w-40 shrink-0" : ""}`} data-active={t.active} onClick={() => onTask(t.id)} aria-label={`${label(t.id)}${t.running ? " (running)" : ""}${t.badge ? " (needs attention)" : ""}${t.mark ? ` (${t.mark})` : ""}`}>
          <span className="flex items-center gap-1.5">
            <AppIcon icon={icon(t.id)} className="size-4" /> {label(t.id)}
            {t.badge && <Badge variant="destructive">!</Badge>}
            {!t.badge && t.mark && <Badge variant="destructive" data-badge={t.mark}>{t.mark}</Badge>}
          </span>
          {t.running && <span className="absolute inset-x-3 bottom-0 h-0.5 rounded-full bg-ok" data-running="true" aria-hidden />}
        </Button>
      ))}
      </div>
    </div>
  );
}
