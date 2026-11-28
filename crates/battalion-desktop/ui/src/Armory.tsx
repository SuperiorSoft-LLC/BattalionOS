// SPDX-License-Identifier: MPL-2.0
// The Armory window: Browse and Installed lists, an app page with Install, Update, Roll back and Remove (real progress, the error text
// and Retry), and an offline notice. An install that succeeds puts the icon on the desktop by itself; the page then offers Open.
import { ArrowLeft, Boxes, CloudOff, X } from "lucide-react";
import { useEffect, useState } from "react";
import { Button, Chip, EmptyState, Input, ListRow, ProgressLine, RefusalBanner, Select, SelectContent, SelectItem, SelectTrigger, SelectValue, Tabs, TabsContent, TabsList, TabsTrigger } from "../kit";
import { AppIcon } from "./AppIcon";
import type { ArmoryOp, FolderRequest, FolderResult, LocalRepo } from "./os";
import { confirmText, osRowActions, OS_ROW, type OsPhase, type OsUpdateInfo } from "./osUpdate";
import { appView, availableOf, browseIds, channelNoteOf, channelsOf, channelText, desktopIds, iconOf, labelOf, offline, pendingOf, previousOf, progressOf, waitText, type Model } from "./store";

export interface ArmoryActions {
  install(id: string): void;
  update(id: string): void;
  /** UPDATE-LIVE-1 "Update now": the app checkpoints at once and is updated. */
  updateNow(id: string): void;
  /** UPDATE-LIVE-1 "Later": the update applies by itself at the app's next start or idle moment. */
  later(id: string): void;
  rollback(id: string): void;
  /** ARMORY-CHANNEL-SWITCH: move the app to another channel (installs nothing). */
  setChannel(id: string, channel: string): void;
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
  if (v.kind === "installed" && pendingOf(m, id)) return <Chip tone="amber">Updates to {pendingOf(m, id)} when idle</Chip>;
  if (v.kind === "installed") return <Chip tone={availableOf(m, id) ? "amber" : "green"}>{availableOf(m, id) ? `Update ${availableOf(m, id)}` : v.running && !(m.ended ?? []).includes(id) ? "Running" : "Installed"}</Chip>;
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
  if (op?.kind === "not-ready") {
    // UPDATE-LIVE-1: not a failure. The new version is downloaded; the app is still working and keeps working.
    return (
      <div className="flex flex-col gap-1" role="status">
        <span className="text-sm">{waitText(labelOf(m, id), op.reason, op.etaMs)} Version {op.version} is downloaded and ready.</span>
        <div className="flex gap-2">
          <Button size="sm" onClick={() => a.updateNow(id)}>
            Update now
          </Button>
          <Button size="sm" variant="outline" onClick={() => a.later(id)}>
            Later
          </Button>
        </div>
      </div>
    );
  }
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

/** ARMORY-LOCAL-2: what "Install from folder…" can do (the host's os_install_from_folder). */
export interface FolderActions {
  install(req: FolderRequest): Promise<FolderResult>;
  /** An app was installed or replaced: the lists are read again. */
  changed(): void;
}

/** ARMORY-LOCAL-2: run one folder install, asking for each confirm the host needs, then report. */
function useFolderInstall(f: FolderActions | undefined) {
  const [ask, setAsk] = useState<{ req: FolderRequest; message: string; kind: "unsigned" | "replacesCatalog" } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const run = (req: FolderRequest) => {
    if (!f) return;
    setBusy(true);
    setError(null);
    setDone(null);
    f.install(req).then(
      (r) => {
        setBusy(false);
        if ("confirm" in r) setAsk({ req: { ...req, path: r.path }, message: r.message, kind: r.confirm });
        else if ("ok" in r) {
          setAsk(null);
          setDone(r.from ? `${r.id} ${r.version} installed from the folder (was ${r.from}).` : `${r.id} ${r.version} installed from the folder.`);
          f.changed();
        }
      },
      (e) => {
        setBusy(false);
        setError(String(e));
      },
    );
  };
  const agree = () => ask && run(ask.kind === "unsigned" ? { ...ask.req, allowUnsigned: true } : { ...ask.req, replaceCatalog: true });
  const view = (
    <>
      {busy && <ProgressLine value={null} />}
      {error && <RefusalBanner text={`Not installed: ${error}`} onDismiss={() => setError(null)} />}
      {done && (
        <div role="status" className="rounded-lg border bg-card p-2 text-xs text-muted-foreground">
          {done}
        </div>
      )}
      {ask && (
        <div role="alertdialog" aria-label={ask.kind === "unsigned" ? "Unsigned local app" : "Replace the catalog app"} className="flex flex-col gap-2 rounded-lg border bg-card p-3 text-sm">
          <span className="font-medium">{ask.kind === "unsigned" ? "Unsigned local app" : "Replace the catalog app?"}</span>
          <span>{ask.message}</span>
          <span className="truncate font-mono text-xs text-muted-foreground">{ask.req.path}</span>
          <div className="flex gap-2">
            <Button size="sm" variant={ask.kind === "unsigned" ? "destructive" : "default"} onClick={agree}>
              {ask.kind === "unsigned" ? "Install anyway" : "Replace it"}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setAsk(null)}>
              Cancel
            </Button>
          </div>
        </div>
      )}
    </>
  );
  return { run, view, busy };
}

/** ARMORY-LOCAL-2: the "Install from folder…" bar (a folder, or a .bosapp package file). */
function InstallFromFolder({ f }: { f: FolderActions }) {
  const { run, view, busy } = useFolderInstall(f);
  return (
    <div className="flex flex-col gap-2" data-install-from-folder>
      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" variant="outline" disabled={busy} onClick={() => run({})}>
          Install from folder…
        </Button>
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => run({ file: true })}>
          Package file…
        </Button>
        <span className="text-xs text-muted-foreground">An app you built: its folder, its dist folder, or the .bosapp.</span>
      </div>
      {view}
    </div>
  );
}

function AppPage({ m, id, a, back, folder }: { m: Model; id: string; a: ArmoryActions; back: () => void; folder?: FolderActions }) {
  const local = m.installed.find((x) => x.id === id)?.local;
  const reinstall = useFolderInstall(folder);
  const v = appView(m, id);
  const op = m.ops[id];
  const entry = m.catalog?.apps.find((c) => c.id === id);
  const newer = availableOf(m, id);
  const prev = previousOf(m, id);
  const channels = channelsOf(m, id);
  const channel = m.installed.find((x) => x.id === id)?.channel || "stable";
  const note = channelText(channelNoteOf(m, id));
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
        {entry && !entry.fromLocalRepo && <span>· from {entry.source}</span>}
        {entry?.fromLocalRepo && <Chip tone="blue">{entry.label ?? "Local, built on this machine"}</Chip>}
        {entry?.fromLocalRepo && <span>· {entry.source.replace(/^local:/, "")}</span>}
        {local && <Chip tone="blue">Local</Chip>}
        {local && <span className="truncate">· from {local.path}{local.signing === "trusted" ? "" : " (unsigned)"}</span>}
        {v.kind === "installed" && <span>· version {v.version}</span>}
        {v.kind !== "installed" && entry?.version && <span>· latest {entry.version}</span>}
        <StateChip m={m} id={id} />
      </div>
      {v.kind === "installed" && channels.length > 0 && <ChannelPicker id={id} channels={channels} channel={channel} note={note} disabled={busy} onPick={(c) => a.setChannel(id, c)} />}
      {busy && <Progress m={m} id={id} fallback={op?.kind === "working" ? VERB[op.op] : v.kind === "installing" ? v.stage : "Working"} />}
      {v.kind === "failed" && !op && <RefusalBanner text={`Install failed: ${v.error}`} onDismiss={() => a.dismiss(id)} />}
      <OpError m={m} id={id} a={a} />
      {reinstall.view}
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
        {v.kind === "installed" && !busy && newer && !note && <Button variant="secondary" onClick={() => a.update(id)}>Update to {newer}</Button>}
        {v.kind === "installed" && !busy && prev && (
          <Button variant="outline" onClick={() => a.rollback(id)}>
            Roll back to {prev}
          </Button>
        )}
        {v.kind === "installed" && !busy && local && folder && (
          <Button variant="outline" disabled={reinstall.busy} onClick={() => reinstall.run({ path: local.path })}>
            Reinstall from folder
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

/** ARMORY-CHANNEL-SWITCH: the channel an installed app follows. Switching installs nothing; the note says when the channel has
 *  no build yet or is behind the installed version (then no update is offered). */
export function ChannelPicker({ id, channels, channel, note, disabled, onPick }: { id: string; channels: string[]; channel: string; note: string | null; disabled?: boolean; onPick: (c: string) => void }) {
  return (
    <div className="flex flex-wrap items-center gap-2 text-sm">
      <span className="text-muted-foreground">Channel</span>
      <Select value={channel} disabled={disabled} onValueChange={(c) => c !== channel && onPick(c)}>
        <SelectTrigger size="sm" aria-label={`Channel of ${id}`} className="w-36">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {channels.map((c) => (
            <SelectItem key={c} value={c}>
              {c}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {note && <span className="text-xs text-muted-foreground">{note}</span>}
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
 *  Dismiss while the one button reads "Try again". No Open, no Remove. ARMORY-OSROW-1: the button alone says an update is there (no
 *  second "Update X" chip). */
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
        <RefusalBanner text={`Update failed: ${os.phase.error}`} onDismiss={os.dismiss} />
      )}
    </div>
  );
}

export function Armory({ m, a, osRow, repos, folder }: { m: Model; a: ArmoryActions; osRow?: OsRowProps; repos?: LocalRepoActions; folder?: FolderActions }) {
  const [page, setPage] = useState<string | null>(null);
  const off = offline(m);
  const browse = browseIds(m);
  const installed = desktopIds(m);
  // ARMORY-OSROW-1: the count is the rows the tab shows, so the BattalionOS row counts when it is there.
  const installedCount = installed.length + (osRow?.info ? 1 : 0);
  const subtitle = (id: string) => {
    const inst = m.installed.find((x) => x.id === id);
    return inst ? `${id} · ${inst.version}` : id;
  };
  const icon = (id: string) => <AppIcon icon={iconOf(m, id)} className="size-5" />;
  const browseRow = (id: string) => (
    <button key={id} type="button" className="block w-full text-left" onClick={() => setPage(id)} aria-label={`Open ${labelOf(m, id)} page`}>
      <ListRow
        row={{ id, title: labelOf(m, id), subtitle: subtitle(id) }}
        icon={icon(id)}
        trailing={
          <span className="flex items-center gap-1">
            {m.catalog?.apps.find((c) => c.id === id)?.fromLocalRepo && <Chip tone="neutral">Local</Chip>}
            <StateChip m={m} id={id} />
          </span>
        }
      />
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
              {m.installed.find((x) => x.id === id)?.local && <Chip tone="neutral">Local</Chip>}
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
          <AppPage m={m} id={page} a={a} back={() => setPage(null)} folder={folder} />
        ) : (
          <Tabs defaultValue="browse" className="p-4">
            <TabsList>
              <TabsTrigger value="browse">Browse</TabsTrigger>
              <TabsTrigger value="installed">Installed ({installedCount})</TabsTrigger>
              {repos && <TabsTrigger value="local">Local repos</TabsTrigger>}
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
              {folder && <InstallFromFolder f={folder} />}
              {osRow && <OsRow m={m} os={osRow} />}
              {installedCount === 0 && <EmptyState icon={<Boxes />} title="Nothing installed yet" hint="Install an app from Browse and it appears on the desktop." />}
              {installed.map(installedRow)}
            </TabsContent>
            {repos && (
              <TabsContent value="local" className="mt-3 flex flex-col gap-2">
                <LocalRepos r={repos} />
              </TabsContent>
            )}
          </Tabs>
        )}
      </div>
    </section>
  );
}

/** LOCAL-REPO-1: what the Local repos tab can do (the host's os_local_repo_* commands). */
export interface LocalRepoActions {
  list(): Promise<LocalRepo[]>;
  add(name: string): Promise<void>;
  rename(name: string, newName: string): Promise<void>;
  remove(name: string): Promise<void>;
  /** ARMORY-LOCAL-2: list a folder kept elsewhere (a checkout with catalog.json); no path = the folder picker. */
  link?(name: string, path?: string): Promise<void>;
  /** A repo changed: the catalog is read again. */
  changed(): void;
}

/** LOCAL-REPO-1: the private local repos agents publish to (an app publish command with `--repo <name>`): add, rename, remove. */
export function LocalRepos({ r }: { r: LocalRepoActions }) {
  const [repos, setRepos] = useState<LocalRepo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [editing, setEditing] = useState<{ name: string; to: string } | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);
  const load = () => r.list().then(setRepos, (e) => setError(String(e)));
  useEffect(() => void load(), []);
  const run = (p: Promise<void>) =>
    p.then(
      () => {
        setError(null);
        r.changed();
        return load();
      },
      (e) => setError(String(e)),
    );
  return (
    <div className="flex flex-col gap-2">
      <p className="text-xs text-muted-foreground">
        Apps an agent builds and publishes on this machine (its app publish command, with <span className="font-mono">--repo &lt;name&gt;</span>). They are signed with this device&apos;s own key and install without the internet.
      </p>
      {error && <RefusalBanner text={error} onDismiss={() => setError(null)} />}
      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (name.trim()) void run(r.add(name.trim())).then(() => setName(""));
        }}
      >
        <Input aria-label="New local repo name" placeholder="New repo name, e.g. Experiments" value={name} onChange={(e) => setName(e.target.value)} />
        <Button type="submit" size="sm" disabled={!name.trim()}>
          Add repo
        </Button>
        {r.link && (
          <Button type="button" size="sm" variant="outline" disabled={!name.trim()} onClick={() => void run(r.link!(name.trim())).then(() => setName(""))} title="Name it, then pick a folder holding catalog.json (a local checkout of an apps repo)">
            Add folder…
          </Button>
        )}
      </form>
      {repos && repos.length === 0 && <EmptyState icon={<Boxes />} title="No local repos yet" hint="The first publish makes My apps." />}
      {repos?.map((repo) => (
        <div key={repo.path} className="flex flex-col gap-1" data-local-repo={repo.name}>
          {editing?.name === repo.name ? (
            <form
              className="flex gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                if (editing.to.trim()) void run(r.rename(repo.name, editing.to.trim())).then(() => setEditing(null));
              }}
            >
              <Input aria-label={`New name for ${repo.name}`} value={editing.to} onChange={(e) => setEditing({ name: repo.name, to: e.target.value })} autoFocus />
              <Button type="submit" size="sm">
                Save
              </Button>
              <Button type="button" size="sm" variant="ghost" onClick={() => setEditing(null)}>
                Cancel
              </Button>
            </form>
          ) : (
            <ListRow
              row={{ id: repo.path, title: repo.name, subtitle: repo.path }}
              icon={<Boxes className="size-5" />}
              trailing={
                repo.registered ? (
                  <Chip tone="neutral">Set in os.json</Chip>
                ) : (
                  <span className="flex items-center gap-1">
                    {repo.linked && <Chip tone="neutral">Folder</Chip>}
                    <Button size="xs" variant="outline" onClick={() => setEditing({ name: repo.name, to: repo.name })} aria-label={`Rename ${repo.name}`}>
                      Rename
                    </Button>
                    <Button size="xs" variant="ghost" onClick={() => setConfirm(repo.name)} aria-label={`Remove ${repo.name}`}>
                      Remove
                    </Button>
                  </span>
                )
              }
            />
          )}
          {confirm === repo.name && (
            <div role="alertdialog" aria-label={`Remove ${repo.name}`} className="flex flex-col gap-2 rounded-lg border bg-card p-3 text-sm">
              <span>{repo.linked ? `Stop listing ${repo.name}? The folder and its files stay; apps installed from it stay installed.` : `Remove ${repo.name} and its packages? Apps installed from it stay installed.`}</span>
              <div className="flex gap-2">
                <Button
                  size="sm"
                  variant="destructive"
                  onClick={() => {
                    setConfirm(null);
                    void run(r.remove(repo.name));
                  }}
                >
                  Remove
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setConfirm(null)}>
                  Cancel
                </Button>
              </div>
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
