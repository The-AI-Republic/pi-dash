// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dashboard sidebar (ADM-062, ADM-063, from parity/specs/admin.md
// §8.1/§8.2): the settings-area navigation with prefix highlight,
// plus the collapse mechanics — footer toggle, narrow-viewport
// one-way auto-collapse, small-screen outside-tap collapse, and the
// off-canvas mobile rail. God-mode's own chrome: the main shell
// sidebar is not reused.
import { Link, useLocation } from "@tanstack/react-router";
import { Cpu, LayoutGrid, Repeat, Send, Settings, ShieldCheck, Sparkles } from "lucide-react";
import * as React from "react";

import { Tooltip } from "@pidash/kit";

import {
  ADMIN_NARROW_VIEWPORT_PX,
  ADMIN_SIDEBAR_ENTRIES,
  isSidebarEntryActive,
  type AdminSidebarEntryDef,
} from "../adminShell.js";
import { useAdminSidebarStore } from "../stores.js";
import { AdminAccountMenu } from "./AdminAccountMenu.js";
import { AdminHelpSection } from "./AdminHelpSection.js";

/** Fresh icon per entry; every table key is covered (see the suite). */
const ENTRY_ICONS: Record<string, typeof Settings> = {
  general: Settings,
  email: Send,
  authentication: ShieldCheck,
  workspace: LayoutGrid,
  ai: Sparkles,
  loop: Repeat,
  runners: Cpu,
};

type RouterLinkProps = Parameters<typeof Link>[0];

// Console pages land slice by slice, so most entry targets are not in
// the route tree yet; the cast carries the literal through the same
// way the first-workspace shortcut reaches its out-of-tree page.
function entryLinkProps(href: string): RouterLinkProps {
  return { to: href } as RouterLinkProps;
}

function SidebarEntry({
  entry,
  pathname,
  collapsed,
}: {
  entry: AdminSidebarEntryDef;
  pathname: string;
  collapsed: boolean;
}): React.ReactElement {
  const active = isSidebarEntryActive(pathname, entry.href);
  const Icon = ENTRY_ICONS[entry.key]!;
  const link = (
    <Link
      {...entryLinkProps(entry.href)}
      aria-current={active ? "page" : undefined}
      data-testid={`admin-nav-${entry.slug}`}
      className={
        active
          ? "flex w-full items-center gap-(--space-3) rounded-(--radius-control) bg-(--subtle) p-(--space-3) text-(--text)"
          : "flex w-full items-center gap-(--space-3) rounded-(--radius-control) p-(--space-3) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
      }
    >
      <Icon size={18} aria-hidden="true" className="shrink-0" />
      {collapsed ? null : (
        <span className="flex min-w-0 flex-col">
          <span className="text-body block truncate font-medium">{entry.name}</span>
          <span className="text-caption block truncate">{entry.description}</span>
        </span>
      )}
    </Link>
  );
  if (!collapsed) return link;
  return (
    <Tooltip content={entry.name}>
      <span className="flex w-full items-center justify-center">{link}</span>
    </Tooltip>
  );
}

const RAIL_BASE =
  "fixed inset-y-0 left-0 z-40 flex flex-col border-r border-(--border) bg-(--surface) transition-transform duration-300 md:relative md:z-auto";

export function AdminSidebar(): React.ReactElement {
  const collapsed = useAdminSidebarStore((state) => state.collapsed);
  const setCollapsed = useAdminSidebarStore((state) => state.setCollapsed);
  const { pathname } = useLocation();
  const railRef = React.useRef<HTMLElement>(null);

  // Narrow viewports collapse the rail one-way: narrowing collapses
  // (including the small-screen mount), widening never re-expands.
  React.useEffect(() => {
    const collapseWhenNarrow = (): void => {
      if (window.innerWidth <= ADMIN_NARROW_VIEWPORT_PX) setCollapsed(true);
    };
    collapseWhenNarrow();
    window.addEventListener("resize", collapseWhenNarrow);
    return () => window.removeEventListener("resize", collapseWhenNarrow);
  }, [setCollapsed]);

  // An outside tap on small screens hides the open off-canvas rail.
  React.useEffect(() => {
    const collapseOnOutsideTap = (event: PointerEvent): void => {
      if (collapsed) return;
      if (window.innerWidth >= ADMIN_NARROW_VIEWPORT_PX) return;
      // Mounted while the listener lives, so the rail always exists.
      if (!railRef.current!.contains(event.target as Node)) setCollapsed(true);
    };
    globalThis.document.addEventListener("pointerdown", collapseOnOutsideTap);
    return () => globalThis.document.removeEventListener("pointerdown", collapseOnOutsideTap);
  }, [collapsed, setCollapsed]);

  return (
    <aside
      ref={railRef}
      data-testid="admin-sidebar"
      aria-label="Settings navigation"
      className={collapsed ? `${RAIL_BASE} w-72 -translate-x-full md:w-16 md:translate-x-0` : `${RAIL_BASE} w-72`}
    >
      <AdminAccountMenu />
      <nav aria-label="Settings areas" className="flex min-h-0 flex-1 flex-col overflow-y-auto p-(--space-3)">
        <ul className="flex flex-col gap-(--space-1)">
          {ADMIN_SIDEBAR_ENTRIES.map((entry) => (
            <li key={entry.key}>
              <SidebarEntry entry={entry} pathname={pathname} collapsed={collapsed} />
            </li>
          ))}
        </ul>
      </nav>
      <AdminHelpSection />
    </aside>
  );
}
