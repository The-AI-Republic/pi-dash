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

import { spaceSignInHref, type SpaceBoardSearch } from "../lib/address.js";
import { spaceMessage } from "../lib/spaceMessages.js";
import { SpaceProjectMark } from "./SpaceProjectMark.js";

export interface BoardNavbarProps {
  settings: SpaceBoardSettings | undefined;
  anchor: string;
  search: SpaceBoardSearch;
  /** Slice D replaces the right-hand slot; the base keeps a sign-in control. */
  accountSlot?: React.ReactNode;
  children?: React.ReactNode;
}

/**
 * Board navbar (SPC-001 placeholder chrome, SPC-010 identity). The mark
 * resolves through the slice-D mark branches; without settings the bar
 * holds the generic mark and the placeholder name with no content flash.
 * Long names truncate with the full name on hover.
 */
export function BoardNavbar({ settings, anchor, search, accountSlot, children }: BoardNavbarProps): React.ReactElement {
  const details = settings?.project_details ?? null;
  const name = details?.name ?? "...";
  return (
    <header className="flex min-h-14 items-center gap-(--space-3) border-b border-(--border) px-(--space-4)">
      <SpaceProjectMark details={details} />
      <span data-testid="space-navbar-name" title={name} className="text-emphasis truncate">
        {name}
      </span>
      <div className="ml-auto flex items-center gap-(--space-2)">
        {children}
        {accountSlot ?? (
          <a
            href={spaceSignInHref(anchor, search)}
            className="text-body rounded-(--radius-control) border border-(--border) px-(--space-3) py-(--space-1)"
          >
            {spaceMessage("account.signIn")}
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
