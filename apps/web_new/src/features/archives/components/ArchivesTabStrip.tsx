// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archives tab strip (parity/specs/archives.md §1.2, ARCH-001). The
// fixed work-items/cycles/modules strip above every archives screen.
// Each tab is a plain client-side link with its own address; the tab
// matching the route reads active. Gating lives in the tab model.

import { Link } from "@tanstack/react-router";
import { cn } from "@pidash/kit";
import * as React from "react";

import type { ArchivesTab, ArchivesTabKey } from "../tabs.js";

export interface ArchivesTabStripProps {
  tabs: ArchivesTab[];
  activeTab: ArchivesTabKey;
}

const LINK_TONE =
  "text-body rounded-(--radius-control) px-(--space-3) py-(--space-2) whitespace-nowrap hover:text-(--text)";

export function ArchivesTabStrip({ tabs, activeTab }: ArchivesTabStripProps): React.ReactElement {
  return (
    <nav aria-label="Archives" data-testid="archives-tab-strip">
      <ul className="flex items-center gap-(--space-1)">
        {tabs.map((tab) => {
          const active = tab.key === activeTab;
          return (
            <li key={tab.key}>
              <Link
                to={tab.href}
                aria-current={active ? "page" : undefined}
                className={cn(LINK_TONE, active ? "text-(--text) underline underline-offset-4" : "text-(--text-muted)")}
              >
                {tab.label}
              </Link>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
