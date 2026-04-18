/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useState } from "react";
// constants
import type { EPageAccess } from "@apple-pi-dash/constants";
import type { TPage } from "@apple-pi-dash/types";
// ui
import { EModalPosition, EModalWidth, ModalCore } from "@apple-pi-dash/ui";
// hooks
import { useAppRouter } from "@/hooks/use-app-router";
// apple pi dash web hooks
import type { EPageStoreType } from "@/apple-pi-dash-web/hooks/store";
import { usePageStore } from "@/apple-pi-dash-web/hooks/store";
// local imports
import { PageForm } from "./page-form";

type Props = {
  workspaceSlug: string;
  projectId: string;
  isModalOpen: boolean;
  pageAccess?: EPageAccess;
  handleModalClose: () => void;
  redirectionEnabled?: boolean;
  storeType: EPageStoreType;
};

export function CreatePageModal(props: Props) {
  const {
    workspaceSlug,
    projectId,
    isModalOpen,
    pageAccess,
    handleModalClose,
    redirectionEnabled = false,
    storeType,
  } = props;
  // states
  const [pageFormData, setPageFormData] = useState<Partial<TPage>>({
    id: undefined,
    name: "",
    logo_props: undefined,
  });
  // router
  const router = useAppRouter();
  // store hooks
  const { createPage } = usePageStore(storeType);
  const handlePageFormData = <T extends keyof TPage>(key: T, value: TPage[T]) =>
    setPageFormData((prev) => ({ ...prev, [key]: value }));

  // update page access in form data when page access from the store changes
  useEffect(() => {
    setPageFormData((prev) => ({ ...prev, access: pageAccess }));
  }, [pageAccess]);

  const handleStateClear = () => {
    setPageFormData({ id: undefined, name: "", access: pageAccess });
    handleModalClose();
  };

  const handleFormSubmit = async () => {
    if (!workspaceSlug || !projectId) return;

    try {
      const pageData = await createPage(pageFormData);
      if (pageData) {
        handleStateClear();
        if (redirectionEnabled) router.push(`/${workspaceSlug}/projects/${projectId}/pages/${pageData.id}`);
      }
    } catch (error) {
      console.error(error);
    }
  };

  return (
    <ModalCore
      isOpen={isModalOpen}
      handleClose={handleModalClose}
      position={EModalPosition.TOP}
      width={EModalWidth.XXL}
    >
      <PageForm
        formData={pageFormData}
        handleFormData={handlePageFormData}
        handleModalClose={handleStateClear}
        handleFormSubmit={handleFormSubmit}
      />
    </ModalCore>
  );
}
