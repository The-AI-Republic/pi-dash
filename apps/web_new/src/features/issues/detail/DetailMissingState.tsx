// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail missing state (parity/specs/issues.md §13, ISS-143/144). One
// generic state for unknown keys, deleted items, and refused access
// alike: the route never distinguishes them. The caller passes where
// out (workspace home for browse keys, the project list for legacy
// short-links) as the action.

import * as React from "react";

export interface DetailMissingStateProps {
  issueKey: string;
  /** Overrides the stock explanation (short-links also cover archived). */
  body?: string | undefined;
  children: React.ReactNode;
}

export function DetailMissingState({ issueKey, body, children }: DetailMissingStateProps): React.ReactElement {
  return (
    <section aria-label="Work item detail" className="flex flex-col items-start gap-(--space-4) p-(--space-12)">
      <h1 className="text-h2 text-(--text)">No work item matches this address</h1>
      <p className="text-body text-(--text-muted)">
        {body ?? `Nothing is filed under ${issueKey}. It may have been deleted, or the address is wrong.`}
      </p>
      {children}
    </section>
  );
}
