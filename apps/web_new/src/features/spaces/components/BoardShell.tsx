// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public board shell (SPC-001, SPC-016). The shell is deliberately
// separate from the workspace AppShell: no sidebar, title bar, palette,
// workspace switcher or session gate. Slice D fills the navbar controls,
// slice B the landing gate and slice E the layouts (features/spaces/layouts);
// this slice owns the composition points and the loading/failure states
// around them.

import { Spinner } from "@pidash/kit";
import type { SpaceBoardIssue, SpaceBoardSettings } from "@pidash/api-client";
import * as React from "react";

import { projectMarkGlyph } from "../lib/projectMark.js";
import { spaceSignInHref, type SpaceBoardSearch } from "../lib/address.js";

export interface BoardNavbarProps {
  settings: SpaceBoardSettings | undefined;
  anchor: string;
  search: SpaceBoardSearch;
  /** Slice D replaces the right-hand slot; the base keeps a sign-in control. */
  accountSlot?: React.ReactNode;
  children?: React.ReactNode;
}

/** Placeholder while settings load: generic mark and name, no content flash. */
export function BoardNavbar({ settings, anchor, search, accountSlot, children }: BoardNavbarProps): React.ReactElement {
  const details = settings?.project_details ?? null;
  const glyph = details === null ? null : projectMarkGlyph(details.logo_props);
  const name = details?.name ?? "...";
  return (
    <header className="flex min-h-14 items-center gap-(--space-3) border-b border-(--border) px-(--space-4)">
      <span
        data-testid="space-navbar-mark"
        className="text-body flex size-7 items-center justify-center rounded-(--radius-control) bg-(--subtle)"
      >
        {glyph ?? (details === null ? null : name.slice(0, 1).toUpperCase())}
      </span>
      <span data-testid="space-navbar-name" className="text-emphasis truncate">
        {name}
      </span>
      <div className="ml-auto flex items-center gap-(--space-2)">
        {children}
        {accountSlot ?? (
          <a
            href={spaceSignInHref(anchor, search)}
            className="text-body rounded-(--radius-control) border border-(--border) px-(--space-3) py-(--space-1)"
          >
            Sign in
          </a>
        )}
      </div>
    </header>
  );
}

export function BoardLoading(): React.ReactElement {
  return (
    <div
      data-testid="space-board-loading"
      role="status"
      aria-label="Loading board"
      className="flex justify-center py-(--space-16)"
    >
      <Spinner size="large" label="Loading board" />
    </div>
  );
}

/** Public key shown beside a row, built from the project identifier in settings. */
export function issueKey(projectIdentifier: string, issue: Pick<SpaceBoardIssue, "sequence_id">): string {
  return `${projectIdentifier}-${issue.sequence_id}`;
}
