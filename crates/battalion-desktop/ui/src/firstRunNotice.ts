// SPDX-License-Identifier: MPL-2.0
// BOS-DEFAULT-INF: the apps BattalionOS installs by default on its first open (first-run.json) are named once, in plain words,
// with where to remove them. The host sends `os://first-run` `done` only on the run that installed them, so a later start says nothing.

/** `os://first-run` payload (host.rs `run_first_run`). */
export interface FirstRunEvent {
  status: string;
  ids?: string[];
}

/** The ids to announce: only a run that finished and installed something. */
export const firstRunInstalled = (e: FirstRunEvent): string[] => (e.status === "done" && Array.isArray(e.ids) ? e.ids : []);

/** "Agent Hub is installed with BattalionOS. You can remove it in the Armory." (names come from the catalog, never from code). */
export function firstRunNoticeText(names: string[]): string | null {
  if (names.length === 0) return null;
  const list = names.length === 1 ? names[0] : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
  return names.length === 1
    ? `${list} is installed with BattalionOS. You can remove it in the Armory.`
    : `${list} are installed with BattalionOS. You can remove them in the Armory.`;
}
