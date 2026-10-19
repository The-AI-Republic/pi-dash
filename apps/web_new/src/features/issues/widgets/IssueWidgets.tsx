// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail widget stack (parity/specs/issues.md §15, ISS-173–186). Placed
// under the description; the relations and links sections each render
// only with content (or with an add control for editors). Later slices
// add sub-work items and attachments around them.

import * as React from "react";

import { LinksWidget } from "./LinksWidget.js";
import { RelationsWidget } from "./RelationsWidget.js";

export interface IssueWidgetsProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Current project identifier for row badges. */
  projectIdentifier: string;
  canEdit: boolean;
}

export function IssueWidgets({
  workspaceSlug,
  projectId,
  issueId,
  projectIdentifier,
  canEdit,
}: IssueWidgetsProps): React.ReactElement {
  return (
    <section aria-label="Work item details" className="mt-(--space-6) flex flex-col gap-(--space-2)">
      <RelationsWidget
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        issueId={issueId}
        projectIdentifier={projectIdentifier}
        canEdit={canEdit}
      />
      <LinksWidget workspaceSlug={workspaceSlug} projectId={projectId} issueId={issueId} canEdit={canEdit} />
    </section>
  );
}
