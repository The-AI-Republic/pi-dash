/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
// ui
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { AlertModalCore } from "@pi-dash/ui";

interface IStickyDelete {
  isOpen: boolean;
  handleSubmit: () => Promise<void>;
  handleClose: () => void;
}

export const StickyDeleteModal = observer(function StickyDeleteModal(props: IStickyDelete) {
  const { isOpen, handleClose, handleSubmit } = props;
  // states
  const [loader, setLoader] = useState(false);
  // hooks
  const { t } = useTranslation();

  const formSubmit = async () => {
    try {
      setLoader(true);
      await handleSubmit();
    } catch {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Sticky not removed"),
        message: t("The sticky could not be removed"),
      });
    } finally {
      setLoader(false);
    }
  };

  return (
    <AlertModalCore
      handleClose={handleClose}
      handleSubmit={formSubmit}
      isSubmitting={loader}
      isOpen={isOpen}
      title={t("Delete sticky")}
      content={t("Are you sure you want to delete this sticky?")}
    />
  );
});
