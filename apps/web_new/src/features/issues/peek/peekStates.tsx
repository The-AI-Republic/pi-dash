// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared peek status panels (parity/specs/issues.md §14, ISS-170/171).
// The loading skeleton mimics the loaded body — header, title,
// description, sub-issues, attachments, property grid — and the error
// panel collapses every failure into one message. Both leave only the
// Close button interactive: no title, no actions, no retry, no
// redirect. The resolving wrapper and every nesting level share them.

import * as React from "react";

import { Skeleton } from "@pidash/kit";
import { PeekPanel } from "../../../shared/shell/PeekPanel.js";
import type { PeekMode } from "./PeekModeToggle.js";

/** Loading panel, with its Close control (ISS-170). */
export const PEEK_LOADING_LABEL = "Loading work item";

/** Generic failure message; every fetch failure collapses into it (ISS-171). */
export const PEEK_MISSING_TITLE = "This work item isn't available";
export const PEEK_MISSING_BODY = "It may have been deleted or archived, or you may not have access to it.";

export interface PeekStatusPanelProps {
  mode: PeekMode;
  onClose: () => void;
  /** Caller-known Escape suppression (a nested peek above this one). */
  suppressEscape?: boolean | undefined;
  /** Embedded hosts disable the outside-click close. */
  embedded?: boolean | undefined;
  /** Test handle for the panel; nested panels take their own. */
  testId?: string | undefined;
  /** A nested panel paints above its parent. */
  stacked?: boolean | undefined;
}

function StatusShell({
  mode,
  onClose,
  suppressEscape = false,
  embedded = false,
  testId,
  stacked = false,
  children,
}: PeekStatusPanelProps & { children: React.ReactNode }): React.ReactElement {
  return (
    <PeekPanel
      layout={mode}
      stacked={stacked}
      hideTitle
      title=""
      titleLabel="Work item title"
      onClose={onClose}
      suppressEscape={embedded || suppressEscape}
      {...(embedded ? {} : { onOutsideClick: onClose })}
      {...(testId === undefined ? {} : { testId })}
    >
      {children}
    </PeekPanel>
  );
}

export function PeekLoadingPanel(props: PeekStatusPanelProps): React.ReactElement {
  return (
    <StatusShell {...props}>
      <div role="status" aria-label={PEEK_LOADING_LABEL} className="flex min-w-0 flex-1 flex-col gap-(--space-4)">
        <Skeleton height="22px" width="45%" />
        <Skeleton height="34px" width="70%" />
        <Skeleton height="14px" />
        <Skeleton height="14px" />
        <Skeleton height="14px" width="55%" />
        <Skeleton height="26px" width="40%" />
        <Skeleton height="56px" />
        <Skeleton height="26px" />
        <Skeleton height="26px" />
        <Skeleton height="26px" width="80%" />
      </div>
    </StatusShell>
  );
}

export function PeekErrorPanel(props: PeekStatusPanelProps): React.ReactElement {
  return (
    <StatusShell {...props}>
      <div className="flex min-w-0 flex-1 flex-col items-start gap-(--space-2)">
        <h2 className="text-h3 text-(--text)">{PEEK_MISSING_TITLE}</h2>
        <p className="text-body text-(--text-muted)">{PEEK_MISSING_BODY}</p>
      </div>
    </StatusShell>
  );
}
