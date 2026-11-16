// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stored access line for the edit dialog (parity/specs/views.md §6,
// VIEW-015). `access` is serializer read-only on the view endpoints,
// so the edit dialog never edits it: the cloud build shows the
// record's stored value read-only, and the OSS edition (no access
// slot) renders nothing in its place.

import * as React from "react";

import { edition } from "../../../core/edition/index.js";
import { viewAccessLabel } from "../lib/projectViews.js";

export interface ViewAccessSummaryProps {
  /** The record's stored access value. */
  access: number;
}

/** The stored access as read-only text, or nothing on OSS. */
export function ViewAccessSummary({ access }: ViewAccessSummaryProps): React.ReactElement | null {
  if (edition.slots?.viewAccess === undefined) return null;
  return (
    <p data-testid="view-access-summary" className="text-body text-(--text-muted)">
      Shared with: {viewAccessLabel(access)}
    </p>
  );
}
