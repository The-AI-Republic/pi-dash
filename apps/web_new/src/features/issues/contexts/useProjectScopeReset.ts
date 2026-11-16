// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-scoped resets for the archives tabs (parity/specs/archives.md
// §2.2/§2.5). The archived expression, display choices and peek selection
// all belong to one project; a project switch must not carry them over.

import * as React from "react";

/** The peek params the archived list address can carry. */
const PEEK_PARAMS = ["peekIssueId", "peekProjectId", "peekNestingLevel"] as const;

/**
 * The search record with every peek param removed, or null when the record
 * carries none (a reset with nothing to do must not navigate). A selected
 * item for another project is still cleared: the archives peek is
 * same-project only.
 */
export function searchWithoutPeek(search: Record<string, unknown>): Record<string, unknown> | null {
  const carries = PEEK_PARAMS.some((param) => search[param] !== undefined);
  if (!carries) return null;
  const next = { ...search };
  for (const param of PEEK_PARAMS) delete next[param];
  return next;
}

/**
 * Runs `onScopeChange` once whenever the scope key changes, never on the
 * first mount. Callers pass the peek-clearing navigation; the first mount
 * is skipped because a reload of the same address must keep its peek.
 */
export function useProjectScopeReset(scope: string, onScopeChange: () => void): void {
  const previous = React.useRef(scope);
  const latest = React.useRef(onScopeChange);
  latest.current = onScopeChange;
  React.useEffect(() => {
    if (previous.current === scope) return;
    previous.current = scope;
    latest.current();
  }, [scope]);
}

/** Callback a project switch uses to rewrite the address. */
export type PeekResetApply = (search: Record<string, unknown>) => void;

/**
 * Clears a carried-over peek when the archived tab switches project. A
 * switch whose address carries no peek params does nothing, so a plain
 * project change never pushes a redundant history entry.
 */
export function usePeekResetOnProjectChange(
  scope: string,
  search: Record<string, unknown>,
  apply: PeekResetApply
): void {
  const latestApply = React.useRef(apply);
  latestApply.current = apply;
  useProjectScopeReset(scope, () => {
    const next = searchWithoutPeek(search);
    if (next !== null) latestApply.current(next);
  });
}
