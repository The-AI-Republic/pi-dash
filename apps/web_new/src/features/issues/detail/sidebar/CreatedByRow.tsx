// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Created-by row (parity/specs/issues.md §13, ISS-152). Read-only for
// every viewer: the creator's avatar plus name, with no control and no
// editing path. Intake-bot creators present under the product name.
// Until the workspace directory answers, the row holds its place with a
// neutral placeholder instead of claiming the creator is unknown.

import { Skeleton, Avatar } from "@pidash/kit";
import * as React from "react";

import type { CreatorDisplay } from "./creator.js";
import { SIDEBAR_ROW_CLASS, SIDEBAR_ROW_LABEL_CLASS } from "./SidebarRow.js";

export interface CreatedByRowProps {
  creator: CreatorDisplay | null;
  /** True while the directory read that names the creator is in flight. */
  pending?: boolean;
}

export function CreatedByRow({ creator, pending = false }: CreatedByRowProps): React.ReactElement | null {
  if (creator === null) return null;
  const labelId = React.useId();
  return (
    <div className={SIDEBAR_ROW_CLASS} role="group" aria-labelledby={labelId}>
      <span id={labelId} className={SIDEBAR_ROW_LABEL_CLASS}>
        Created by
      </span>
      <div className="flex min-w-0 flex-1 items-center gap-(--space-2)">
        {pending ? (
          <>
            <Skeleton width="20px" height="20px" round />
            <Skeleton width="96px" height="12px" />
          </>
        ) : (
          <>
            <Avatar
              name={creator.name}
              {...(creator.avatarUrl === null ? {} : { src: creator.avatarUrl })}
              size="small"
            />
            <span className="text-meta truncate text-(--text)">{creator.name}</span>
          </>
        )}
      </div>
    </div>
  );
}
