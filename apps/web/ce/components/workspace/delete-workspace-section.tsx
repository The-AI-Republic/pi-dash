/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
// apple pi dash imports
import { WORKSPACE_TRACKER_ELEMENTS } from "@apple-pi-dash/constants";
import { useTranslation } from "@apple-pi-dash/i18n";
import { Button } from "@apple-pi-dash/propel/button";
import type { IWorkspace } from "@apple-pi-dash/types";
// components
import { SettingsBoxedControlItem } from "@/components/settings/boxed-control-item";
// local imports
import { DeleteWorkspaceModal } from "./delete-workspace-modal";

type TDeleteWorkspace = {
  workspace: IWorkspace | null;
};

export const DeleteWorkspaceSection = observer(function DeleteWorkspaceSection(props: TDeleteWorkspace) {
  const { workspace } = props;
  // states
  const [deleteWorkspaceModal, setDeleteWorkspaceModal] = useState(false);
  // translation
  const { t } = useTranslation();

  return (
    <>
      <DeleteWorkspaceModal
        data={workspace}
        isOpen={deleteWorkspaceModal}
        onClose={() => setDeleteWorkspaceModal(false)}
      />
      <SettingsBoxedControlItem
        title={t("workspace_settings.settings.general.delete_workspace")}
        description={t("workspace_settings.settings.general.delete_workspace_description")}
        control={
          <Button
            variant="error-outline"
            onClick={() => setDeleteWorkspaceModal(true)}
            data-ph-element={WORKSPACE_TRACKER_ELEMENTS.DELETE_WORKSPACE_BUTTON}
          >
            {t("delete")}
          </Button>
        }
      />
    </>
  );
});
