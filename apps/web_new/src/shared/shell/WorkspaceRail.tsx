// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Far-left app rail (SHELL-063, SHELL-064, SHELL-099, from
// parity/specs/shell.md §5). Dock entries from the edition on top, the
// workspace settings entry pinned below them, and a right-click menu
// for density plus docking. Density and per-workspace docking persist
// through the rail store; the route mounts this only while the edition
// enables the rail and the workspace keeps it docked.
import { Check, LayoutGrid, Settings } from "lucide-react";
import * as React from "react";

import { MENU_PORTAL_ID, OverlayPortal } from "./document.js";
import { isRailUndocked, useRailState, type RailDensity } from "./railState.js";

/**
 * One edition dock entry rendered in the rail strip. The icon is the
 * rail's own mark, not per-entry edition data: it is built here so the
 * lazy rail chunk carries it instead of the eager shell.
 */
export interface RailDockItem {
  id: string;
  label: string;
  to: string;
}

export interface WorkspaceRailProps {
  workspaceSlug: string;
  /** Current pathname, for the entry active states. */
  currentPath: string;
  dockItems: RailDockItem[];
}

/** A dock entry reads active under its target except on settings and inbox routes. */
export function isDockItemActive(currentPath: string, itemTo: string, workspaceSlug: string): boolean {
  const targetPrefix = itemTo.endsWith("/") ? itemTo : `${itemTo}/`;
  const underTarget = currentPath === itemTo || currentPath.startsWith(targetPrefix);
  if (!underTarget) return false;
  if (currentPath.startsWith(`/${workspaceSlug}/settings`)) return false;
  return !currentPath.startsWith(`/${workspaceSlug}/notifications`);
}

/** The settings entry highlights on workspace settings routes only. */
export function isSettingsEntryActive(currentPath: string, workspaceSlug: string): boolean {
  return currentPath.startsWith(`/${workspaceSlug}/settings`);
}

export function WorkspaceRail({ workspaceSlug, currentPath, dockItems }: WorkspaceRailProps): React.ReactElement {
  const density = useRailState((state) => state.density);
  const undocked = useRailState((state) => isRailUndocked(state, workspaceSlug));
  const setDensity = useRailState((state) => state.setDensity);
  const setUndocked = useRailState((state) => state.setUndocked);
  const [menuAt, setMenuAt] = React.useState<{ x: number; y: number } | null>(null);

  const captioned = density === "labeled";
  const settingsActive = isSettingsEntryActive(currentPath, workspaceSlug);

  return (
    <nav
      aria-label="Applications"
      data-testid="workspace-rail"
      onContextMenu={(event) => {
        event.preventDefault();
        setMenuAt({ x: event.clientX, y: event.clientY });
      }}
      className={`flex h-full shrink-0 flex-col justify-between gap-(--space-4) border-r border-(--border) bg-(--bg) px-(--space-2) py-(--space-3) ${
        captioned ? "w-15" : "w-12"
      }`}
    >
      <ul className={`flex flex-col ${captioned ? "gap-(--space-4)" : "gap-(--space-3)"}`}>
        {dockItems.map((item) => {
          const active = isDockItemActive(currentPath, item.to, workspaceSlug);
          return (
            <li key={item.id}>
              <a
                href={item.to}
                aria-current={active ? "page" : undefined}
                aria-label={item.label}
                className={`flex flex-col items-center gap-(--space-1) rounded-(--radius-control) px-(--space-1) py-(--space-2) ${
                  active ? "bg-(--subtle) text-(--text)" : "text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
                }`}
              >
                <LayoutGrid size={20} strokeWidth={1.5} aria-hidden="true" />
                {captioned ? <span className="text-caption max-w-full truncate">{item.label}</span> : null}
              </a>
            </li>
          );
        })}
      </ul>
      <div className="flex flex-col gap-(--space-2)">
        <div aria-hidden="true" className="mx-(--space-2) border-t border-(--border)" />
        <a
          href={`/${workspaceSlug}/settings`}
          aria-label="Settings"
          aria-current={settingsActive ? "page" : undefined}
          className={`flex flex-col items-center gap-(--space-1) rounded-(--radius-control) px-(--space-1) py-(--space-2) ${
            settingsActive
              ? "bg-(--subtle) text-(--text)"
              : "text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
          }`}
        >
          <Settings size={20} strokeWidth={1.5} aria-hidden="true" />
          {captioned ? <span className="text-caption max-w-full truncate">Settings</span> : null}
        </a>
      </div>
      {menuAt ? (
        <OverlayPortal rootId={MENU_PORTAL_ID}>
          <RailMenu
            position={menuAt}
            density={density}
            undocked={undocked}
            onDensity={(next) => {
              setDensity(next);
              setMenuAt(null);
            }}
            onDockToggle={() => {
              setUndocked(workspaceSlug, !undocked);
              setMenuAt(null);
            }}
            onClose={() => setMenuAt(null)}
          />
        </OverlayPortal>
      ) : null}
    </nav>
  );
}

function RailMenu({
  position,
  density,
  undocked,
  onDensity,
  onDockToggle,
  onClose,
}: {
  position: { x: number; y: number };
  density: RailDensity;
  undocked: boolean;
  onDensity: (density: RailDensity) => void;
  onDockToggle: () => void;
  onClose: () => void;
}): React.ReactElement {
  const menuRef = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    menuRef.current?.focus();
    const onPointerDown = (event: PointerEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) onClose();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [onClose]);

  const itemClasses =
    "text-body flex w-full cursor-pointer items-center justify-between gap-(--space-2) rounded-(--radius-control) px-(--space-4) py-(--space-3) text-left text-(--text) outline-none hover:bg-(--subtle)";

  return (
    <div
      ref={menuRef}
      tabIndex={-1}
      role="menu"
      aria-label="App rail options"
      style={{ left: position.x, top: position.y }}
      className="shadow-popover fixed min-w-48 rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-2)"
    >
      <button
        type="button"
        role="menuitemradio"
        aria-checked={density === "icons"}
        onClick={() => onDensity("icons")}
        className={itemClasses}
      >
        <span>Icon only</span>
        {density === "icons" ? <Check size={14} strokeWidth={2} aria-hidden="true" /> : null}
      </button>
      <button
        type="button"
        role="menuitemradio"
        aria-checked={density === "labeled"}
        onClick={() => onDensity("labeled")}
        className={itemClasses}
      >
        <span>Icon with name</span>
        {density === "labeled" ? <Check size={14} strokeWidth={2} aria-hidden="true" /> : null}
      </button>
      <div role="separator" className="mx-(--space-2) my-(--space-2) border-t border-(--border)" />
      <button type="button" role="menuitem" onClick={onDockToggle} className={itemClasses}>
        <span>{undocked ? "Dock App Rail" : "Undock App Rail"}</span>
      </button>
    </div>
  );
}
