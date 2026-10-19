// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project schedulers section shell (parity/specs/agents.md §5, AGT-021):
// List and Calendar tabs above the tab body. The section renders inside
// the project layout's access gate, so every allowed project viewer reads
// it; the list tab is mounted here and the calendar tab by NEWFRONT-593.

import { createFileRoute, Link, Outlet, useLocation } from "@tanstack/react-router";
import { CalendarClock, List } from "lucide-react";
import * as React from "react";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/schedulers")({
  component: ProjectSchedulersSectionComponent,
});

export interface SchedulerSectionTab {
  key: "list" | "calendar";
  label: string;
  href: string;
  active: boolean;
}

function trimTrailing(pathname: string): string {
  return pathname.replace(/\/+$/, "");
}

/**
 * Tab state for a path inside the section. The bare section address (and
 * its trailing-slash form) reads as Calendar because it redirects there;
 * an install detail page underneath highlights neither tab.
 */
export function schedulerSectionTabs(pathname: string, base: string): SchedulerSectionTab[] {
  const current = trimTrailing(pathname);
  const listHref = `${base}/list`;
  const calendarHref = `${base}/calendar`;
  return [
    { key: "list", label: "List", href: listHref, active: current === listHref },
    {
      key: "calendar",
      label: "Calendar",
      href: calendarHref,
      active: current === calendarHref || current === base || current === `${base}/`,
    },
  ];
}

export function ProjectSchedulersSectionComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const { pathname } = useLocation();
  const tabs = schedulerSectionTabs(pathname, `/${ws}/projects/${projectId}/schedulers`);
  return (
    <div className="flex h-full min-h-0 w-full flex-col">
      <nav
        aria-label="Scheduler views"
        className="flex items-center gap-(--space-1) border-b border-(--border) px-(--space-4)"
      >
        {tabs.map((tab) => (
          <Link
            key={tab.key}
            to={tab.href}
            aria-current={tab.active ? "page" : undefined}
            className={
              tab.active
                ? "text-body flex items-center gap-(--space-2) border-b-2 border-(--accent) px-(--space-3) py-(--space-2) font-medium text-(--text)"
                : "text-body flex items-center gap-(--space-2) border-b-2 border-transparent px-(--space-3) py-(--space-2) text-(--text-muted) hover:text-(--text)"
            }
          >
            {tab.key === "list" ? (
              <List size={16} strokeWidth={1.5} aria-hidden="true" />
            ) : (
              <CalendarClock size={16} strokeWidth={1.5} aria-hidden="true" />
            )}
            {tab.label}
          </Link>
        ))}
      </nav>
      <div className="min-h-0 flex-1 overflow-auto">
        <Outlet />
      </div>
    </div>
  );
}
