// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Narrow-screen settings bar (parity/specs/workspace-settings.md §1,
// WSET-001). Below the desktop breakpoint the sidebar disappears; a
// hamburger opens the same filtered entries in a floating panel beside a
// crumb naming the current section. The panel closes on an outside press
// or after following an entry. Above the breakpoint the bar is hidden.

import { Menu } from "lucide-react";
import * as React from "react";

import type { VisibleWorkspaceSettingsGroup } from "../lib/sections.js";
import { SettingsNavigation } from "./SettingsNavigation.js";

export interface SettingsMobileNavProps {
  groups: readonly VisibleWorkspaceSettingsGroup[];
  activeSection: string;
  activeLabel: string;
  onNavigate: (href: string) => void;
}

export function SettingsMobileNav({
  groups,
  activeSection,
  activeLabel,
  onNavigate,
}: SettingsMobileNavProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const rootRef = React.useRef<HTMLDivElement | null>(null);
  const triggerRef = React.useRef<HTMLButtonElement | null>(null);

  React.useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent): void => {
      const target = event.target as Node;
      if (rootRef.current?.contains(target) === true) return;
      setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [open]);

  return (
    <div ref={rootRef} className="relative flex shrink-0 flex-col border-b border-(--border) md:hidden">
      <div className="flex items-center gap-(--space-2) px-(--space-3) py-(--space-2)">
        <button
          ref={triggerRef}
          type="button"
          aria-label="Open settings navigation"
          aria-expanded={open}
          aria-controls="workspace-settings-drawer"
          onClick={() => {
            setOpen((current) => !current);
          }}
          className="inline-flex size-8 items-center justify-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
        >
          <Menu size={18} strokeWidth={1.5} aria-hidden="true" />
        </button>
        <span data-testid="workspace-settings-mobile-crumb" className="text-body truncate text-(--text)">
          {activeLabel}
        </span>
      </div>
      {open ? (
        <div
          id="workspace-settings-drawer"
          data-testid="workspace-settings-drawer"
          className="shadow-dialog absolute top-full left-0 z-20 w-72 rounded-(--radius-control) border border-(--border) bg-(--bg) p-(--space-2) md:hidden"
          onKeyDown={(event) => {
            if (event.key !== "Escape") return;
            event.stopPropagation();
            setOpen(false);
            triggerRef.current?.focus();
          }}
        >
          <SettingsNavigation
            idPrefix="workspace-settings-drawer-group"
            groups={groups}
            activeSection={activeSection}
            onNavigate={onNavigate}
            onFollow={() => {
              setOpen(false);
            }}
          />
        </div>
      ) : null}
    </div>
  );
}
