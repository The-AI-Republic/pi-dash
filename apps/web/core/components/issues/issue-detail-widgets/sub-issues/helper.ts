/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useMemo } from "react";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { TIssueServiceType, TSubIssueOperations } from "@pi-dash/types";
import { EIssueServiceType } from "@pi-dash/types";
import { copyUrlToClipboard } from "@pi-dash/utils";
// hooks
import { useIssueDetail } from "@/hooks/store/use-issue-detail";

export const useSubIssueOperations = (issueServiceType: TIssueServiceType): TSubIssueOperations => {
  // translation
  const { t } = useTranslation();
  // store hooks
  const {
    subIssues: { setSubIssueHelpers },
    createSubIssues,
    fetchSubIssues,
    updateSubIssue,
    deleteSubIssue,
    removeSubIssue,
  } = useIssueDetail(issueServiceType);

  const subIssueOperations: TSubIssueOperations = useMemo(
    () => ({
      copyLink: async (path) => {
        await copyUrlToClipboard(path);
        setToast({
          type: TOAST_TYPE.SUCCESS,
          title: t("Link copied!"),
          message: t("{entity} link copied to clipboard", {
            entity:
              issueServiceType === EIssueServiceType.ISSUES
                ? t("Sub-work items", { count: 1 })
                : t("{count, plural, one {Work item} other {Work items}}", { count: 1 }),
          }),
        });
      },
      fetchSubIssues: async (workspaceSlug, projectId, parentIssueId) => {
        try {
          await fetchSubIssues(workspaceSlug, projectId, parentIssueId);
        } catch {
          setToast({
            type: TOAST_TYPE.ERROR,
            title: t("Error!"),
            message: t("Error fetching {entity}", {
              entity:
                issueServiceType === EIssueServiceType.ISSUES
                  ? t("Sub-work items", { count: 2 })
                  : t("{count, plural, one {Work item} other {Work items}}", { count: 2 }),
            }),
          });
        }
      },
      addSubIssue: async (workspaceSlug, projectId, parentIssueId, issueIds) => {
        try {
          await createSubIssues(workspaceSlug, projectId, parentIssueId, issueIds);
          setToast({
            type: TOAST_TYPE.SUCCESS,
            title: t("Success!"),
            message: t("{entity} added successfully", {
              entity:
                issueServiceType === EIssueServiceType.ISSUES
                  ? t("Sub-work items")
                  : t("{count, plural, one {Work item} other {Work items}}", { count: issueIds.length }),
            }),
          });
        } catch {
          setToast({
            type: TOAST_TYPE.ERROR,
            title: t("Error!"),
            message: t("Error adding {entity}", {
              entity:
                issueServiceType === EIssueServiceType.ISSUES
                  ? t("Sub-work items")
                  : t("{count, plural, one {Work item} other {Work items}}", { count: issueIds.length }),
            }),
          });
        }
      },
      updateSubIssue: async (
        workspaceSlug,
        projectId,
        parentIssueId,
        issueId,
        issueData,
        oldIssue = {},
        fromModal = false
      ) => {
        try {
          setSubIssueHelpers(parentIssueId, "issue_loader", issueId);
          await updateSubIssue(workspaceSlug, projectId, parentIssueId, issueId, issueData, oldIssue, fromModal);
          setToast({
            type: TOAST_TYPE.SUCCESS,
            title: t("Success!"),
            message: t("Sub-work item updated successfully"),
          });
          setSubIssueHelpers(parentIssueId, "issue_loader", issueId);
        } catch (_error) {
          setToast({
            type: TOAST_TYPE.ERROR,
            title: t("Error!"),
            message: t("Error updating sub-work item"),
          });
        }
      },
      removeSubIssue: async (workspaceSlug, projectId, parentIssueId, issueId) => {
        try {
          setSubIssueHelpers(parentIssueId, "issue_loader", issueId);
          await removeSubIssue(workspaceSlug, projectId, parentIssueId, issueId);
          setToast({
            type: TOAST_TYPE.SUCCESS,
            title: t("Success!"),
            message: t("{entity} removed successfully", {
              entity:
                issueServiceType === EIssueServiceType.ISSUES
                  ? t("Sub-work items")
                  : t("{count, plural, one {Work item} other {Work items}}", { count: 1 }),
            }),
          });
          setSubIssueHelpers(parentIssueId, "issue_loader", issueId);
        } catch (_error) {
          setToast({
            type: TOAST_TYPE.ERROR,
            title: t("Error!"),
            message: t("Error removing {entity}", {
              entity:
                issueServiceType === EIssueServiceType.ISSUES
                  ? t("Sub-work items")
                  : t("{count, plural, one {Work item} other {Work items}}", { count: 1 }),
            }),
          });
        }
      },
      deleteSubIssue: async (workspaceSlug, projectId, parentIssueId, issueId) => {
        try {
          setSubIssueHelpers(parentIssueId, "issue_loader", issueId);
          await deleteSubIssue(workspaceSlug, projectId, parentIssueId, issueId);
          setSubIssueHelpers(parentIssueId, "issue_loader", issueId);
        } catch (_error) {
          setToast({
            type: TOAST_TYPE.ERROR,
            title: t("Error!"),
            message: t("{entity} delete failed", {
              entity:
                issueServiceType === EIssueServiceType.ISSUES
                  ? t("Sub-work items")
                  : t("{count, plural, one {Work item} other {Work items}}", { count: 1 }),
            }),
          });
        }
      },
    }),
    [
      createSubIssues,
      deleteSubIssue,
      fetchSubIssues,
      issueServiceType,
      removeSubIssue,
      setSubIssueHelpers,
      t,
      updateSubIssue,
    ]
  );

  return subIssueOperations;
};
