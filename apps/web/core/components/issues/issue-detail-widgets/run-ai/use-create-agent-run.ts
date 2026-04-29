/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useCallback, useState } from "react";
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
// hooks
import { useWorkspace } from "@/hooks/store/use-workspace";
// services
import { AgentRunService } from "@/services/runner";

const agentRunService = new AgentRunService();

type CreateRunArgs = {
  workspaceSlug: string;
  issueId: string;
  /** When provided, dispatches a fresh prompted run via ``createAgentRun``
   * (the "Run AI" button path). When omitted, falls through to
   * ``commentAndRun`` which reuses the per-issue continuation pipeline
   * and rebuilds the prompt from issue + comments server-side (the
   * "Comment & Run" modal path). */
  prompt?: string;
};

export function useCreateAgentRun() {
  const workspaceStore = useWorkspace();
  const { t } = useTranslation();
  const [isSubmitting, setIsSubmitting] = useState(false);

  const triggerRun = useCallback(
    async ({ workspaceSlug, issueId, prompt }: CreateRunArgs) => {
      const workspace = workspaceStore.getWorkspaceBySlug(workspaceSlug);
      if (!workspace?.id) {
        setToast({
          type: TOAST_TYPE.ERROR,
          title: t("run_ai.failed_workspace_title"),
          message: t("run_ai.workspace_not_found"),
        });
        return null;
      }

      setIsSubmitting(true);
      try {
        const run = prompt
          ? await agentRunService.createAgentRun({
              workspace: workspace.id,
              work_item: issueId,
              prompt,
            })
          : await agentRunService.commentAndRun({
              workspace: workspace.id,
              work_item: issueId,
            });
        setToast({
          type: TOAST_TYPE.SUCCESS,
          title: t("run_ai.success_title"),
          message: t("run_ai.success_message"),
        });
        return run;
      } catch (error: unknown) {
        const message = (error as { error?: string })?.error ?? t("run_ai.failed_message");
        setToast({
          type: TOAST_TYPE.ERROR,
          title: t("run_ai.failed_title"),
          message,
        });
        return null;
      } finally {
        setIsSubmitting(false);
      }
    },
    [workspaceStore, t]
  );

  return { triggerRun, isSubmitting };
}
