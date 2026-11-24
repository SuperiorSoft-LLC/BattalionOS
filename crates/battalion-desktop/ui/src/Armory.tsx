// SPDX-License-Identifier: MPL-2.0
// The Armory window: Browse and Installed lists, an app page with Install (progress, error, Retry), and an offline notice.
// An install that succeeds puts the icon on the desktop by itself; the page then offers Open.
import { ArrowLeft, Boxes, CloudOff, X } from "lucide-react";
import { useState } from "react";
import { Button, Chip, EmptyState, ListRow, ProgressLine, RefusalBanner, Tabs, TabsContent, TabsList, TabsTrigger } from "../kit";
import { appLabel, appView, browseIds, desktopIds, offline, type Model } from "./store";

export interface ArmoryActions { install(id: string): void; open(id: string): void; reload(): void; close(): void }

function StateChip({ m, id }: { m: Model; id: string }) {
  const v = appView(m, id);
  if (v.kind === "installed") return <Chip tone="green">{v.running ? "Running" : "Installed"}</Chip>;
  if (v.kind === "installing") return <Chip tone="blue">{v.stage}…</Chip>;
  if (v.kind === "failed") return <Chip tone="red">Failed</Chip>;
  if (v.kind === "incompatible") return <Chip tone="neutral">Not compatible</Chip>;
  return null;
}

function AppPage({ m, id, a, back }: { m: Model; id: string; a: ArmoryActions; back: () => void }) {
  const v = appView(m, id);
  const entry = m.catalog?.apps.find((c) => c.id === id);
  return (
    <div className="flex flex-col gap-3 p-4">
      <Button variant="ghost" size="sm" className="self-start" onClick={back}>
        <ArrowLeft /> Back
      </Button>
      <h2 className="text-lg font-semibold">{appLabel(id)}</h2>
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <span className="font-mono">{id}</span>
        {entry && <span>· from {entry.source}</span>}
        {v.kind === "installed" && <span>· version {v.version}</span>}
        <StateChip m={m} id={id} />
      </div>
      {v.kind === "installing" && (
        <div className="flex flex-col gap-1" aria-live="polite">
          <ProgressLine value={null} />
          <span className="text-xs text-muted-foreground">{v.stage}…</span>
        </div>
      )}
      {v.kind === "failed" && <RefusalBanner text={`Install failed: ${v.error}`} onDismiss={() => undefined} />}
      <div className="flex gap-2">
        {v.kind === "available" && <Button onClick={() => a.install(id)}>Install</Button>}
        {v.kind === "failed" && <Button onClick={() => a.install(id)}>Retry</Button>}
        {v.kind === "installed" && <Button onClick={() => a.open(id)}>Open</Button>}
        {v.kind === "incompatible" && <span className="text-sm text-muted-foreground">This app needs a different BattalionOS version.</span>}
      </div>
    </div>
  );
}

export function Armory({ m, a }: { m: Model; a: ArmoryActions }) {
  const [page, setPage] = useState<string | null>(null);
  const off = offline(m);
  const browse = browseIds(m);
  const installed = desktopIds(m);
  const row = (id: string) => (
    <button key={id} type="button" className="block w-full text-left" onClick={() => setPage(id)} aria-label={`Open ${appLabel(id)} page`}>
      <ListRow row={{ id, title: appLabel(id), subtitle: id }} icon={<Boxes className="size-4" />} trailing={<StateChip m={m} id={id} />} />
    </button>
  );

  return (
    <section role="dialog" aria-label="Armory" className="absolute inset-4 z-10 mx-auto flex max-w-3xl flex-col overflow-hidden rounded-xl border bg-background shadow-2xl">
      <header className="flex items-center justify-between border-b px-4 py-2">
        <span className="flex items-center gap-2 font-medium">
          <Boxes className="size-4" /> Armory
        </span>
        <Button variant="ghost" size="icon-sm" aria-label="Close Armory" onClick={a.close}>
          <X />
        </Button>
      </header>
      {off.offline && (
        <div role="status" className="flex items-center justify-between gap-2 border-b bg-warn/10 px-4 py-2 text-xs text-warn">
          <span className="flex items-center gap-2">
            <CloudOff className="size-4" /> Offline{m.catalog ? ": showing the last saved list" : ""}. {off.reason}
          </span>
          <Button size="xs" variant="outline" onClick={a.reload}>
            Retry
          </Button>
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-auto">
        {page ? (
          <AppPage m={m} id={page} a={a} back={() => setPage(null)} />
        ) : (
          <Tabs defaultValue="browse" className="p-4">
            <TabsList>
              <TabsTrigger value="browse">Browse</TabsTrigger>
              <TabsTrigger value="installed">Installed ({installed.length})</TabsTrigger>
            </TabsList>
            <TabsContent value="browse" className="mt-3 flex flex-col gap-2">
              {m.catalogLoading && !m.catalog && <ProgressLine value={null} />}
              {!m.catalog && m.catalogError && (
                <EmptyState icon={<CloudOff />} title="The app list could not be loaded" hint={m.catalogError} action={<Button size="sm" onClick={a.reload}>Retry</Button>} />
              )}
              {m.catalog && browse.length === 0 && <EmptyState icon={<Boxes />} title="No apps in the catalog yet" />}
              {browse.map(row)}
            </TabsContent>
            <TabsContent value="installed" className="mt-3 flex flex-col gap-2">
              {installed.length === 0 && <EmptyState icon={<Boxes />} title="Nothing installed yet" hint="Install an app from Browse and it appears on the desktop." />}
              {installed.map(row)}
            </TabsContent>
          </Tabs>
        )}
      </div>
    </section>
  );
}
