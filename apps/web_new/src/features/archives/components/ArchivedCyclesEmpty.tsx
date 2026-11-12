// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-cycles zero state (parity/specs/archives.md §4.4,
// ARCH-017). An illustrated card explaining that archiving completed
// cycles is what fills the screen. The pinned heading and its
// supporting line are siblings under one parent: the cycles driver's
// archived-detail reader takes the card's text minus the heading, so
// the copy must keep the heading exact and the line must keep naming
// archiving.

import { emptyStateImagePath } from "@pidash/kit";
import * as React from "react";

/** Pinned heading; the parity driver matches the archived card on it. */
export const ARCHIVED_CYCLES_EMPTY_HEADING = "No archived cycles yet";

export function ArchivedCyclesEmpty(): React.ReactElement {
  return (
    <div
      data-testid="archived-cycles-empty"
      className="flex flex-col items-center gap-(--space-4) px-(--space-4) py-(--space-12) text-center"
    >
      <img
        src={emptyStateImagePath({ category: "general", type: "empty" })}
        alt=""
        aria-hidden="true"
        draggable={false}
        className="h-40 w-40"
      />
      <h3 className="text-h3 text-(--text)">{ARCHIVED_CYCLES_EMPTY_HEADING}</h3>
      <p className="text-body text-(--text-muted)">Completed cycles you archive are kept here.</p>
    </div>
  );
}
