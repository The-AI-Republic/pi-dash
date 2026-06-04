/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useMemo } from "react";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { TIssueLink, TIssueServiceType } from "@pi-dash/types";
// hooks
import { useIssueDetail } from "@/hooks/store/use-issue-detail";
// local imports
import type { TLinkOperations } from "../../issue-detail/links";

export const useLinkOperations = (
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  issueServiceType: TIssueServiceType
): TLinkOperations => {
  const { createLink, updateLink, removeLink } = useIssueDetail(issueServiceType);
  // i18n
  const { t } = useTranslation();

  const handleLinkOperations: TLinkOperations = useMemo(
    () => ({
      create: async (data: Partial<TIssueLink>) => {
        try {
          if (!workspaceSlug || !projectId || !issueId) throw new Error("Missing required fields");
          await createLink(workspaceSlug, projectId, issueId, data);
          setToast({
            message: t("The link has been successfully created"),
            type: TOAST_TYPE.SUCCESS,
            title: t("Link created"),
          });
        } catch (error: any) {
          setToast({
            message: error?.data?.error ?? t("The link could not be created"),
            type: TOAST_TYPE.ERROR,
            title: t("Link not created"),
          });
          throw error;
        }
      },
      update: async (linkId: string, data: Partial<TIssueLink>) => {
        try {
          if (!workspaceSlug || !projectId || !issueId) throw new Error("Missing required fields");
          await updateLink(workspaceSlug, projectId, issueId, linkId, data);
          setToast({
            message: t("The link has been successfully updated"),
            type: TOAST_TYPE.SUCCESS,
            title: t("Link updated"),
          });
        } catch (error: any) {
          setToast({
            message: error?.data?.error ?? t("The link could not be updated"),
            type: TOAST_TYPE.ERROR,
            title: t("Link not updated"),
          });
          throw error;
        }
      },
      remove: async (linkId: string) => {
        try {
          if (!workspaceSlug || !projectId || !issueId) throw new Error("Missing required fields");
          await removeLink(workspaceSlug, projectId, issueId, linkId);
          setToast({
            message: t("The link has been successfully removed"),
            type: TOAST_TYPE.SUCCESS,
            title: t("Link removed"),
          });
        } catch {
          setToast({
            message: t("The link could not be removed"),
            type: TOAST_TYPE.ERROR,
            title: t("Link not removed"),
          });
        }
      },
    }),
    [workspaceSlug, projectId, issueId, createLink, updateLink, removeLink, t]
  );

  return handleLinkOperations;
};
