// SPDX-License-Identifier: Apache-2.0
// Copyright (c) SuperiorSoft LLC
// Where the kit's floating parts (dialogs, selects, menus, tooltips) render: document.body, unless an ancestor names
// another element with <PortalContainer>. A page that needs its dialogs inside its own
// style scope names its own root.
import { createContext, useContext, type ReactNode } from "react";

const PortalContainerContext = createContext<HTMLElement | null>(null);

/** Renders [children] with the kit's portals going into [container] (null: document.body). */
export function PortalContainer({ container, children }: { container: HTMLElement | null; children: ReactNode }) {
  return <PortalContainerContext value={container}>{children}</PortalContainerContext>;
}

/** The element the nearest <PortalContainer> names, or undefined (Radix's default: document.body). */
export function usePortalContainer(): HTMLElement | undefined {
  return useContext(PortalContainerContext) ?? undefined;
}
