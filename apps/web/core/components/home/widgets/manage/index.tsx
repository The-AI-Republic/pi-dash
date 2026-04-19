/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// pi dash types
// pi dash ui
import { useTranslation } from "@pi-dash/i18n";
import { EModalWidth, ModalCore } from "@pi-dash/ui";
import { WidgetList } from "./widget-list";

export type TProps = {
  workspaceSlug: string;
  isModalOpen: boolean;
  handleOnClose?: () => void;
};

export const ManageWidgetsModal = observer(function ManageWidgetsModal(props: TProps) {
  // props
  const { workspaceSlug, isModalOpen, handleOnClose } = props;
  const { t } = useTranslation();

  return (
    <ModalCore isOpen={isModalOpen} handleClose={handleOnClose} width={EModalWidth.MD}>
      <div className="p-4">
        <div className="text-18 font-medium"> {t("home.manage_widgets")}</div>
        <WidgetList workspaceSlug={workspaceSlug} />
      </div>
    </ModalCore>
  );
});
