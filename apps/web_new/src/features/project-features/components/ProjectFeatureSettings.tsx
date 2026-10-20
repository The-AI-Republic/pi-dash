// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One dedicated feature tab (parity/specs/project-settings-part-f.md,
// PSET-043–047). The switch reads the already-loaded project row: no
// fetch, no loader of its own — the shell has resolved the project and
// its standing before this body mounts. Each press PATCHes one boolean
// under a pending notice that settles into success or failure; a failed
// write leaves the switch on the value the server last stored.

import { Switch } from "@pidash/kit";
import * as React from "react";

import { useProjectSettings } from "../../project-settings/index.js";
import { useProjectRow } from "../../auth/index.js";
import { notifyPending } from "../../../shared/shell/index.js";
import {
  FEATURE_UPDATE_FAILED_MESSAGE,
  FEATURE_UPDATE_FAILED_TITLE,
  FEATURE_UPDATE_PENDING_MESSAGE,
  FEATURE_UPDATE_PENDING_TITLE,
  FEATURE_UPDATE_SUCCESS_MESSAGE,
  FEATURE_UPDATE_SUCCESS_TITLE,
  useUpdateProjectFeature,
  type FeaturePatch,
} from "../api/featureQueries.js";
import { isFeatureEnabled, nextFeatureValue, type ProjectFeatureDef } from "../lib/registry.js";

export interface ProjectFeatureSettingsProps {
  feature: ProjectFeatureDef;
}

export function ProjectFeatureSettings({ feature }: ProjectFeatureSettingsProps): React.ReactElement {
  const { workspaceSlug, projectId } = useProjectSettings();
  const project = useProjectRow(workspaceSlug, projectId);
  const update = useUpdateProjectFeature(workspaceSlug);
  const row = project.data;
  const enabled = isFeatureEnabled(row, feature);
  const descriptionId = React.useId();

  function toggle(): void {
    const next = nextFeatureValue(row, feature);
    const notice = notifyPending(FEATURE_UPDATE_PENDING_TITLE, FEATURE_UPDATE_PENDING_MESSAGE);
    update.mutate(
      { projectId, patch: { [feature.flag]: next } as FeaturePatch },
      {
        onSuccess: () =>
          notice.settle({
            title: FEATURE_UPDATE_SUCCESS_TITLE,
            description: FEATURE_UPDATE_SUCCESS_MESSAGE,
          }),
        onError: () =>
          notice.settle({
            title: FEATURE_UPDATE_FAILED_TITLE,
            description: FEATURE_UPDATE_FAILED_MESSAGE,
            failed: true,
          }),
      }
    );
  }

  return (
    <section
      aria-label={`${feature.label} feature`}
      data-testid="project-feature-settings"
      data-feature={feature.key}
      className="flex flex-col gap-(--space-6)"
    >
      <div className="flex items-start justify-between gap-(--space-6) rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-6) py-(--space-5)">
        <div className="flex min-w-0 flex-col gap-(--space-1)">
          <h2 className="text-body font-medium text-(--text)">{feature.switchLabel}</h2>
          <p id={descriptionId} className="text-meta text-(--text-muted)">
            {feature.explainer}
          </p>
        </div>
        <Switch
          checked={enabled}
          onCheckedChange={toggle}
          label={feature.switchLabel}
          {...(update.isPending ? { disabled: true } : {})}
          className="mt-(--space-1)"
        />
      </div>
    </section>
  );
}
