// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Content-pane placeholder (parity/specs/intake.md INT-001): no request
// is selected, so the pane says to pick one rather than showing an empty
// detail. Purely presentational; the detail itself is slice D.

import { Inbox } from "lucide-react";
import * as React from "react";

export function IntakePlaceholder(): React.ReactElement {
  return (
    <div
      data-testid="intake-placeholder"
      className="flex h-full flex-col items-center justify-center gap-(--space-2) p-(--space-8) text-center"
    >
      <Inbox size={24} strokeWidth={1.5} aria-hidden="true" className="text-(--text-muted)" />
      <p className="text-body text-(--text)">Pick a request to see its details.</p>
      <p className="text-meta text-(--text-muted)">Choose a request from the list on the left.</p>
    </div>
  );
}
