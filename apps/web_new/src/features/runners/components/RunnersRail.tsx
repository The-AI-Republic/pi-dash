// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners side rail (NEWFRONT-540, RUN-004; built-in contact NEWFRONT-553,
// RUN-033). An overview link, the desktop's built-in agent when this build
// has one, then one chat contact per runner, scoped to the current
// workspace or project. No loading skeleton and no error row: an
// unfetched or empty list reads as "none connected".

import { Link } from "@tanstack/react-router";
import { Bot, LayoutDashboard } from "lucide-react";
import * as React from "react";

import type { Runner } from "@pidash/api-client";
import type { LocalContact } from "../chat/localContact.js";
import { runnersBase, type RunnersScopeInput } from "../scope.js";
import { runnerDisplayStatus } from "../status.js";
import { RunnerStatusDot } from "./RunnerStatusBadge.js";

/** Rail header: the canonical area name (D27, RUN-004/RUN-005). */
export const RAIL_HEADER = "AI Workers";
export const BUILTIN_SECTION_HEADER = "Built-in";

/** Chat address for one runner; plain string per the router's Link typing. */
export function chatHref(base: string, runnerId: string): string {
  return `${base}/chat/${runnerId}`;
}

export interface RunnersRailProps {
  scope: RunnersScopeInput;
  runners: readonly Runner[];
  /** Project scope picks the narrower empty-state sentence. */
  isProjectScope: boolean;
  /** The built-in local contact (desktop only; empty elsewhere). */
  localContacts?: readonly LocalContact[];
}

export function RunnersRail({
  scope,
  runners,
  isProjectScope,
  localContacts = [],
}: RunnersRailProps): React.ReactElement {
  const base = runnersBase(scope);
  return (
    <aside
      aria-label="AI Workers navigation"
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
        {localContacts.length === 0 ? null : (
          <>
            <div className="text-meta mt-(--space-2) px-(--space-2) font-semibold tracking-wide text-(--text-muted) uppercase">
              {BUILTIN_SECTION_HEADER}
            </div>
            {localContacts.map((contact) =>
              contact.unavailableReason === null ? (
                <Link
                  key={contact.id}
                  to={chatHref(base, contact.id)}
                  data-testid="runners-rail-local-contact"
                  className="text-body flex min-h-11 items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
                >
                  <Bot size={16} strokeWidth={1.5} aria-hidden="true" className="shrink-0" />
                  <span className="min-w-0 flex-1 truncate">{contact.name}</span>
                  <span className="text-caption text-(--text-muted)">{contact.label}</span>
                </Link>
              ) : (
                <span
                  key={contact.id}
                  data-testid="runners-rail-local-contact"
                  aria-disabled="true"
                  title={contact.unavailableReason}
                  className="text-body flex min-h-11 cursor-not-allowed flex-col gap-(--space-1) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) opacity-60"
                >
                  <span className="flex items-center gap-(--space-2)">
                    <Bot size={16} strokeWidth={1.5} aria-hidden="true" className="shrink-0" />
                    <span className="min-w-0 flex-1 truncate">{contact.name}</span>
                    <span className="text-caption">{contact.label}</span>
                  </span>
                  <span data-testid="runners-rail-local-reason" className="text-caption pl-6">
                    {contact.unavailableReason}
                  </span>
                </span>
              )
            )}
          </>
        )}
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
