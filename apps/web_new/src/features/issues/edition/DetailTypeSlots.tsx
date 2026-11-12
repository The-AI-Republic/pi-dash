// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Work-item-type surfaces on the issue detail (parity/specs/issues.md
// §20/§23, ISS-235): the badge beside the identifier, the extra
// property block under the sidebar and the type/property activity rows.
// The badge is cloud-only and edition-supplied. The property block and
// the type/property activity rows are out of parity scope (owner
// decision 2026-10-30, NEWFRONT-1038), so no edition registers their
// providers: both slots stay declared but inert. The community build
// has no flag, no provider and therefore renders nothing, leaving the
// bare copyable identifier the oracle pins.

import * as React from "react";

import { isEditionFlagOn, ISSUE_TYPES_FLAG } from "./flags.js";
import { getEditionIssuesProviders } from "./providers.js";

export interface DetailTypeSlotsProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  editable: boolean;
}

/** Whether any type surface can render in this build. */
export function hasDetailTypeSurfaces(): boolean {
  if (!isEditionFlagOn(ISSUE_TYPES_FLAG)) return false;
  const slot = getEditionIssuesProviders().issueTypeDetail;
  return (
    slot !== undefined && (slot.badge !== undefined || slot.properties !== undefined || slot.activity !== undefined)
  );
}

/** Badge or switcher beside the identifier; null without a provider. */
export function DetailTypeBadge({
  workspaceSlug,
  projectId,
  issueId,
  editable,
}: DetailTypeSlotsProps): React.ReactElement | null {
  const Badge = getEditionIssuesProviders().issueTypeDetail?.badge;
  if (!hasDetailTypeSurfaces() || Badge === undefined) return null;
  return (
    <span data-testid="detail-type-badge">
      <Badge workspaceSlug={workspaceSlug} projectId={projectId} issueId={issueId} editable={editable} />
    </span>
  );
}

/** Additional custom properties under the sidebar; null without a provider. */
export function DetailTypeProperties({
  workspaceSlug,
  projectId,
  issueId,
  editable,
}: DetailTypeSlotsProps): React.ReactElement | null {
  const Properties = getEditionIssuesProviders().issueTypeDetail?.properties;
  if (!hasDetailTypeSurfaces() || Properties === undefined) return null;
  return (
    <div data-testid="detail-type-properties">
      <Properties workspaceSlug={workspaceSlug} projectId={projectId} issueId={issueId} editable={editable} />
    </div>
  );
}

/** Type and property activity rows; null without a provider. */
export function DetailTypeActivity({
  workspaceSlug,
  projectId,
  issueId,
}: Omit<DetailTypeSlotsProps, "editable">): React.ReactElement | null {
  const Activity = getEditionIssuesProviders().issueTypeDetail?.activity;
  if (!hasDetailTypeSurfaces() || Activity === undefined) return null;
  return (
    <div data-testid="detail-type-activity">
      <Activity workspaceSlug={workspaceSlug} projectId={projectId} issueId={issueId} />
    </div>
  );
}
