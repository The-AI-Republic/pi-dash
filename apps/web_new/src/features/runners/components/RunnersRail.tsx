// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners side rail (NEWFRONT-540, RUN-004). An overview link plus one chat
// contact per runner, scoped to the current workspace or project and
// refreshed on the list's short cadence. No loading skeleton and no error
// row: an unfetched or empty list reads as "none connected".

import { Link } from "@tanstack/react-router";
import { Bot, LayoutDashboard } from "lucide-react";
import * as React from "react";

import type { Runner } from "@pidash/api-client";
import { runnersBase, type RunnersScopeInput } from "../scope.js";
import { runnerDisplayStatus } from "../status.js";
import { RunnerStatusDot } from "./RunnerStatusBadge.js";

export const RAIL_HEADER = "AI Agents";

/** Chat address for one runner; plain string per the router's Link typing. */
export function chatHref(base: string, runnerId: string): string {
  return `${base}/chat/${runnerId}`;
}

export interface RunnersRailProps {
  scope: RunnersScopeInput;
  runners: readonly Runner[];
  /** Project scope picks the narrower empty-state sentence. */
  isProjectScope: boolean;
}

export function RunnersRail({ scope, runners, isProjectScope }: RunnersRailProps): React.ReactElement {
  const base = runnersBase(scope);
  return (
    <aside
      aria-label="AI runners navigation"
      data-testid="runners-rail"
      className="flex w-64 shrink-0 flex-col border-r border-(--border) bg-(--surface)"
    >
      <div className="text-emphasis flex h-12 items-center border-b border-(--border) px-(--space-4) font-semibold text-(--text)">
        {RAIL_HEADER}
      </div>
      <nav className="flex flex-col gap-(--space-1) p-(--space-2)">
        <Link
          to={base}
          activeOptions={{ exact: true }}
          className="text-body flex h-9 items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
        >
          <LayoutDashboard size={16} strokeWidth={1.5} aria-hidden="true" />
          <span>Overview</span>
        </Link>
        <div className="text-meta mt-(--space-2) px-(--space-2) font-medium tracking-wide text-(--text-muted) uppercase">
          Runners
        </div>
        {runners.length === 0 ? (
          <p data-testid="runners-rail-empty" className="text-meta px-(--space-2) py-(--space-2) text-(--text-muted)">
            {isProjectScope ? "No runners connected to this project yet." : "No runners connected yet."}
          </p>
        ) : (
          runners.map((runner) => (
            <Link
              key={runner.id}
              to={chatHref(base, runner.id)}
              data-testid="runners-rail-contact"
              className="text-body flex min-h-11 items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
            >
              <Bot size={16} strokeWidth={1.5} aria-hidden="true" className="shrink-0" />
              <span className="min-w-0 flex-1 truncate">{runner.name}</span>
              <RunnerStatusDot status={runnerDisplayStatus(runner)} />
            </Link>
          ))
        )}
      </nav>
    </aside>
  );
}
