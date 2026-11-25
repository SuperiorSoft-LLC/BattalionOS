// SPDX-License-Identifier: MPL-2.0
// BOS-34: X hides the OS window (BattalionOS and its apps keep running). The first time, the shell says so in words,
// with Exit beside it; after that X hides at once. Remembered in the shell window's localStorage.

export const CLOSE_NOTICE_KEY = "bos.closeNoticeSeen";
export const CLOSE_NOTICE_TEXT =
  "Closing hides this window. BattalionOS and its apps keep running; open it again from its icon in the notification area or by starting BattalionOS. Exit ends BattalionOS and stops its apps.";

type Store = Pick<Storage, "getItem" | "setItem">;

/** Does X show the notice (first time) rather than hide at once? */
export function closeAsks(store: Store): boolean {
  return store.getItem(CLOSE_NOTICE_KEY) !== "1";
}

/** The notice was seen: later X hides at once. */
export function closeNoticeSeen(store: Store): void {
  store.setItem(CLOSE_NOTICE_KEY, "1");
}
