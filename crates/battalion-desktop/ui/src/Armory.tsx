// SPDX-License-Identifier: MPL-2.0
// The Armory window: Browse and Installed lists, an app page with Install, Update, Roll back and Remove (real progress, the error text
// and Retry), and an offline notice. An install that succeeds puts the icon on the desktop by itself; the page then offers Open.
import { ArrowLeft, Boxes, CloudOff, X } from "lucide-react";
import { useState } from "react";
import { Button, Chip, EmptyState, ListRow, ProgressLine, RefusalBanner, Tabs, TabsContent, TabsList, TabsTrigger } from "../kit";
import { AppIcon } from "./AppIcon";
import type { ArmoryOp } from "./os";
import { confirmText, osRowActions, OS_ROW, type OsPhase, type OsUpdateInfo } from "./osUpdate";
import { appView, availableOf, browseIds, desktopIds, iconOf, labelOf, offline, previousOf, progressOf, type Model } from "./store";

export interface ArmoryActions {
  install(id: string): void;
  update(id: string): void;
  rollback(id: string): void;
  remove(id: string): void;
  /** Clears a failed action's error. */
  dismiss(id: string): void;
  open(id: string): void;
  reload(): void;
  close(): void;
}

const VERB: Record<ArmoryOp, string> = { install: "Install", update: "Update", rollback: "Roll back", remove: "Remove" };

function StateChip({ m, id }: { m: Model; id: string }) {
  const v = appView(m, id);
  const op = m.ops[id];
  if (op?.kind === "working") return <Chip tone="blue">{progressOf(m, id)?.text ?? `${VERB[op.op]}…`}</Chip>;
  if (v.kind === "installed") return <Chip tone={availableOf(m, id) ? "amber" : "green"}>{availableOf(m, id) ? `Update ${availableOf(m, id)}` : v.running ? "Running" : "Installed"}</Chip>;
  if (v.kind === "installing") return <Chip tone="blue">{v.stage}…</Chip>;
  if (v.kind === "failed") return <Chip tone="red">Failed</Chip>;
  if (v.kind === "incompatible") return <Chip tone="neutral">Not compatible</Chip>;
  if (v.kind === "unavailable") return <Chip tone="neutral">Not available</Chip>;
  return null;
}

/** The real progress of whatever is running for this app: a filling bar while bytes are known, a moving one for the other steps. */
function Progress({ m, id, fallback }: { m: Model; id: string; fallback: string }) {
  const p = progressOf(m, id);
  return (
    <div className="flex flex-col gap-1" aria-live="polite">
      <ProgressLine value={p?.percent ?? null} />
      <span className="text-xs text-muted-foreground">{p?.text ?? `${fallback}…`}</span>
    </div>
  );
}

/** A failed update, rollback or remove: the Armory's text, Retry (same action again) and Dismiss. */
function OpError({ m, id, a }: { m: Model; id: string; a: ArmoryActions }) {
  const op = m.ops[id];
  if (op?.kind !== "failed") return null;
  const retry = { install: a.install, update: a.update, rollback: a.rollback, remove: a.remove }[op.op];
  return (
    <div className="flex flex-col gap-1">
      <RefusalBanner text={`${VERB[op.op]} failed: ${op.error}`} onDismiss={() => a.dismiss(id)} />
      <Button size="sm" variant="outline" className="self-start" onClick={() => retry(id)}>
        Retry
      </Button>
    </div>
  );
}

function AppPage({ m, id, a, back }: { m: Model; id: string; a: ArmoryActions; back: () => void }) {
  const v = appView(m, id);
  const op = m.ops[id];
  const entry = m.catalog?.apps.find((c) => c.id === id);
  const newer = availableOf(m, id);
  const prev = previousOf(m, id);
  const busy = op?.kind === "working" || v.kind === "installing";
  const [confirm, setConfirm] = useState(false);
  return (
    <div className="flex flex-col gap-3 p-4">
      <Button variant="ghost" size="sm" className="self-start" onClick={back}>
        <ArrowLeft /> Back
      </Button>
      <div className="flex items-center gap-3">
        <span className="grid size-12 shrink-0 place-items-center rounded-xl border bg-card text-muted-foreground">
          <AppIcon icon={iconOf(m, id)} />
        </span>
        <div className="min-w-0">
          <h2 className="truncate text-lg font-semibold">{labelOf(m, id)}</h2>
          {(entry?.publisher || entry?.summary) && <p className="truncate text-xs text-muted-foreground">{[entry.publisher, entry.summary].filter(Boolean).join(" · ")}</p>}
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        <span className="font-mono">{id}</span>
        {entry && <span>· from {entry.source}</span>}
        {v.kind === "installed" && <span>· version {v.version}</span>}
        {v.kind !== "installed" && entry?.version && <span>· latest {entry.version}</span>}
        <StateChip m={m} id={id} />
      </div>
      {busy && <Progress m={m} id={id} fallback={op?.kind === "working" ? VERB[op.op] : v.kind === "installing" ? v.stage : "Working"} />}
      {v.kind === "failed" && !op && <RefusalBanner text={`Install failed: ${v.error}`} onDismiss={() => a.dismiss(id)} />}
      <OpError m={m} id={id} a={a} />
      {confirm && !busy && (
        <div role="alertdialog" aria-label={`Remove ${labelOf(m, id)}`} className="flex flex-col gap-2 rounded-lg border bg-card p-3 text-sm">
          <span>Remove {labelOf(m, id)}? Its data stays, so installing it again finds it.</span>
          <div className="flex gap-2">
            <Button
              size="sm"
              variant="destructive"
              onClick={() => {
                setConfirm(false);
                a.remove(id);
              }}
            >
              Remove
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setConfirm(false)}>
              Cancel
            </Button>
          </div>
        </div>
      )}
      <div className="flex flex-wrap gap-2">
        {v.kind === "available" && <Button onClick={() => a.install(id)}>Install</Button>}
        {v.kind === "failed" && !op && <Button onClick={() => a.install(id)}>Retry</Button>}
        {v.kind === "installed" && !busy && <Button onClick={() => a.open(id)}>Open</Button>}
        {v.kind === "installed" && !busy && newer && <Button variant="secondary" onClick={() => a.update(id)}>Update to {newer}</Button>}
        {v.kind === "installed" && !busy && prev && (
          <Button variant="outline" onClick={() => a.rollback(id)}>
            Roll back to {prev}
          </Button>
        )}
        {(v.kind === "installed" || v.kind === "failed") && !busy && (
          <Button variant="outline" onClick={() => setConfirm(true)}>
            Remove
          </Button>
        )}
        {v.kind === "unavailable" && <span className="text-sm text-muted-foreground">{v.reason}</span>}
        {v.kind === "incompatible" && <span className="text-sm text-muted-foreground">This app needs a different BattalionOS version.</span>}
      </div>
    </div>
  );
}

/** What the BattalionOS row needs: what the OS knows, how the last action went, and the two things it can do. */
export interface OsRowProps {
  info: OsUpdateInfo | null;
  phase: OsPhase;
  install(): void;
  dismiss(): void;
}

/** BOS-30: BattalionOS itself in Installed. Not an app: its version, "Update to X" (confirmed in words), progress, a plain error with
 *  Retry. No Open, no Remove. */
function OsRow({ m, os }: { m: Model; os: OsRowProps }) {
  const [confirm, setConfirm] = useState(false);
  if (!os.info) return null;
  const info = os.info;
  const working = os.phase.kind === "working" || info.installing;
  const actions = osRowActions(info, os.phase);
  return (
    <div className="flex flex-col gap-1" data-installed={OS_ROW}>
      <ListRow
        row={{ id: OS_ROW, title: "BattalionOS", subtitle: `version ${info.current}` }}
        icon={<Boxes className="size-5" />}
        trailing={
          <span className="flex items-center gap-1">
            {info.available && !working && <Chip tone="amber">Update {info.available.version}</Chip>}
            {working && <Chip tone="blue">{progressOf(m, OS_ROW)?.text ?? "Updating…"}</Chip>}
            {!info.available && !working && <Chip tone="green">Up to date</Chip>}
            {actions.map((label) => (
              <Button key={label} size="xs" onClick={() => setConfirm(true)} aria-label={label}>
                {label}
              </Button>
            ))}
          </span>
        }
      />
      {confirm && !working && (
        <div role="alertdialog" aria-label="Update BattalionOS" className="flex flex-col gap-2 rounded-lg border bg-card p-3">
          <p className="text-sm">{confirmText(info)}</p>
          <div className="flex gap-2">
            <Button
              size="sm"
              onClick={() => {
                setConfirm(false);
                os.install();
              }}
            >
              Update and restart
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setConfirm(false)}>
              Cancel
            </Button>
          </div>
        </div>
      )}
      {working && <Progress m={m} id={OS_ROW} fallback="Updating" />}
      {os.phase.kind === "failed" && (
        <div className="flex flex-col gap-1">
          <RefusalBanner text={`Update failed: ${os.phase.error}`} onDismiss={os.dismiss} />
          <Button size="sm" variant="outline" className="self-start" onClick={() => os.install()}>
            Retry
          </Button>
        </div>
      )}
    </div>
  );
}

export function Armory({ m, a, osRow }: { m: Model; a: ArmoryActions; osRow?: OsRowProps }) {
  const [page, setPage] = useState<string | null>(null);
  const off = offline(m);
  const browse = browseIds(m);
  const installed = desktopIds(m);
  const subtitle = (id: string) => {
    const inst = m.installed.find((x) => x.id === id);
    return inst ? `${id} · ${inst.version}` : id;
  };
  const icon = (id: string) => <AppIcon icon={iconOf(m, id)} className="size-5" />;
  const browseRow = (id: string) => (
    <button key={id} type="button" className="block w-full text-left" onClick={() => setPage(id)} aria-label={`Open ${labelOf(m, id)} page`}>
      <ListRow row={{ id, title: labelOf(m, id), subtitle: subtitle(id) }} icon={icon(id)} trailing={<StateChip m={m} id={id} />} />
    </button>
  );
  // Installed rows carry their own buttons, so the row itself is not a button.
  const installedRow = (id: string) => {
    const op = m.ops[id];
    const busy = op?.kind === "working" || appView(m, id).kind === "installing";
    const newer = availableOf(m, id);
    return (
      <div key={id} className="flex flex-col gap-1" data-installed={id}>
        <ListRow
          row={{ id, title: labelOf(m, id), subtitle: subtitle(id) }}
          icon={icon(id)}
          trailing={
            <span className="flex items-center gap-1">
              <StateChip m={m} id={id} />
              {!busy && newer && (
                <Button size="xs" onClick={() => a.update(id)} aria-label={`Update ${labelOf(m, id)}`}>
                  Update
                </Button>
              )}
              <Button size="xs" variant="outline" onClick={() => setPage(id)} aria-label={`Open ${labelOf(m, id)} page`}>
                Details
              </Button>
              {!busy && (
                <Button size="xs" variant="ghost" onClick={() => setPage(id)} aria-label={`Remove ${labelOf(m, id)}`}>
                  Remove
                </Button>
              )}
            </span>
          }
        />
        {busy && <Progress m={m} id={id} fallback={op?.kind === "working" ? VERB[op.op] : "Working"} />}
        <OpError m={m} id={id} a={a} />
      </div>
    );
  };

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
              {browse.map(browseRow)}
            </TabsContent>
            <TabsContent value="installed" className="mt-3 flex flex-col gap-2">
              {osRow && <OsRow m={m} os={osRow} />}
              {installed.length === 0 && <EmptyState icon={<Boxes />} title="Nothing installed yet" hint="Install an app from Browse and it appears on the desktop." />}
              {installed.map(installedRow)}
            </TabsContent>
          </Tabs>
        )}
      </div>
    </section>
  );
}
