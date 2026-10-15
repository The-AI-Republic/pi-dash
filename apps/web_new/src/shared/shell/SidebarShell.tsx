// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collapsible sidebar shell (SHELL-067/068/069/098, from
// parity/specs/shell.md §6). The container every sidebar-content slice
// renders inside: a title header with the brand and the personalization
// launcher, a quick-actions slot, a scroll area, and a bottom account
// area that mounts only while fully open so exactly one account menu
// exists. The right-edge grip drag-resizes within the store clamps and
// double-click toggles; a hovered collapsed edge previews the navigation
// in a temporary overlay; below the breakpoint the open sidebar floats
// above content and collapses on outside presses. Collapse, peek and
// width persist per panel through the chrome store.
import { IconButton } from "@pidash/kit";
import { SlidersHorizontal } from "lucide-react";
import * as React from "react";

import { MAIN_SIDEBAR_PANEL, sidebarPanelStore } from "./chromeState.js";
import { useNarrowViewport } from "./narrowViewport.js";

export interface SidebarShellProps {
  /** Persistence id; panels share the shell but keep separate snapshots. */
  panel?: string;
  /** Stable DOM id for the measured landmark. */
  domId?: string;
  /** Brand mark heading the title row. */
  brand: React.ReactNode;
  /** Opens personalization; without it the launcher stays unmounted. */
  onPersonalize?: (() => void) | undefined;
  /** Quick actions under the header (the quick-create slice fills this). */
  quickActions?: React.ReactNode | undefined;
  /** Scroll-area content: switcher, entries, projects. */
  children: React.ReactNode;
  /** Bottom account row; mounted only while fully open. */
  account?: React.ReactNode | undefined;
}

/** How long a vacated peek overlay lingers before dismissing itself. */
export const SIDEBAR_PEEK_DISMISS_MS = 500;

function ShellHeader({ brand, launcher }: { brand: React.ReactNode; launcher?: React.ReactNode }): React.ReactElement {
  return (
    <div className="flex items-center justify-between gap-(--space-2)">
      <div className="min-w-0 flex-1">{brand}</div>
      {launcher}
    </div>
  );
}

function ShellScroll({ children }: { children: React.ReactNode }): React.ReactElement {
  return <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto">{children}</div>;
}

export function SidebarShell({
  panel = MAIN_SIDEBAR_PANEL,
  domId = "main-sidebar",
  brand,
  onPersonalize,
  quickActions,
  children,
  account,
}: SidebarShellProps): React.ReactElement {
  const store = sidebarPanelStore(panel);
  const collapsed = store((state) => state.sidebarCollapsed);
  const peekOpen = store((state) => state.peekOpen);
  const width = store((state) => state.sidebarWidth);
  const narrow = useNarrowViewport();
  const [resizing, setResizing] = React.useState(false);
  const asideRef = React.useRef<HTMLElement | null>(null);
  const overlayRef = React.useRef<HTMLDivElement | null>(null);
  const dragStart = React.useRef({ x: 0, width: 0 });
  const peekTimer = React.useRef<ReturnType<typeof setTimeout> | null>(null);

  const cancelPeekDismiss = (): void => {
    if (peekTimer.current !== null) {
      clearTimeout(peekTimer.current);
      peekTimer.current = null;
    }
  };

  const schedulePeekDismiss = (): void => {
    cancelPeekDismiss();
    peekTimer.current = setTimeout(() => {
      peekTimer.current = null;
      store.getState().clearPeek();
    }, SIDEBAR_PEEK_DISMISS_MS);
  };

  // A pending dismissal must not fire after unmount.
  React.useEffect(
    () => () => {
      if (peekTimer.current !== null) clearTimeout(peekTimer.current);
    },
    []
  );

  // Crossing below the breakpoint collapses the open sidebar; widening
  // never reopens it — the toggle does. Reads the store directly so a
  // toggle while narrow does not retrigger the effect.
  React.useEffect(() => {
    if (narrow && !store.getState().sidebarCollapsed) {
      store.getState().clearPeek();
      store.getState().setSidebarCollapsed(true);
    }
  }, [narrow, store]);

  // The floating sidebar dismisses on outside presses; the docked one
  // ignores them.
  React.useEffect(() => {
    if (!narrow || collapsed) return;
    const onPointerDown = (event: PointerEvent): void => {
      if (!asideRef.current!.contains(event.target as Node)) {
        store.getState().clearPeek();
        store.getState().setSidebarCollapsed(true);
      }
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [narrow, collapsed, store]);

  // Grip drag: track the pointer on the document so fast drags past the
  // edge keep resizing; the store clamps into the draggable range.
  React.useEffect(() => {
    if (!resizing) return;
    const onMove = (event: MouseEvent): void => {
      store.getState().setSidebarWidth(dragStart.current.width + (event.clientX - dragStart.current.x));
    };
    const onUp = (): void => {
      setResizing(false);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    return () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
  }, [resizing, store]);

  const flip = (): void => {
    cancelPeekDismiss();
    store.getState().clearPeek();
    store.getState().toggleSidebarCollapsed();
  };

  const open = !collapsed;
  const floating = narrow && open;
  const showPeek = collapsed && peekOpen;
  const launcher =
    onPersonalize === undefined ? undefined : (
      <IconButton label="Personalize sidebar" data-testid="personalize-nav" onClick={onPersonalize}>
        <SlidersHorizontal size={16} strokeWidth={1.5} aria-hidden="true" />
      </IconButton>
    );

  return (
    <>
      <aside
        id={domId}
        ref={asideRef}
        aria-label="Workspace navigation"
        inert={collapsed}
        style={{ width: collapsed ? 0 : width }}
        className={`relative flex shrink-0 flex-col gap-(--space-4) border-r border-(--border) bg-(--surface) p-(--space-4)${
          collapsed ? " w-0 overflow-hidden" : ""
        }${floating ? " shadow-popover absolute inset-y-0 left-0 z-40" : ""}${
          resizing ? "" : " transition-[width] duration-(--motion-duration) ease-in-out motion-reduce:transition-none"
        }`}
      >
        <ShellHeader brand={brand} launcher={launcher} />
        {quickActions === undefined ? null : <div data-testid="sidebar-quick-actions">{quickActions}</div>}
        <ShellScroll>{children}</ShellScroll>
        {open && account !== undefined ? (
          <div className="flex items-center gap-(--space-2) border-t border-(--border) pt-(--space-4)">{account}</div>
        ) : null}
        <div
          role="separator"
          aria-label="Resize sidebar"
          aria-orientation="vertical"
          onMouseDown={(event) => {
            dragStart.current = { x: event.clientX, width: store.getState().sidebarWidth };
            setResizing(true);
          }}
          onDoubleClick={flip}
          className={`absolute top-0 right-0 h-full w-1 cursor-ew-resize ${
            resizing ? "bg-(--accent)" : "hover:bg-(--subtle)"
          }`}
        />
      </aside>
      {collapsed && !narrow ? (
        <button
          type="button"
          aria-label="Preview sidebar"
          data-testid="sidebar-peek-trigger"
          onPointerEnter={() => store.getState().setPeekOpen(true)}
          onClick={() => store.getState().setPeekOpen(true)}
          onFocus={() => store.getState().setPeekOpen(true)}
          onBlur={(event) => {
            // Focus moving into the overlay keeps it up; focus anywhere
            // else (or a blur with the overlay already gone) closes it.
            if (overlayRef.current === null || !overlayRef.current.contains(event.relatedTarget as Node | null)) {
              store.getState().clearPeek();
            }
          }}
          className="absolute inset-y-0 left-0 z-30 w-2 cursor-pointer hover:bg-(--subtle) focus-visible:bg-(--subtle)"
        />
      ) : null}
      {showPeek ? (
        <div
          ref={overlayRef}
          role="complementary"
          aria-label="Sidebar preview"
          data-testid="sidebar-peek"
          style={{ width }}
          onPointerEnter={cancelPeekDismiss}
          onPointerLeave={schedulePeekDismiss}
          onKeyDown={(event) => {
            if (event.key === "Escape") store.getState().clearPeek();
          }}
          className="shadow-popover absolute inset-y-0 left-0 z-40 flex flex-col gap-(--space-4) overflow-hidden border-r border-(--border) bg-(--surface) p-(--space-4)"
        >
          <ShellHeader brand={brand} />
          <ShellScroll>{children}</ShellScroll>
        </div>
      ) : null}
    </>
  );
}
