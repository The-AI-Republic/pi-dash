// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Finished-cycle read-only notice (parity/specs/cycles.md §8,
// CYC-023). The detail page mounts this for finished cycles: it
// explains the cycle has finished and is not editable. Where the
// transfer prompt is offered, it composes below the notice through
// children (slice H).

import { Lock } from "lucide-react";
import * as React from "react";

export interface FinishedCycleNoticeProps {
  /** The transfer prompt, where the detail offers one. */
  children?: React.ReactNode;
}

export function FinishedCycleNotice({ children }: FinishedCycleNoticeProps): React.ReactElement {
  return (
    <section aria-label="Finished cycle" className="flex flex-col gap-(--space-2)">
      <p className="text-body flex items-center gap-(--space-2) text-(--text-muted)">
        <Lock size={16} strokeWidth={1.5} aria-hidden="true" />
        This cycle has finished and is not editable.
      </p>
      {children}
    </section>
  );
}
