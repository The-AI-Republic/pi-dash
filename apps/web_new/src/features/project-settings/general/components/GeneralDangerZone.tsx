// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Danger zone on the General tab (parity/specs/project-settings-general.md
// §Danger zone; PSET-010–012). Archive and delete are admin-only. The
// archive row opens the shared confirm dialog in archive mode and both
// actions return to the project list; delete uses its own double-typed
// confirmation. Restore stays card-wired, as the oracle pins.

import { Button } from "@pidash/kit";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { DeleteProjectDialog, ProjectArchiveDialog } from "../../../projects/index.js";

export interface GeneralDangerZoneProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  /** Route change after archive/delete; tests inject it. */
  onNavigate?: ((href: string) => void) | undefined;
}

export function GeneralDangerZone({
  workspaceSlug,
  projectId,
  projectName,
  onNavigate,
}: GeneralDangerZoneProps): React.ReactElement {
  const [archiveOpen, setArchiveOpen] = React.useState(false);
  const [deleteOpen, setDeleteOpen] = React.useState(false);
  const routerNavigate = useNavigate();
  const navigate = React.useCallback(
    (href: string) => {
      // Client-side by default: a full reload would wipe the success
      // notice before the operator can read it (PSET-010).
      if (onNavigate !== undefined) onNavigate(href);
      else routerNavigate({ to: "/$ws/projects", params: { ws: workspaceSlug } });
    },
    [onNavigate, routerNavigate, workspaceSlug]
  );

  return (
    <section
      aria-label="Danger zone"
      data-testid="general-danger-zone"
      className="flex flex-col gap-(--space-4) rounded-(--radius-card) border border-(--danger) p-(--space-4)"
    >
      <div className="flex flex-col gap-(--space-1)">
        <h2 className="text-h4-medium text-(--danger)">Danger zone</h2>
        <p className="text-body text-(--text-muted)">
          These actions change who can reach this project. Only project admins see them.
        </p>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-(--space-3)">
        <div className="flex flex-col gap-(--space-1)">
          <span className="text-body font-medium text-(--text)">Archive project</span>
          <span className="text-meta text-(--text-muted)">
            Remove it from navigation and search while keeping its history reachable.
          </span>
        </div>
        <Button variant="danger" onClick={() => setArchiveOpen(true)}>
          Archive
        </Button>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-(--space-3)">
        <div className="flex flex-col gap-(--space-1)">
          <span className="text-body font-medium text-(--text)">Delete project</span>
          <span className="text-meta text-(--text-muted)">
            Permanently remove the project and everything inside it.
          </span>
        </div>
        <Button variant="danger" onClick={() => setDeleteOpen(true)}>
          Delete
        </Button>
      </div>

      <ProjectArchiveDialog
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        projectName={projectName}
        mode="archive"
        open={archiveOpen}
        onOpenChange={setArchiveOpen}
        onNavigate={navigate}
      />
      <DeleteProjectDialog
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        projectName={projectName}
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        onNavigate={navigate}
      />
    </section>
  );
}
