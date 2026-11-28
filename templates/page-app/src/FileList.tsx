// SPDX-License-Identifier: Apache-2.0
// One folder of the OS root: breadcrumbs, its entries, and a "new file" button. It only shows; App owns the state.
import type { FsEntry } from "@battalionos/app-api";
import { crumbs, join } from "./path";

export type Listing = { state: "loading" } | { state: "error"; message: string } | { state: "ready"; entries: FsEntry[] };

interface Props {
  dir: string;
  listing: Listing;
  selected: string | null;
  onOpenDir(dir: string): void;
  onOpenFile(path: string): void;
  onNewFile(): void;
  onRetry(): void;
}

export function FileList({ dir, listing, selected, onOpenDir, onOpenFile, onNewFile, onRetry }: Props) {
  const parts = crumbs(dir);
  return (
    <nav className="files" aria-label="Files">
      <div className="crumbs">
        <button type="button" className="link" onClick={() => onOpenDir("")}>Root</button>
        {parts.map((p, i) => (
          <span key={i}>
            {" / "}
            <button type="button" className="link" onClick={() => onOpenDir(parts.slice(0, i + 1).join("/"))}>{p}</button>
          </span>
        ))}
      </div>
      <button type="button" onClick={onNewFile}>New file</button>
      {listing.state === "loading" && <p role="status">Loading…</p>}
      {listing.state === "error" && (
        <div role="alert" className="error">
          <p>{listing.message}</p>
          <button type="button" onClick={onRetry}>Try again</button>
        </div>
      )}
      {listing.state === "ready" && listing.entries.length === 0 && <p className="muted">This folder is empty.</p>}
      {listing.state === "ready" && listing.entries.length > 0 && (
        <ul>
          {listing.entries.map((e) => {
            const path = join(dir, e.name);
            return (
              <li key={e.name}>
                <button
                  type="button"
                  className={path === selected ? "entry selected" : "entry"}
                  aria-current={path === selected ? "true" : undefined}
                  onClick={() => (e.kind === "dir" ? onOpenDir(path) : onOpenFile(path))}
                >
                  <span aria-hidden="true">{e.kind === "dir" ? "📁" : "📄"}</span> {e.name}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </nav>
  );
}
