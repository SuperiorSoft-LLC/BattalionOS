// SPDX-License-Identifier: MPL-2.0
// The first-run card: shown on an empty desktop. One click on an app installs it; the icon then appears on the desktop.
import { Boxes } from "lucide-react";
import { Button, Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "../kit";
import { labelOf, offline, type Model } from "./store";

export function FirstRun({ m, install, openArmory, dismiss }: { m: Model; install: (id: string) => void; openArmory: () => void; dismiss: () => void }) {
  const suggestions = (m.catalog?.apps ?? []).filter((a) => a.compatible && !a.unavailable).slice(0, 3);
  const off = offline(m);
  return (
    <Card className="absolute left-1/2 top-1/2 w-96 -translate-x-1/2 -translate-y-1/2" role="region" aria-label="Welcome">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Boxes className="size-4" /> Welcome to BattalionOS
        </CardTitle>
        <CardDescription>Your desktop is empty. Install an app and its icon appears here.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        {m.catalogLoading && !m.catalog && <span className="text-xs text-muted-foreground">Loading apps…</span>}
        {off.offline && <span className="text-xs text-warn">Offline. {off.reason}</span>}
        {suggestions.map((a) => (
          <Button key={a.id} variant="outline" className="justify-between" onClick={() => install(a.id)}>
            {labelOf(m, a.id)} <span className="text-xs text-muted-foreground">Install</span>
          </Button>
        ))}
      </CardContent>
      <CardFooter className="gap-2">
        <Button size="sm" onClick={openArmory}>
          Open Armory
        </Button>
        <Button size="sm" variant="ghost" onClick={dismiss}>
          Not now
        </Button>
      </CardFooter>
    </Card>
  );
}
