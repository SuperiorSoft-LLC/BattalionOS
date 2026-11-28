// SPDX-License-Identifier: Apache-2.0
// The demo: list a folder of the OS root, open a text file, edit and save it. The last folder is kept in the app's own
// store (bos.store), so the app opens where it was left. Every OS call goes through `bos` (src/bos.ts).
import { NotFoundError, PermissionError, type BosError } from "@battalionos/app-api";
import { useCallback, useEffect, useState } from "react";
import { bos } from "./bos";
import { Editor, type Doc } from "./Editor";
import { FileList, type Listing } from "./FileList";
import { join } from "./path";

const LAST_DIR = "lastDir";

function explain(e: unknown): string {
  if (e instanceof PermissionError) return `This app needs the '${e.permission}' permission in app.json.`;
  if (e instanceof NotFoundError) return "It is not there any more.";
  return (e as BosError)?.message ?? String(e);
}

export function App() {
  const [dir, setDir] = useState<string | null>(null);
  const [listing, setListing] = useState<Listing>({ state: "loading" });
  const [doc, setDoc] = useState<Doc>({ state: "none" });
  const [status, setStatus] = useState("");

  // Start in the folder the store remembers (the root when none).
  useEffect(() => {
    bos.store.get<string>(LAST_DIR).then((d) => setDir(d ?? ""), () => setDir(""));
  }, []);

  const load = useCallback(async (d: string) => {
    setListing({ state: "loading" });
    try {
      const entries = await bos.fs.listAll(d);
      entries.sort((a, b) => Number(b.kind === "dir") - Number(a.kind === "dir") || a.name.localeCompare(b.name));
      setListing({ state: "ready", entries });
    } catch (e) {
      setListing({ state: "error", message: explain(e) });
    }
  }, []);

  useEffect(() => {
    if (dir === null) return;
    void load(dir);
    void bos.store.set(LAST_DIR, dir).catch(() => undefined);
    // Live: the list follows changes made by anyone (another app, the person's own tools).
    let stop: (() => Promise<void>) | undefined;
    let gone = false;
    bos.fs.watch(dir, () => void load(dir)).then(
      (s) => (gone ? void s() : (stop = s)),
      () => undefined,
    );
    return () => {
      gone = true;
      void stop?.();
    };
  }, [dir, load]);

  const openFile = async (path: string) => {
    setDoc({ state: "loading", path });
    try {
      setDoc({ state: "ready", path, text: await bos.fs.readText(path), dirty: false, saving: false });
    } catch (e) {
      setDoc({ state: "error", path, message: explain(e) });
    }
  };

  const save = async () => {
    if (doc.state !== "ready") return;
    setDoc({ ...doc, saving: true });
    try {
      await bos.fs.writeText(doc.path, doc.text);
      setDoc({ ...doc, dirty: false, saving: false });
      setStatus(`Saved ${doc.path}`);
    } catch (e) {
      setDoc({ ...doc, saving: false });
      setStatus(`Not saved: ${explain(e)}`);
    }
  };

  const newFile = async () => {
    if (dir === null) return;
    const name = `untitled-${new Date().toISOString().replace(/[:.]/g, "-")}.txt`;
    try {
      await bos.fs.writeText(join(dir, name), "", { overwrite: false });
      await load(dir);
      await openFile(join(dir, name));
    } catch (e) {
      setStatus(`Could not create a file: ${explain(e)}`);
    }
  };

  return (
    <main className="app">
      <header>
        <h1>My App</h1>
        <span className="muted">{bos.backend === "local" ? "local backend (no OS)" : "BattalionOS"}</span>
      </header>
      <div className="split">
        <FileList
          dir={dir ?? ""}
          listing={dir === null ? { state: "loading" } : listing}
          selected={doc.state === "none" ? null : doc.path}
          onOpenDir={setDir}
          onOpenFile={openFile}
          onNewFile={newFile}
          onRetry={() => dir !== null && void load(dir)}
        />
        <Editor doc={doc} onChange={(text) => doc.state === "ready" && setDoc({ ...doc, text, dirty: true })} onSave={save} />
      </div>
      <p className="status" role="status" aria-live="polite">{status}</p>
    </main>
  );
}
