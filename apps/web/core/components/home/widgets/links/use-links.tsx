/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useMemo } from "react";
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { TProjectLink } from "@pi-dash/types";
import { useHome } from "@/hooks/store/use-home";

export type TLinkOperations = {
  create: (data: Partial<TProjectLink>) => Promise<void>;
  update: (linkId: string, data: Partial<TProjectLink>) => Promise<void>;
  remove: (linkId: string) => Promise<void>;
};
export type TProjectLinkRoot = {
  workspaceSlug: string;
};

export const useLinks = (workspaceSlug: string) => {
  // hooks
  const {
    quickLinks: {
      createLink,
      updateLink,
      removeLink,
      isLinkModalOpen,
      toggleLinkModal,
      linkData,
      setLinkData,
      fetchLinks,
    },
  } = useHome();
  const { t } = useTranslation();

  const linkOperations: TLinkOperations = useMemo(
    () => ({
      create: async (data: Partial<TProjectLink>) => {
        try {
          if (!workspaceSlug) throw new Error("Missing required fields");
          await createLink(workspaceSlug, data);
          setToast({
            message: t("The link has been successfully created"),
            type: TOAST_TYPE.SUCCESS,
            title: t("Link created"),
          });
          toggleLinkModal(false);
        } catch (error: any) {
          console.error("error", error?.data?.error);
          setToast({
            message: error?.data?.error ?? t("The link could not be created"),
            type: TOAST_TYPE.ERROR,
            title: t("Link not created"),
          });
          throw error;
        }
      },
      update: async (linkId: string, data: Partial<TProjectLink>) => {
        try {
          if (!workspaceSlug) throw new Error("Missing required fields");
          await updateLink(workspaceSlug, linkId, data);
          setToast({
            message: t("The link has been successfully updated"),
            type: TOAST_TYPE.SUCCESS,
            title: t("Link updated"),
          });
          toggleLinkModal(false);
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
          if (!workspaceSlug) throw new Error("Missing required fields");
          await removeLink(workspaceSlug, linkId);
          setToast({
            message: t("The link has been successfully removed"),
            type: TOAST_TYPE.SUCCESS,
            title: t("The link has been successfully removed"),
          });
        } catch (error: any) {
          setToast({
            message: error?.data?.error ?? t("The link could not be removed"),
            type: TOAST_TYPE.ERROR,
            title: t("Link not removed"),
          });
        }
      },
    }),
    [workspaceSlug]
  );

  const handleOnClose = () => {
    toggleLinkModal(false);
  };

  return { linkOperations, handleOnClose, isLinkModalOpen, toggleLinkModal, linkData, setLinkData, fetchLinks };
};
