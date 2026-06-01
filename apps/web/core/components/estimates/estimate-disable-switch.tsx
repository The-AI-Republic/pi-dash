/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { ToggleSwitch } from "@pi-dash/ui";
// hooks
import { useProjectEstimates } from "@/hooks/store/estimates";
import { useProject } from "@/hooks/store/use-project";
// i18n
type TEstimateDisableSwitch = {
  workspaceSlug: string;
  projectId: string;
  isAdmin: boolean;
};

export const EstimateDisableSwitch = observer(function EstimateDisableSwitch(props: TEstimateDisableSwitch) {
  const { workspaceSlug, projectId, isAdmin } = props;
  // i18n
  const { t } = useTranslation();
  // hooks
  const { updateProject, currentProjectDetails } = useProject();
  const { currentActiveEstimateId } = useProjectEstimates();

  const currentProjectActiveEstimate = currentProjectDetails?.estimate || undefined;

  const disableEstimate = async () => {
    if (!workspaceSlug || !projectId) return;

    try {
      await updateProject(workspaceSlug, projectId, {
        estimate: currentProjectActiveEstimate ? null : currentActiveEstimateId,
      });
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: currentProjectActiveEstimate
          ? t("Success!")
          : t("Success!"),
        message: currentProjectActiveEstimate
          ? t("Estimates have been disabled.")
          : t("Estimates have been enabled."),
      });
    } catch (_err) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Error!"),
        message: t("Estimate could not be disabled. Please try again"),
      });
    }
  };

  return (
    <ToggleSwitch
      value={Boolean(currentProjectActiveEstimate)}
      onChange={disableEstimate}
      disabled={!isAdmin}
      size="sm"
    />
  );
});
