// SPDX-License-Identifier: MPL-2.0
// OPEN-WITH-1: the "Open with" chooser. An app asked the OS to open a file and no app was named or remembered for its type: the
// person picks the app that receives it (only installed apps that declare the type are listed), and may remember the choice.
import { useState } from "react";
import { AppDialog, Button, Switch } from "../kit";
import type { OpenWithCandidate } from "./os";

export function OpenWithChooser({ req, done }: { req: { path: string; ext: string; choose: OpenWithCandidate[] }; done: (pick: { appId: string; always: boolean } | null) => void }) {
  const [always, setAlways] = useState(false);
  const name = req.path.split("/").pop() ?? req.path;
  return (
    <AppDialog open onOpenChange={(o) => !o && done(null)} title="Open with" description={`Choose the app that opens ${name}.`}>
      <div className="flex flex-col gap-2" role="list" aria-label="Apps that open this file">
        {req.choose.map((c) => (
          <Button key={c.id} role="listitem" variant="outline" className="justify-between" onClick={() => done({ appId: c.id, always })}>
            <span>{c.name}</span>
            <span className="text-xs text-muted-foreground">{c.label}</span>
          </Button>
        ))}
        <label className="mt-2 flex items-center gap-2 text-sm">
          <Switch checked={always} onCheckedChange={setAlways} aria-label={`Always use this app for .${req.ext} files`} />
          Always use this app for .{req.ext} files
        </label>
      </div>
    </AppDialog>
  );
}
