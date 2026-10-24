// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages visibility tabs (parity/specs/pages.md §2.6 and §3.1,
// PAGE-002). Real links carry the one supported query value, so a tab is
// shareable; selecting the already-active tab is deliberately inert.
// The strip only mounts once the collection has at least one row.

import { Link } from "@tanstack/react-router";
import * as React from "react";

import { PAGE_LIST_TABS, pageTabHref, type PageListTab } from "../lib/pageList.js";

export interface PagesListTabsProps {
  workspaceSlug: string;
  projectId: string;
  activeTab: PageListTab;
}

export function PagesListTabs({ workspaceSlug, projectId, activeTab }: PagesListTabsProps): React.ReactElement {
  return (
    <nav aria-label="Page visibility" data-testid="pages-list-tabs" className="flex items-center gap-(--space-1)">
      {PAGE_LIST_TABS.map((tab) => {
        const active = tab.key === activeTab;
        return (
          <Link
            key={tab.key}
            to={pageTabHref(workspaceSlug, projectId, tab.key)}
            aria-current={active ? "page" : undefined}
            className={
              active
                ? "text-body rounded-(--radius-control) bg-(--subtle) px-(--space-3) py-(--space-1) font-medium text-(--text)"
                : "text-body rounded-(--radius-control) px-(--space-3) py-(--space-1) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
            }
          >
            <div className={active ? "text-(--text)" : "text-(--text-muted)"}>{tab.label}</div>
          </Link>
        );
      })}
    </nav>
  );
}
