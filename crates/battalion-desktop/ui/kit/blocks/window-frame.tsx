// SPDX-License-Identifier: Apache-2.0
// Copyright (c) SuperiorSoft LLC
// OS-WINDOW-1 §3 (WM-4): the one window chrome. The OS shell draws `float` and `native` windows with it, and it ships as
// `@battalion/app-sdk/chrome` for `inline` windows and the local backend. Its look comes only from the `--bos-win-*`
// tokens (lib/win-tokens.ts), with the OS defaults as fallbacks. There is NO className or style prop on the chrome:
// apps style only their content (children). No shell imports, no Tailwind classes: it renders the same in any build.
import { Minus, Square, X, Copy } from "lucide-react";
import { useState, type CSSProperties, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import { WIN_TOKENS_DARK, type WinToken } from "../lib/win-tokens";

export type WindowEdge = "North" | "South" | "East" | "West" | "NorthEast" | "NorthWest" | "SouthEast" | "SouthWest";
/** A rectangle in the frame's own coordinates (px). */
export interface FrameRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface WindowFrameProps {
  title: string;
  icon?: ReactNode;
  badge?: ReactNode;
  children?: ReactNode;
  /** Window state (drawn by the chrome, not styling). */
  active?: boolean;
  maximized?: boolean;
  /** Where the window would snap to, while a drag is over a snap zone; null hides the preview. */
  snapPreview?: FrameRect | null;
  /** WM-8: extra title-bar buttons before Minimize (the OS shell's "Pop out to its own window"). */
  actions?: ReactNode;
  onMinimize?: () => void;
  onMaximize?: () => void;
  onClose?: () => void;
  onFocus?: () => void;
  onDragStart?: (e: ReactMouseEvent) => void;
  onResizeStart?: (edge: WindowEdge, e: ReactMouseEvent) => void;
}

/** `var(--bos-win-x, <dark default>)`: the token, or the OS default when no token is set. */
export const tok = (k: WinToken) => `var(${k}, ${WIN_TOKENS_DARK[k]})`;

const EDGE = 4;
const CORNER = 10;
/** The eight resize strips of the frame and what each starts. */
export const FRAME_STRIPS: { edge: WindowEdge; style: CSSProperties; cursor: string }[] = [
  { edge: "North", style: { top: 0, left: CORNER, right: CORNER, height: EDGE }, cursor: "ns-resize" },
  { edge: "South", style: { bottom: 0, left: CORNER, right: CORNER, height: EDGE }, cursor: "ns-resize" },
  { edge: "West", style: { left: 0, top: CORNER, bottom: CORNER, width: EDGE }, cursor: "ew-resize" },
  { edge: "East", style: { right: 0, top: CORNER, bottom: CORNER, width: EDGE }, cursor: "ew-resize" },
  { edge: "NorthWest", style: { top: 0, left: 0, width: CORNER, height: CORNER }, cursor: "nwse-resize" },
  { edge: "NorthEast", style: { top: 0, right: 0, width: CORNER, height: CORNER }, cursor: "nesw-resize" },
  { edge: "SouthWest", style: { bottom: 0, left: 0, width: CORNER, height: CORNER }, cursor: "nesw-resize" },
  { edge: "SouthEast", style: { bottom: 0, right: 0, width: CORNER, height: CORNER }, cursor: "nwse-resize" },
];

export function ChromeButton({ label, danger, onClick, children }: { label: string; danger?: boolean; onClick?: () => void; children: ReactNode }) {
  const [hover, setHover] = useState(false);
  const [focus, setFocus] = useState(false);
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      data-win-btn={label.toLowerCase()}
      onMouseDown={(e) => e.stopPropagation()}
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      onFocus={() => setFocus(true)}
      onBlur={() => setFocus(false)}
      style={{
        display: "grid",
        placeItems: "center",
        width: 40,
        height: "100%",
        border: 0,
        padding: 0,
        cursor: "default",
        color: hover && danger ? "#fff" : "inherit",
        background: hover ? tok(danger ? "--bos-win-btn-danger" : "--bos-win-btn-hover") : "transparent",
        outline: focus ? `2px solid ${tok("--bos-win-focus-ring")}` : "none",
        outlineOffset: -2,
      }}
    >
      {children}
    </button>
  );
}

export function WindowFrame({ title, icon, badge, children, active = true, maximized = false, snapPreview, actions, onMinimize, onMaximize, onClose, onFocus, onDragStart, onResizeStart }: WindowFrameProps) {
  const start = (e: ReactMouseEvent) => {
    if (e.button === 0) onDragStart?.(e);
  };
  return (
    <div
      role="dialog"
      aria-label={title}
      data-testid="bos-window-frame"
      data-active={active ? "true" : "false"}
      data-maximized={maximized ? "true" : "false"}
      onMouseDownCapture={() => onFocus?.()}
      style={{
        position: "relative",
        display: "flex",
        flexDirection: "column",
        width: "100%",
        height: "100%",
        overflow: "hidden",
        background: tok("--bos-win-bg"),
        color: tok("--bos-win-fg"),
        border: maximized ? 0 : `1px solid ${tok("--bos-win-border")}`,
        borderRadius: maximized ? 0 : tok("--bos-win-radius"),
        boxShadow: maximized ? "none" : tok("--bos-win-shadow"),
      }}
    >
      <header
        data-win-part="title"
        onMouseDown={start}
        onDoubleClick={() => onMaximize?.()}
        style={{
          display: "flex",
          flexShrink: 0,
          alignItems: "center",
          gap: 8,
          height: tok("--bos-win-title-h"),
          paddingLeft: 10,
          userSelect: "none",
          borderBottom: `1px solid ${tok("--bos-win-border")}`,
          background: tok(active ? "--bos-win-title-bg" : "--bos-win-title-bg-inactive"),
          opacity: active ? 1 : 0.85,
          fontSize: 13,
        }}
      >
        {icon != null && (
          <span data-win-part="icon" aria-hidden style={{ display: "grid", placeItems: "center", width: 16, height: 16, pointerEvents: "none" }}>
            {icon}
          </span>
        )}
        <span data-win-part="label" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontWeight: 600, pointerEvents: "none" }}>
          {title}
        </span>
        {badge != null && (
          <span data-win-part="badge" style={{ pointerEvents: "none" }}>
            {badge}
          </span>
        )}
        <div data-win-part="drag" style={{ flex: 1, minWidth: 16, height: "100%" }} />
        <div style={{ display: "flex", height: "100%", alignItems: "stretch" }}>
          {actions != null && (
            <div data-win-part="actions" onMouseDown={(e) => e.stopPropagation()} style={{ display: "flex", alignItems: "stretch" }}>
              {actions}
            </div>
          )}
          <ChromeButton label="Minimize" onClick={onMinimize}>
            <Minus size={16} />
          </ChromeButton>
          <ChromeButton label={maximized ? "Restore" : "Maximize"} onClick={onMaximize}>
            {maximized ? <Copy size={13} /> : <Square size={13} />}
          </ChromeButton>
          <ChromeButton label="Close" danger onClick={onClose}>
            <X size={16} />
          </ChromeButton>
        </div>
      </header>
      <div data-win-part="content" style={{ position: "relative", flex: 1, minHeight: 0, overflow: "auto" }}>
        {children}
      </div>
      {!maximized &&
        FRAME_STRIPS.map((s) => (
          <div
            key={s.edge}
            data-win-resize={s.edge}
            onMouseDown={(e) => e.button === 0 && onResizeStart?.(s.edge, e)}
            style={{ position: "absolute", zIndex: 2, ...s.style, cursor: s.cursor }}
          />
        ))}
      {snapPreview && (
        <div
          data-win-part="snap-preview"
          aria-hidden
          style={{
            position: "fixed",
            left: snapPreview.x,
            top: snapPreview.y,
            width: snapPreview.w,
            height: snapPreview.h,
            pointerEvents: "none",
            zIndex: 3,
            background: tok("--bos-win-snap-preview"),
            border: `2px solid ${tok("--bos-win-focus-ring")}`,
            borderRadius: tok("--bos-win-radius"),
          }}
        />
      )}
    </div>
  );
}
