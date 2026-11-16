// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared run-status badge (NEWFRONT-545, RUN-022). Renders the status token
// as a readable label with a tone, and never throws on a value the product
// no longer models: the raw token shows on a neutral badge instead.

import { Badge } from "@pidash/kit";
import * as React from "react";

import { runStatusLabel, runStatusTone } from "./runStatus.js";

export interface RunStatusBadgeProps {
  /** Raw API status; unknown values render verbatim. */
  status: string;
}

export function RunStatusBadge({ status }: RunStatusBadgeProps): React.ReactElement {
  return <Badge tone={runStatusTone(status)}>{runStatusLabel(status)}</Badge>;
}
