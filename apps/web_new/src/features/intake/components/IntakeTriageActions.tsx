// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Triage action area (parity/specs/intake.md INT-020–023, 025). The
// detail header's Accept and Decline buttons plus the five triage
// dialogs: the shared work-item form for accept (edit mode, locked to
// the request's project, with the accept write ahead of the save),
// and the intake-owned decline, snooze, duplicate and delete dialogs.
// The overflow menu entries live in the header; both fire through the
// one controls object the detail builds, so buttons and menu share a
// single dialog state.

import { Button } from "@pidash/kit";
import * as React from "react";

import { WorkItemCreateDialog } from "../../issues/index.js";
import type { TriageControls } from "../api/triage.js";
import { IntakeDeclineDialog } from "./IntakeDeclineDialog.js";
import { IntakeDeleteDialog } from "./IntakeDeleteDialog.js";
import { IntakeDuplicateDialog } from "./IntakeDuplicateDialog.js";
import { IntakeSnoozeDialog } from "./IntakeSnoozeDialog.js";

/** Accept button label. */
export const TRIAGE_ACCEPT_LABEL = "Accept";
/** Decline button label. */
export const TRIAGE_DECLINE_LABEL = "Decline";
/** Accept form's save label: the request joins the project. */
export const ACCEPT_SAVE_LABEL = "Add to project";

export interface IntakeTriageActionsProps {
  controls: TriageControls;
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  projectIdentifier: string;
  /** The acted request's work-item id, excluded from the picker. */
  issueId: string;
  /**
   * The viewer's project role, or null outside the project. The accept
   * form filters its offered projects by create permission, so the
   * locked row must carry the role or the form degrades to its
   * no-project notice (gate NEWFRONT-695, INT-020).
   */
  memberRole: number | null;
}

export function IntakeTriageActions({
  controls,
  workspaceSlug,
  projectId,
  projectName,
  projectIdentifier,
  issueId,
  memberRole,
}: IntakeTriageActionsProps): React.ReactElement {
  return (
    <>
      <Button
        variant="primary"
        size="small"
        data-testid="intake-triage-accept"
        onClick={() => controls.fireAction("accept")}
      >
        {TRIAGE_ACCEPT_LABEL}
      </Button>
      <Button
        variant="secondary"
        size="small"
        data-testid="intake-triage-decline"
        onClick={() => controls.fireAction("decline")}
      >
        {TRIAGE_DECLINE_LABEL}
      </Button>
      <IntakeTriageDialogs
        controls={controls}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        projectName={projectName}
        projectIdentifier={projectIdentifier}
        issueId={issueId}
        memberRole={memberRole}
      />
    </>
  );
}

export type IntakeTriageDialogsProps = IntakeTriageActionsProps;

/**
 * The five triage dialogs without the buttons (INT-030). The mobile
 * header fires the same controls object from its menu entries, so it
 * mounts only the dialogs; the buttons stay a desktop-header control.
 */
export function IntakeTriageDialogs({
  controls,
  workspaceSlug,
  projectId,
  projectName,
  projectIdentifier,
  issueId,
  memberRole,
}: IntakeTriageDialogsProps): React.ReactElement {
  return (
    <>
      <WorkItemCreateDialog
        open={controls.openDialog === "accept"}
        onOpenChange={(open) => {
          if (!open) controls.setOpenDialog(null);
        }}
        workspaceSlug={workspaceSlug}
        projects={[{ id: projectId, name: projectName, identifier: projectIdentifier, memberRole }]}
        initialProjectId={projectId}
        lockedProjectId={projectId}
        edit={controls.acceptTarget}
        editTitle={controls.acceptTitle}
        editSaveLabel={ACCEPT_SAVE_LABEL}
        onBeforeEditSubmit={controls.runAcceptWrite}
        onEditSaved={controls.handleAcceptSaved}
      />
      <IntakeDeclineDialog
        open={controls.openDialog === "decline"}
        pending={controls.declinePending}
        onCancel={() => controls.setOpenDialog(null)}
        onConfirm={controls.confirmDecline}
      />
      <IntakeSnoozeDialog
        open={controls.openDialog === "snooze"}
        pending={controls.snoozePending}
        initialDay={controls.snoozeInitialDay}
        onCancel={() => controls.setOpenDialog(null)}
        onConfirm={controls.confirmSnooze}
      />
      <IntakeDuplicateDialog
        open={controls.openDialog === "duplicate"}
        pending={controls.duplicatePending}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        currentIssueId={issueId}
        onCancel={() => controls.setOpenDialog(null)}
        onPick={controls.pickDuplicateTarget}
      />
      <IntakeDeleteDialog
        open={controls.openDialog === "delete"}
        pending={controls.deletePending}
        accepted={controls.deleteAccepted}
        onCancel={() => controls.setOpenDialog(null)}
        onConfirm={controls.confirmDelete}
      />
    </>
  );
}
