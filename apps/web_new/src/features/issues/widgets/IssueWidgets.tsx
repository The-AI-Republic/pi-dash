// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Standalone relations + links stack (parity/specs/issues.md §15,
// ISS-179–186). The detail route mounts both through the widget root's
// section slots; this composition keeps the pair renderable on its own
// (tests, and any surface that has no action row). Each section renders
// only with content; the add entries live in the widget action row.

import * as React from "react";

import { LinksWidget } from "./LinksWidget.js";
import { RelationsWidget } from "./RelationsWidget.js";
import { LinkActionButton, RelationActionMenu, WidgetActionsProvider } from "./widgetActions.js";

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
      <WidgetActionsProvider>
        <div className="flex flex-wrap items-center gap-(--space-2)">
          <RelationActionMenu canEdit={canEdit} />
          <LinkActionButton canEdit={canEdit} />
        </div>
        <RelationsWidget
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          issueId={issueId}
          projectIdentifier={projectIdentifier}
          canEdit={canEdit}
        />
        <LinksWidget workspaceSlug={workspaceSlug} projectId={projectId} issueId={issueId} canEdit={canEdit} />
      </WidgetActionsProvider>
    </section>
  );
}
