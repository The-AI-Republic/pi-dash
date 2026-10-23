// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Triage writes (parity/specs/intake.md INT-020–023, 025, Data layer).
// Accept, decline, snooze, un-snooze and mark-as-duplicate PATCH the
// top-level intake fields with no optimistic paint; delete removes the
// entry. Every move invalidates the tab lists (which carry the Open
// badge) and the detail, announces itself with a toast, and hands the
// neighbour advance to the screen. The duplicate-target search reuses
// the Issues project search contract. `useTriageControls` below owns
// the dialog state, the member-refused notices and the accept hand-off
// to the shared work-item form; the detail calls it once and hands the
// controls to both the header menu and the action buttons, so the two
// stay on one state.

import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  INTAKE_STATUS,
  deleteIntakeIssue,
  searchIssues,
  updateIntakeIssue,
  type ApiClient,
  type IntakeIssueDetailItem,
  type IntakeStatus,
  type IssueSearchHit,
} from "@pidash/api-client";
import type { MenuEntry } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { issueKeys, saveFailureDetail, type WorkItemEditTarget } from "../../issues/index.js";
import { intakeKeys } from "./keys.js";
import { INTAKE_QUERY_POLICY } from "./queries.js";
import {
  TRIAGE_REFUSED_TITLE,
  buildTriageMenuEntries,
  scheduleTriageFocus,
  snoozeDayFromTimestamp,
  snoozeTimestampForDay,
  triageRefusedMessage,
  type TriageRefusedAction,
} from "../lib/triage.js";
import { selectIntakeTriageVisibility, type IntakeTriageVisibility } from "../lib/permissions.js";

/**
 * Revalidate everything a triage move touches: every tab list under the
 * project and the acted request's detail. A deleted row's detail has
 * nothing to re-read, but invalidating (rather than removing) it keeps
 * one path: the query is inactive once the screen advances, so no
 * doomed refetch fires, and a later revisit redirects through the
 * missing-id fallback. An accept that changed the description also
 * revalidates the shared versions list.
 */
export async function invalidateTriageCaches(
  queryClient: QueryClient,
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  options: { descriptionChanged?: boolean } = {}
): Promise<void> {
  const settled = [
    queryClient.invalidateQueries({ queryKey: intakeKeys.lists(workspaceSlug, projectId) }),
    queryClient.invalidateQueries({ queryKey: intakeKeys.detail(workspaceSlug, projectId, issueId) }),
  ];
  if (options.descriptionChanged === true) {
    settled.push(
      queryClient.invalidateQueries({
        queryKey: issueKeys.descriptionVersions(workspaceSlug, projectId, issueId),
      })
    );
  }
  await Promise.all(settled);
}

/** A triage status move: the intake fields with the status required. */
export interface TriageStatusInput {
  status: IntakeStatus;
  snoozed_till?: string | null;
  duplicate_to?: string | null;
}

export interface TriageMoveScope {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Toast title on success. */
  successTitle: string;
  /** Toast title when the move is refused. */
  failureTitle: string;
}

/**
 * One non-optimistic triage PATCH with notice and revalidate
 * (INT-020–023). The caller advances or stays in its own `onSuccess`.
 */
export function useTriageStatusMove(scope: TriageMoveScope) {
  const client = getAppClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: TriageStatusInput) =>
      updateIntakeIssue(client, scope.workspaceSlug, scope.projectId, scope.issueId, input),
    onError: (error: Error) => {
      notifyError(scope.failureTitle, saveFailureDetail(error));
    },
    onSuccess: () => {
      notifySuccess(scope.successTitle);
      void invalidateTriageCaches(queryClient, scope.workspaceSlug, scope.projectId, scope.issueId);
    },
  });
}

export interface DeleteIntakeRequestScope {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
}

/** Notice titles for the delete move (INT-025). */
export const DELETE_SUCCESS_TITLE = "Request deleted";
export const DELETE_FAILED_TITLE = "Couldn't delete the request";

/** Delete a request (INT-025) with notice and revalidate. */
export function useDeleteIntakeRequest(scope: DeleteIntakeRequestScope) {
  const client = getAppClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => deleteIntakeIssue(client, scope.workspaceSlug, scope.projectId, scope.issueId),
    onError: (error: Error) => {
      notifyError(DELETE_FAILED_TITLE, saveFailureDetail(error));
    },
    onSuccess: () => {
      notifySuccess(DELETE_SUCCESS_TITLE);
      void invalidateTriageCaches(queryClient, scope.workspaceSlug, scope.projectId, scope.issueId);
    },
  });
}

/** Notice titles for the status moves. */
export const DECLINE_SUCCESS_TITLE = "Request declined";
export const DECLINE_FAILED_TITLE = "Couldn't decline the request";
export const SNOOZE_SUCCESS_TITLE = "Request snoozed";
export const SNOOZE_FAILED_TITLE = "Couldn't snooze the request";
export const UNSNOOZE_SUCCESS_TITLE = "Snooze removed";
export const UNSNOOZE_FAILED_TITLE = "Couldn't remove the snooze";
export const DUPLICATE_SUCCESS_TITLE = "Marked as duplicate";
export const DUPLICATE_FAILED_TITLE = "Couldn't mark as duplicate";
/** Notice title when the accept write fails before the form saves. */
export const ACCEPT_FAILED_TITLE = "Couldn't accept the request";

/** Query options for the duplicate-target picker search. */
export function duplicateTargetSearchQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  term: string
) {
  return {
    queryKey: intakeKeys.search(workspaceSlug, projectId, term),
    queryFn: () =>
      searchIssues(
        client,
        workspaceSlug,
        projectId,
        term === "" ? { workspaceSearch: false } : { search: term, workspaceSearch: false }
      ),
    ...INTAKE_QUERY_POLICY,
  };
}

/**
 * Project work items for the duplicate picker: recent rows on an empty
 * search, matches once the viewer types. Project-scoped, never the
 * whole workspace.
 */
export function useDuplicateTargetSearch(workspaceSlug: string, projectId: string, term: string, enabled: boolean) {
  return useQuery({ ...duplicateTargetSearchQueryOptions(getAppClient(), workspaceSlug, projectId, term), enabled });
}

/** Drop the acted request's own work item from the picker results. */
export function excludeCurrentTarget(hits: readonly IssueSearchHit[], currentId: string): IssueSearchHit[] {
  return hits.filter((hit) => hit.id !== currentId);
}

/** The triage dialogs; at most one is open. */
export type TriageDialogName = "accept" | "decline" | "snooze" | "duplicate" | "delete";

/** A triage entry fired from the buttons or the overflow menu. */
export type TriageActionName = "accept" | "decline" | "snooze" | "unsnooze" | "duplicate" | "delete";

export interface TriageControlsScope {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  projectIdentifier: string;
  issueId: string;
  /** Route key shown to people, e.g. "WEB-123". */
  issueKey: string;
  /** The request's work item, seeding the accept form's snapshot. */
  issue: IntakeIssueDetailItem;
  status: number;
  snoozedTill: string | null;
  isAdmin: boolean;
  isAuthorized: boolean;
  isCreator: boolean;
  /**
   * Move the address past the acted request; true when the tab emptied.
   * The screen computes the neighbour from its cached rows.
   */
  onAdvance: (removedId: string) => boolean;
}

export interface TriageControls {
  visibility: IntakeTriageVisibility;
  /** Menu entries ahead of the header's copy-link item. */
  leadEntries: MenuEntry[];
  /** Menu entries behind it. */
  trailEntries: MenuEntry[];
  openDialog: TriageDialogName | null;
  setOpenDialog: (name: TriageDialogName | null) => void;
  fireAction: (action: TriageActionName) => void;
  /** The accept form's edit target on the request's work item. */
  acceptTarget: WorkItemEditTarget;
  /** Accept heading naming the request being added. */
  acceptTitle: string;
  /**
   * The accept write (INT-020 ordering): `status=1` before the form's
   * work-item update. Reports its own failure and rethrows, so the
   * shared form aborts the submit and stays open with the error.
   */
  runAcceptWrite: () => Promise<void>;
  /**
   * The accept save hand-back: invalidate, advance past the accepted
   * request, and land focus. A changed description also revalidates
   * the versions list.
   */
  handleAcceptSaved: (changedFields: readonly string[]) => void;
  /** Accept dialog closed: re-read when the write committed but the form save failed. */
  handleAcceptClosed: () => void;
  declinePending: boolean;
  confirmDecline: () => void;
  snoozePending: boolean;
  /** The request's current snooze day for the picker's start, if any. */
  snoozeInitialDay: string | null;
  confirmSnooze: (isoDay: string) => void;
  duplicatePending: boolean;
  pickDuplicateTarget: (targetId: string) => void;
  deletePending: boolean;
  /** The request is accepted, so delete keeps its work item. */
  deleteAccepted: boolean;
  confirmDelete: () => void;
}

function refusedActionFor(action: TriageActionName): TriageRefusedAction | null {
  switch (action) {
    case "accept":
      return "accept";
    case "decline":
      return "decline";
    case "snooze":
    case "unsnooze":
      return "snooze";
    case "duplicate":
      return "duplicate";
    case "delete":
      // Delete has no client-side refusal: the entry itself shows only
      // for an admin or the creator, and the server refuses the rest.
      return null;
  }
}

/**
 * Triage orchestration for one open request (INT-020–023, 025). The
 * detail calls this once and hands the controls to the header menu
 * and the action buttons, so both fire through one dialog state.
 */
export function useTriageControls(scope: TriageControlsScope): TriageControls {
  const queryClient = useQueryClient();
  const [openDialog, setOpenDialog] = React.useState<TriageDialogName | null>(null);
  const acceptCommitted = React.useRef(false);
  const acceptSaved = React.useRef(false);

  const visibility = selectIntakeTriageVisibility({
    isAdmin: scope.isAdmin,
    isAuthorized: scope.isAuthorized,
    isCreator: scope.isCreator,
    status: scope.status,
  });

  const moveScope = {
    workspaceSlug: scope.workspaceSlug,
    projectId: scope.projectId,
    issueId: scope.issueId,
  };
  const declineMove = useTriageStatusMove({
    ...moveScope,
    successTitle: DECLINE_SUCCESS_TITLE,
    failureTitle: DECLINE_FAILED_TITLE,
  });
  const snoozeMove = useTriageStatusMove({
    ...moveScope,
    successTitle: SNOOZE_SUCCESS_TITLE,
    failureTitle: SNOOZE_FAILED_TITLE,
  });
  const unsnoozeMove = useTriageStatusMove({
    ...moveScope,
    successTitle: UNSNOOZE_SUCCESS_TITLE,
    failureTitle: UNSNOOZE_FAILED_TITLE,
  });
  const duplicateMove = useTriageStatusMove({
    ...moveScope,
    successTitle: DUPLICATE_SUCCESS_TITLE,
    failureTitle: DUPLICATE_FAILED_TITLE,
  });
  const deleteMove = useDeleteIntakeRequest(moveScope);

  const advanceAfter = React.useCallback(
    (removedId: string) => {
      const emptied = scope.onAdvance(removedId);
      scheduleTriageFocus(emptied);
    },
    [scope]
  );

  const fireAction = React.useCallback(
    (action: TriageActionName) => {
      const refused = refusedActionFor(action);
      if (refused !== null && !scope.isAdmin) {
        notifyError(TRIAGE_REFUSED_TITLE, triageRefusedMessage(refused));
        return;
      }
      if (action === "unsnooze") {
        unsnoozeMove.mutate(
          { status: INTAKE_STATUS.pending, snoozed_till: null },
          {
            onSuccess: () => {
              advanceAfter(scope.issueId);
            },
          }
        );
        return;
      }
      setOpenDialog(action);
    },
    [advanceAfter, scope.isAdmin, scope.issueId, unsnoozeMove]
  );

  const leadEntries = React.useMemo(
    () =>
      buildTriageMenuEntries({
        snoozed: scope.status === INTAKE_STATUS.snoozed,
        showDelete: visibility.showDelete,
        onSnooze: () => fireAction("snooze"),
        onUnsnooze: () => fireAction("unsnooze"),
        onDuplicate: () => fireAction("duplicate"),
        onDelete: () => fireAction("delete"),
      }),
    [fireAction, scope.status, visibility.showDelete]
  );

  const runAcceptWrite = React.useCallback(async () => {
    try {
      await updateIntakeIssue(getAppClient(), scope.workspaceSlug, scope.projectId, scope.issueId, {
        status: INTAKE_STATUS.accepted,
      });
      acceptCommitted.current = true;
    } catch (error) {
      notifyError(ACCEPT_FAILED_TITLE, saveFailureDetail(error));
      throw error;
    }
  }, [scope.projectId, scope.workspaceSlug, scope.issueId]);

  const handleAcceptSaved = React.useCallback(
    (changedFields: readonly string[]) => {
      acceptSaved.current = true;
      setOpenDialog(null);
      void invalidateTriageCaches(queryClient, scope.workspaceSlug, scope.projectId, scope.issueId, {
        descriptionChanged: changedFields.includes("description_html"),
      });
      advanceAfter(scope.issueId);
    },
    [advanceAfter, queryClient, scope.issueId, scope.projectId, scope.workspaceSlug]
  );

  const handleAcceptClosed = React.useCallback(() => {
    if (acceptSaved.current) {
      acceptSaved.current = false;
      acceptCommitted.current = false;
      return;
    }
    if (acceptCommitted.current) {
      // The accept committed but the form save failed: re-read the
      // server state and stay on the now-accepted request.
      acceptCommitted.current = false;
      void invalidateTriageCaches(queryClient, scope.workspaceSlug, scope.projectId, scope.issueId);
    }
  }, [queryClient, scope.issueId, scope.projectId, scope.workspaceSlug]);

  const acceptTarget = React.useMemo<WorkItemEditTarget>(
    () => ({
      kind: "issue",
      id: scope.issueId,
      projectId: scope.projectId,
      snapshot: scope.issue,
      onChanged: handleAcceptClosed,
    }),
    [handleAcceptClosed, scope.issue, scope.issueId, scope.projectId]
  );

  const confirmDecline = React.useCallback(() => {
    declineMove.mutate(
      { status: INTAKE_STATUS.declined },
      {
        onSuccess: () => {
          setOpenDialog(null);
          advanceAfter(scope.issueId);
        },
      }
    );
  }, [advanceAfter, declineMove, scope.issueId]);

  const confirmSnooze = React.useCallback(
    (isoDay: string) => {
      const stamp = snoozeTimestampForDay(isoDay);
      if (stamp === null) return;
      snoozeMove.mutate(
        { status: INTAKE_STATUS.snoozed, snoozed_till: stamp },
        {
          onSuccess: () => {
            setOpenDialog(null);
            advanceAfter(scope.issueId);
          },
        }
      );
    },
    [advanceAfter, scope.issueId, snoozeMove]
  );

  const pickDuplicateTarget = React.useCallback(
    (targetId: string) => {
      duplicateMove.mutate(
        { status: INTAKE_STATUS.duplicate, duplicate_to: targetId },
        {
          onSuccess: () => {
            // Marking duplicate stays on the request so the duplicate-of
            // link is one step away; focus stays on the resolved detail.
            setOpenDialog(null);
            scheduleTriageFocus(false);
          },
        }
      );
    },
    [duplicateMove]
  );

  const confirmDelete = React.useCallback(() => {
    deleteMove.mutate(undefined, {
      onSuccess: () => {
        setOpenDialog(null);
        advanceAfter(scope.issueId);
      },
    });
  }, [advanceAfter, deleteMove, scope.issueId]);

  return {
    visibility,
    leadEntries: leadEntries.lead,
    trailEntries: leadEntries.trail,
    openDialog,
    setOpenDialog,
    fireAction,
    acceptTarget,
    acceptTitle: `Add ${scope.issueKey} to project work items`,
    runAcceptWrite,
    handleAcceptSaved,
    handleAcceptClosed,
    declinePending: declineMove.isPending,
    confirmDecline,
    snoozePending: snoozeMove.isPending,
    snoozeInitialDay: snoozeDayFromTimestamp(scope.snoozedTill),
    confirmSnooze,
    duplicatePending: duplicateMove.isPending,
    pickDuplicateTarget,
    deletePending: deleteMove.isPending,
    deleteAccepted: scope.status === INTAKE_STATUS.accepted,
    confirmDelete,
  };
}
