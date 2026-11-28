// SPDX-License-Identifier: MPL-2.0
// AND-6: BattalionOS Settings on a phone. One setting so far: how many apps may be open at once (each open app holds one of the
// phone's process slots; opening one more closes the one used longest ago). The host owns the value (shared prefs `bos_slots`).
// FS-API-1: on the desktop the same dialog shows the OS root folder (apps with `files.read` reach only what is under it; the
// `fs.root` setting in os.json moves it).
// OPEN-WITH-1: on the desktop it also lists the default app per file type ("always use this app"), each with Forget.
import { AppDialog, Button, Field, ToggleGroup, ToggleGroupItem } from "../kit";

export function PhoneSettings({ open, onOpenChange, limit, setInUse, phone = true, filesRoot, defaultApps, forgetDefault }: { open: boolean; onOpenChange: (open: boolean) => void; limit: { inUse: number; max: number } | null; setInUse: (n: number) => void; phone?: boolean; filesRoot?: string | null; defaultApps?: Record<string, string> | null; forgetDefault?: (ext: string) => void }) {
  return (
    <AppDialog open={open} onOpenChange={onOpenChange} title="Settings">
      {defaultApps !== undefined && (
        <Field id="default-apps" label="Default apps" hint="The app that opens each kind of file without asking. Forget one to be asked again.">
          <div id="default-apps" className="flex flex-col gap-1" role="list">
            {defaultApps === null ? (
              <p className="text-sm text-muted-foreground" role="status">Reading…</p>
            ) : Object.keys(defaultApps).length === 0 ? (
              <p className="text-sm text-muted-foreground">None yet.</p>
            ) : (
              Object.entries(defaultApps).map(([ext, app]) => (
                <div key={ext} role="listitem" className="flex items-center justify-between gap-2 text-sm">
                  <span>.{ext}: {app}</span>
                  <Button size="sm" variant="outline" onClick={() => forgetDefault?.(ext)}>Forget</Button>
                </div>
              ))
            )}
          </div>
        </Field>
      )}
      {filesRoot !== undefined && (
        <Field id="files-root" label="Files root folder" hint="Apps with the files permission reach only what is in this folder. Set fs.root in os.json to move it.">
          <p id="files-root" className="text-sm break-all" role="status">{filesRoot ?? "Reading…"}</p>
        </Field>
      )}
      {!phone ? null : limit ? (
        <Field id="apps-open" label="Apps open at once" hint="When more apps are opened, the one used longest ago is closed. Fewer saves memory.">
          <ToggleGroup id="apps-open" type="single" variant="outline" value={String(limit.inUse)} onValueChange={(v) => v && setInUse(Number(v))} aria-label="Apps open at once">
            {Array.from({ length: limit.max }, (_, i) => String(i + 1)).map((n) => (
              <ToggleGroupItem key={n} value={n} className="min-w-11 data-[state=on]:bg-primary data-[state=on]:text-primary-foreground" aria-label={`${n} at once`}>{n}</ToggleGroupItem>
            ))}
          </ToggleGroup>
        </Field>
      ) : (
        <p className="text-sm text-muted-foreground" role="status">Reading the settings…</p>
      )}
    </AppDialog>
  );
}
