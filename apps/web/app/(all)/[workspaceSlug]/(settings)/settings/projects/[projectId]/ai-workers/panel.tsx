/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { useProject } from "@/hooks/store/use-project";
import { useUserPermissions } from "@/hooks/store/user";
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";

export const AiWorkersPanel = observer(function AiWorkersPanel({
  workspaceSlug,
  projectId,
}: {
  workspaceSlug: string;
  projectId: string;
}) {
  const { t } = useTranslation();
  const { currentProjectDetails, updateProject } = useProject();
  const { allowPermissions } = useUserPermissions();
  const canEdit = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.PROJECT, workspaceSlug, projectId);
  const current = currentProjectDetails?.dedicated_runner_lost_action ?? "fail_to_backlog";
  const [value, setValue] = useState<"fail_to_backlog" | "fall_back">(current);
  const isDirty = value !== current;
  const [saving, setSaving] = useState(false);

  const handleSave = async () => {
    setSaving(true);
    try {
      await updateProject(workspaceSlug, projectId, { dedicated_runner_lost_action: value } as never);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="max-w-2xl space-y-6">
      <section className="rounded-sm border border-subtle bg-surface-2 p-4">
        <h3 className="text-body-md-medium">{t("When a dedicated runner is revoked or deleted")}</h3>
        <p className="mt-1 text-body-xs-regular text-secondary">
          {t(
            "Choose what happens to issues dedicated to that runner. This applies to every dedicated runner in the project."
          )}
        </p>
        <fieldset className="mt-4 space-y-3" disabled={!canEdit}>
          <label className="flex cursor-pointer items-start gap-3 rounded-sm border border-subtle bg-surface-1 p-3">
            <input
              type="radio"
              name="dedicated_runner_lost_action"
              checked={value === "fail_to_backlog"}
              onChange={() => setValue("fail_to_backlog")}
              className="mt-0.5"
            />
            <span>
              <span className="block text-body-xs-medium">{t("Fail the run and move the issue to Backlog")}</span>
              <span className="mt-0.5 block text-body-xs-regular text-secondary">
                {t(
                  "The queued run fails, the assignment is cleared, and the issue moves to Backlog with a comment naming the runner. A person must choose a new runner or the pod."
                )}
              </span>
            </span>
          </label>
          <label className="flex cursor-pointer items-start gap-3 rounded-sm border border-subtle bg-surface-1 p-3">
            <input
              type="radio"
              name="dedicated_runner_lost_action"
              checked={value === "fall_back"}
              onChange={() => setValue("fall_back")}
              className="mt-0.5"
            />
            <span>
              <span className="block text-body-xs-medium">{t("Fall back to the pod")}</span>
              <span className="mt-0.5 block text-body-xs-regular text-secondary">
                {t(
                  "The issue falls back to its own pod, then the project's default pod, then bounces to Backlog when neither has a runner that can serve it. The system comment names the runner and the pod used."
                )}
              </span>
            </span>
          </label>
        </fieldset>
        {!canEdit && (
          <p className="mt-3 text-body-xs-regular text-tertiary">{t("Only project admins can change this setting.")}</p>
        )}
        {canEdit && (
          <div className="mt-4 flex justify-end">
            <Button onClick={handleSave} disabled={!isDirty || saving} size="sm">
              {saving ? t("Saving…") : t("Save")}
            </Button>
          </div>
        )}
      </section>
    </div>
  );
});
