// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared cycle date span (parity/specs/cycles.md §17, CYC-044). The
// list, detail and peek all render dates through this component so the
// range shape, the offset chip and the viewer tooltip never drift:
// later slices call it, they do not re-implement it. A cycle with
// neither date renders nothing.

import { Tooltip } from "@pidash/kit";
import * as React from "react";

import { cycleSpanModel } from "../lib/cycleDates.js";

export interface CycleDateSpanProps {
  startDate: string | null;
  endDate: string | null;
  /** IANA name of the project's timezone. */
  projectTimeZone: string;
  /** Viewer zone override; defaults to the browser zone. Tests inject. */
  viewerTimeZone?: string | undefined;
}

export function CycleDateSpan({
  startDate,
  endDate,
  projectTimeZone,
  viewerTimeZone,
}: CycleDateSpanProps): React.ReactElement | null {
  const model = cycleSpanModel(startDate, endDate, projectTimeZone, viewerTimeZone);
  if (model.text === null) return null;
  const body = (
    <span data-testid="cycle-date-span" className="inline-flex flex-wrap items-center gap-(--space-2)">
      <span>{model.text}</span>
      {model.chip === null ? null : (
        <span
          data-testid="cycle-date-offset"
          className="text-meta inline-flex items-center rounded-full border border-(--border) bg-(--subtle) px-(--space-2) py-px text-(--text-muted)"
        >
          {model.chip}
        </span>
      )}
    </span>
  );
  if (model.tooltip === null) return body;
  return <Tooltip content={model.tooltip}>{body}</Tooltip>;
}
