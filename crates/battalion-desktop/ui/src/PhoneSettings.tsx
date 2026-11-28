// SPDX-License-Identifier: MPL-2.0
// AND-6: BattalionOS Settings on a phone. One setting so far: how many apps may be open at once (each open app holds one of the
// phone's process slots; opening one more closes the one used longest ago). The host owns the value (shared prefs `bos_slots`).
// FS-API-1: on the desktop the same dialog shows the OS root folder (apps with `files.read` reach only what is under it; the
// `fs.root` setting in os.json moves it).
import { AppDialog, Field, ToggleGroup, ToggleGroupItem } from "../kit";

export function PhoneSettings({ open, onOpenChange, limit, setInUse, phone = true, filesRoot }: { open: boolean; onOpenChange: (open: boolean) => void; limit: { inUse: number; max: number } | null; setInUse: (n: number) => void; phone?: boolean; filesRoot?: string | null }) {
  return (
    <AppDialog open={open} onOpenChange={onOpenChange} title="Settings">
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
