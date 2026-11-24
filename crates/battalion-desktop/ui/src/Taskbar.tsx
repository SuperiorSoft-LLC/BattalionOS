// SPDX-License-Identifier: MPL-2.0
// The taskbar: Start on the left, one button per live app (running dot, badge when it failed). The Start menu lists the installed
// apps, opens the Armory and exits BattalionOS.
import { AppWindow, Boxes, LayoutGrid, Power } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Badge, Button, Separator } from "../kit";
import { appLabel, type TaskItem } from "./store";

export function Taskbar({ items, installedIds, onOpen, onArmory, onExit }: { items: TaskItem[]; installedIds: string[]; onOpen: (id: string) => void; onArmory: () => void; onExit: () => void }) {
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
    <div ref={menu} className="relative flex h-12 shrink-0 items-center gap-1 border-t bg-panel px-2">
      {start && (
        <div role="menu" aria-label="Start" className="absolute bottom-12 left-2 z-20 flex w-64 flex-col gap-1 rounded-lg border bg-popover p-2 shadow-lg">
          <Button variant="ghost" className="justify-start" role="menuitem" onClick={pick(onArmory)}>
            <Boxes /> Armory
          </Button>
          <Separator />
          <div className="px-2 pt-1 text-[11px] uppercase tracking-wide text-muted-foreground">Apps</div>
          {installedIds.length === 0 && <div className="px-2 py-1 text-xs text-muted-foreground">Nothing installed yet.</div>}
          {installedIds.map((id) => (
            <Button key={id} variant="ghost" className="justify-start" role="menuitem" onClick={pick(() => onOpen(id))}>
              <AppWindow /> {appLabel(id)}
            </Button>
          ))}
          <Separator />
          <Button variant="ghost" className="justify-start" role="menuitem" onClick={pick(onExit)}>
            <Power /> Exit BattalionOS
          </Button>
        </div>
      )}
      <Button variant={start ? "secondary" : "ghost"} size="sm" aria-haspopup="menu" aria-expanded={start} onClick={() => setStart(!start)}>
        <LayoutGrid /> Start
      </Button>
      <Separator orientation="vertical" className="mx-1 h-6" />
      {items.map((t) => (
        <Button key={t.id} variant="ghost" size="sm" className="relative flex-col gap-0 pb-1" onClick={() => onOpen(t.id)} aria-label={`${appLabel(t.id)}${t.running ? " (running)" : ""}${t.badge ? " (needs attention)" : ""}`}>
          <span className="flex items-center gap-1.5">
            <AppWindow /> {appLabel(t.id)}
            {t.badge && <Badge variant="destructive">!</Badge>}
          </span>
          {t.running && <span className="absolute inset-x-3 bottom-0 h-0.5 rounded-full bg-ok" data-running="true" aria-hidden />}
        </Button>
      ))}
    </div>
  );
}
