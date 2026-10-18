// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy boundary for the issues rich filters (parity/specs/issues.md §9,
// ISS-095–099). The whole builder — header entry, condition row, value
// editors and picker reads — rides this chunk so the route chunk stays
// free of the filter graph. The render prop hands the mounted control and
// row to the layout root; both come from one builder instance.

import * as React from "react";

import { useRichFilterBuilder, type RichFilterBuilderParts } from "./RichFilterBuilder.js";

export interface IssueFiltersProps {
  workspaceSlug: string;
  projectId: string;
  hideCycle: boolean;
  hideModule: boolean;
  viewsEnabled?: boolean | undefined;
  children: (parts: RichFilterBuilderParts) => React.ReactNode;
}

export default function IssueFilters({
  workspaceSlug,
  projectId,
  hideCycle,
  hideModule,
  viewsEnabled,
  children,
}: IssueFiltersProps): React.ReactElement {
  const parts = useRichFilterBuilder({
    workspaceSlug,
    projectId,
    entity: { scope: "project", projectId },
    hideCycle,
    hideModule,
    viewsEnabled,
  });
  return <React.Fragment>{children(parts)}</React.Fragment>;
}
