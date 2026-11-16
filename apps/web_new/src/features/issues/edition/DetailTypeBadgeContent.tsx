// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud type badge body (parity/specs/issues.md §20/§23, ISS-235): names
// the issue's work-item type beside the identifier. The detail read
// carries the type id and the project type list names it; an untyped
// item (or a type the list no longer offers) shows nothing. Loaded as
// its own chunk behind the provider, so the community bundle never
// carries the type reads.
import { Badge } from "@pidash/kit";
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { issueDetailQueryOptions } from "../api/queries.js";
import { editionOptionsKey } from "./EditionFilterSections.js";
import { isEditionFlagOn, ISSUE_TYPES_FLAG } from "./flags.js";
import { getEditionIssuesProviders, type WorkItemTypeOption } from "./providers.js";

export interface DetailTypeBadgeInput {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  editable: boolean;
}

/** The type name for the badge, or null when the badge stays hidden. */
export function selectTypeBadgeName(
  typeId: string | null | undefined,
  options: readonly WorkItemTypeOption[] | undefined
): string | null {
  if (typeId === null || typeId === undefined) return null;
  return options?.find((option) => option.id === typeId)?.name ?? null;
}

export function DetailTypeBadgeContent(input: DetailTypeBadgeInput): React.ReactElement | null {
  const source = getEditionIssuesProviders().workItemTypes;
  // The detail screen already reads this row, so the badge reuses the
  // cached read instead of fetching it again.
  const detail = useQuery(issueDetailQueryOptions(getAppClient(), input.workspaceSlug, input.projectId, input.issueId));
  const types = useQuery({
    queryKey: editionOptionsKey(input.workspaceSlug, input.projectId, "types"),
    // The read stays disabled without a source, so it always has one here.
    queryFn: () => source!({ workspaceSlug: input.workspaceSlug, projectId: input.projectId }),
    enabled: isEditionFlagOn(ISSUE_TYPES_FLAG) && source !== undefined,
  });
  const name = selectTypeBadgeName(detail.data?.type_id ?? null, types.data);
  if (name === null) return null;
  // The slot wrapper's own testid marks the mount even when the badge
  // hides, so the name carries a second hook for the cloud legs.
  return (
    <span data-testid="detail-type-badge-name">
      <Badge tone="neutral">{name}</Badge>
    </span>
  );
}
