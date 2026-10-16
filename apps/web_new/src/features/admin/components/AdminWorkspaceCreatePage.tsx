// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Create-workspace page (ADM-043–045, from parity/specs/admin.md
// §5.7–§5.9): the heading, the back shortcut to the list, and the
// create form. Mounted by the workspace create route inside the
// dashboard layout.
import { Link } from "@tanstack/react-router";
import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/index.js";
import { WorkspaceCreateForm } from "./WorkspaceCreateForm.js";

/** Document title for the create-workspace route. */
export const WORKSPACE_CREATE_PAGE_TITLE = "Create workspace — Pi Dash";

/** Heading for the create-workspace page. */
export const WORKSPACE_CREATE_HEADING = "New workspace";

/** Shortcut target back to the workspace list. */
export const WORKSPACE_CREATE_BACK_PATH = "/god-mode/workspace";

type RouterLinkProps = Parameters<typeof Link>[0];

// The cast carries the literal through the same way CrossAppLink
// reaches out-of-tree paths (see the workspace list's create link).
const backLinkProps = { to: WORKSPACE_CREATE_BACK_PATH } as RouterLinkProps;

export function AdminWorkspaceCreatePage(): React.ReactElement {
  useDocumentHead({ title: WORKSPACE_CREATE_PAGE_TITLE, robots: "noindex, nofollow" });
  return (
    <div
      data-testid="admin-workspace-create-page"
      className="mx-auto flex w-full max-w-3xl flex-col gap-(--space-6) px-(--space-6) py-(--space-8)"
    >
      <div className="flex flex-col gap-(--space-2)">
        <h1 data-testid="admin-workspace-create-heading" className="text-h2 text-(--text)">
          {WORKSPACE_CREATE_HEADING}
        </h1>
        <p data-testid="admin-workspace-create-helper" className="text-body text-(--text-muted)">
          Name the workspace and choose its web address. You become its owner automatically.
        </p>
        <Link
          {...backLinkProps}
          data-testid="admin-workspace-create-back"
          className="text-body w-fit font-medium text-(--accent) hover:underline"
        >
          Back to workspaces
        </Link>
      </div>
      <WorkspaceCreateForm />
    </div>
  );
}
