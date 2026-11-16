// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Attribution rows (parity/specs/pages.md §6.9, §13.3, PAGE-050). Who
// edited the page last, with how long ago, and who created it, with the
// calendar date. Both names lead to the person's profile. A departed
// member keeps the row and its address but shows a neutral label instead
// of a name, so nothing breaks when the directory no longer lists them.

import { Avatar } from "@pidash/kit";
import { Link } from "@tanstack/react-router";
import * as React from "react";

import { PAGE_DEACTIVATED_USER, type PageActorRow } from "../lib/pagePane.js";

export const PAGE_EDITED_BY_LABEL = "Edited by";
export const PAGE_CREATED_BY_LABEL = "Created by";

export interface PageActorsInfoProps {
  rows: readonly PageActorRow[];
}

function roleLabel(role: PageActorRow["role"]): string {
  return role === "edited" ? PAGE_EDITED_BY_LABEL : PAGE_CREATED_BY_LABEL;
}

function ActorIdentity({ row }: { row: PageActorRow }): React.ReactElement {
  const name = (
    <span className="text-body min-w-0 truncate text-(--text)">
      {row.deactivated ? PAGE_DEACTIVATED_USER : row.name}
    </span>
  );
  return (
    <div className="flex min-w-0 items-center gap-(--space-2)">
      <span className="text-caption shrink-0 text-(--text-muted)">{row.timeText}</span>
      {/* The avatar sits in its own box: the row's direct child spans stay
          the time, so a reader can find it without knowing the artwork. */}
      <div className="shrink-0">
        <Avatar
          name={row.deactivated ? PAGE_DEACTIVATED_USER : row.name}
          {...(row.deactivated || row.avatarUrl === null ? {} : { src: row.avatarUrl })}
          size="small"
        />
      </div>
      {row.href === null ? (
        name
      ) : (
        <Link
          to={row.href}
          className="text-body min-w-0 truncate rounded-(--radius-control) text-(--text) hover:underline"
        >
          {row.deactivated ? PAGE_DEACTIVATED_USER : row.name}
        </Link>
      )}
    </div>
  );
}

export function PageActorsInfo({ rows }: PageActorsInfoProps): React.ReactElement {
  return (
    <div data-testid="page-actors" className="flex flex-col gap-(--space-3)">
      {rows.map((row) => (
        <div key={row.role} className="flex flex-col gap-(--space-1)">
          <span className="text-caption text-(--text-muted)">{roleLabel(row.role)}</span>
          <ActorIdentity row={row} />
        </div>
      ))}
    </div>
  );
}
