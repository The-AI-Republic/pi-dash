/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { mutate } from "swr";
// types
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { APITokenService } from "@pi-dash/services";
import type { IApiToken } from "@pi-dash/types";
// ui
import { AlertModalCore } from "@pi-dash/ui";
// fetch-keys
import { API_TOKENS_LIST } from "@/constants/fetch-keys";

type Props = {
  isOpen: boolean;
  onClose: () => void;
  tokenId: string;
};

const apiTokenService = new APITokenService();

export function DeleteApiTokenModal(props: Props) {
  const { isOpen, onClose, tokenId } = props;
  // states
  const [deleteLoading, setDeleteLoading] = useState<boolean>(false);
  // router params
  const { t } = useTranslation();

  const handleClose = () => {
    onClose();
    setDeleteLoading(false);
  };

  const handleDeletion = async () => {
    setDeleteLoading(true);

    await apiTokenService
      .destroy(tokenId)
      .then(() => {
        setToast({
          type: TOAST_TYPE.SUCCESS,
          title: t("Success!"),
          message: t("The token has been successfully deleted"),
        });

        mutate<IApiToken[]>(
          API_TOKENS_LIST,
          (prevData) => (prevData ?? []).filter((token) => token.id !== tokenId),
          false
        );

        handleClose();
        setDeleteLoading(false);
      })
      .catch((err) => {
        setToast({
          type: TOAST_TYPE.ERROR,
          title: t("Error!"),
          message: err?.message ?? t("The token could not be deleted"),
        });
        setDeleteLoading(false);
      });
  };

  return (
    <AlertModalCore
      handleClose={handleClose}
      handleSubmit={handleDeletion}
      isSubmitting={deleteLoading}
      isOpen={isOpen}
      title={t("Delete personal access token")}
      content={<>{t("Any application using this token will no longer have the access to Pi Dash data. This action cannot be undone.")} </>}
    />
  );
}
