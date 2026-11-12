// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake duplicate-detection indicator (parity/specs/intake.md INT-018).
// The shared duplicate panel behind the intake testid the parity driver
// reads, mounted while composing (the create dialog) and while viewing
// (the detail). Renders nothing without matches, so the OSS build —
// where the seam never searches — shows no trace of the feature.

import type { IssueSearchHit } from "@pidash/api-client";
import * as React from "react";

import { DuplicatePanel } from "../../issues/index.js";

export interface IntakeDuplicateIndicatorProps {
  /** Near-duplicate work items for the draft or the viewed request. */
  matches: readonly IssueSearchHit[];
}

export function IntakeDuplicateIndicator({ matches }: IntakeDuplicateIndicatorProps): React.ReactElement | null {
  if (matches.length === 0) return null;
  return (
    <div data-testid="intake-duplicate-indicator">
      <DuplicatePanel matches={matches} />
    </div>
  );
}
