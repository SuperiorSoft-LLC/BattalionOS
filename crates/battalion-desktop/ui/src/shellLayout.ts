// SPDX-License-Identifier: MPL-2.0
// BOS-LAYOUT-1: the shell is one column (title bar, app area, taskbar) exactly the window's height, like the Windows desktop: the
// taskbar keeps its own height and the app area fills the work area above it. A maximized undecorated window on Windows reaches past
// the monitor's work area by its resize frame; the column is padded by that overflow so the taskbar stays fully on screen.
import type { CSSProperties } from "react";
import type { Inset } from "./os";

/** The root column: the window's height, never taller, never a page scroll. */
export const SHELL_CLASS = "flex h-full max-h-full min-h-0 flex-col overflow-hidden bg-background text-foreground";

export const NO_INSET: Inset = { top: 0, right: 0, bottom: 0, left: 0 };

/** A host answer made safe: negative, missing or non-number sides are 0. */
export function cleanInset(i: Partial<Inset> | null | undefined): Inset {
  const n = (v: unknown) => (typeof v === "number" && Number.isFinite(v) && v > 0 ? v : 0);
  return { top: n(i?.top), right: n(i?.right), bottom: n(i?.bottom), left: n(i?.left) };
}

/** The root's style: a phone keeps clear of its system bars; a desktop window keeps clear of the part beyond the work area. */
export function shellStyle(phone: boolean, inset: Inset, phoneInsets: CSSProperties): CSSProperties | undefined {
  if (phone) return phoneInsets;
  const i = cleanInset(inset);
  if (!i.top && !i.right && !i.bottom && !i.left) return undefined;
  return { paddingTop: i.top, paddingRight: i.right, paddingBottom: i.bottom, paddingLeft: i.left, boxSizing: "border-box" };
}
