// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics section guidance panel (parity/specs/analytics.md §§6,
// 9–10, ANL-006). The compact empty view chart sections show when
// their read succeeds with nothing to plot. Later slices reuse
// this panel with their own copy; the parity driver reads it
// through analyticsSectionGuidance.
import { EmptyState } from "@pidash/kit";
import * as React from "react";

export interface AnalyticsSectionGuidanceProps {
  title: string;
  description: string;
}

export function AnalyticsSectionGuidance({ title, description }: AnalyticsSectionGuidanceProps): React.ReactElement {
  return (
    <div data-testid="analytics-section-guidance">
      <EmptyState variant="section" title={title} description={description} />
    </div>
  );
}
