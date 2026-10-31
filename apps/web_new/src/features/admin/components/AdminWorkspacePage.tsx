// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace settings page (ADM-037–042, from parity/specs/admin.md
// §5): the creation-restriction toggle over the instance workspace
// list. Mounted by the _dashboard workspace route inside the dashboard
// layout; the create form (NEWFRONT-513) lives on its own route.
import * as React from "react";

import { WorkspaceCreationToggle } from "./WorkspaceCreationToggle.js";
import { WorkspaceList } from "./WorkspaceList.js";

export function AdminWorkspacePage(): React.ReactElement {
  return (
    <div
      data-testid="admin-workspace-page"
      className="mx-auto flex w-full max-w-3xl flex-col gap-(--space-8) px-(--space-6) py-(--space-8)"
    >
      <WorkspaceCreationToggle />
      <WorkspaceList />
    </div>
  );
}
