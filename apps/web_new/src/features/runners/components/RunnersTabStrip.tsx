// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners sub-tabs (NEWFRONT-540, RUN-003). Three links into the current
// scope's tree; the active marker comes from the URL, and the overview
// entry only matches its exact path so child routes highlight their own
// tab. Pure routing: no data reads here.

import { Link } from "@tanstack/react-router";
import * as React from "react";

import { runnersBase, type RunnersScopeInput } from "../scope.js";

export interface RunnersTab {
  key: "overview" | "runs" | "approvals";
  label: string;
  /** Path relative to the area base; empty for the overview root. */
  child: "" | "runs" | "approvals";
}

export const RUNNERS_TABS: readonly RunnersTab[] = [
  { key: "overview", label: "Overview", child: "" },
  { key: "runs", label: "Runs", child: "runs" },
  { key: "approvals", label: "Approvals", child: "approvals" },
];

/**
 * Which tab a pathname selects. Trailing slashes are ignored; the
 * overview tab requires an exact match on the area base.
 */
export function runnersActiveTabKey(pathname: string, scope: RunnersScopeInput): RunnersTab["key"] | null {
  const base = runnersBase(scope).replace(/\/+$/, "");
  const current = pathname.replace(/\/+$/, "");
  if (current === base) return "overview";
  if (current === `${base}/runs` || current.startsWith(`${base}/runs/`)) return "runs";
  if (current === `${base}/approvals`) return "approvals";
  return null;
}

export interface RunnersTabStripProps {
  scope: RunnersScopeInput;
  /** Current pathname; the caller reads it from the router. */
  pathname: string;
}

export function RunnersTabStrip({ scope, pathname }: RunnersTabStripProps): React.ReactElement {
  const base = runnersBase(scope);
  const active = runnersActiveTabKey(pathname, scope);
  return (
    <nav aria-label="Runners tabs" data-testid="runners-tabs" className="flex border-b border-(--border)">
      {RUNNERS_TABS.map((tab) => {
        const href = tab.child === "" ? base : `${base}/${tab.child}`;
        const selected = tab.key === active;
        // The router marks fuzzy-active links aria-current="page" itself,
        // overriding the explicit marker: without exact matching the
        // overview link stays marked on every child route (NEWFRONT-557).
        // Runs stays fuzzy so a run detail still selects it.
        return (
          <Link
            key={tab.key}
            to={href}
            activeOptions={tab.key === "runs" ? { exact: false } : { exact: true }}
            aria-current={selected ? "page" : undefined}
            className={`text-body flex h-9 items-center border-b-2 px-(--space-3) font-medium ${
              selected
                ? "border-(--accent) text-(--text)"
                : "border-transparent text-(--text-muted) hover:text-(--text)"
            }`}
          >
            {tab.label}
          </Link>
        );
      })}
    </nav>
  );
}
