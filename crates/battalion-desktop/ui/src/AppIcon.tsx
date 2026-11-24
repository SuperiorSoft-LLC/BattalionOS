// SPDX-License-Identifier: MPL-2.0
// An app's icon: its own picture when the manifest or catalog has one, else the generic glyph.
import { AppWindow } from "lucide-react";

export function AppIcon({ icon, className = "size-6" }: { icon: string | null; className?: string }) {
  if (!icon) return <AppWindow className={className} aria-hidden />;
  return <img src={icon} alt="" className={`${className} object-contain`} draggable={false} />;
}
