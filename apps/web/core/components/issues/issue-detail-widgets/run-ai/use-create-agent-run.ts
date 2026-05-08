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
  /** Selects the dispatch path. Both modes have the prompt rendered
   * server-side from the issue's phase template (``coding-task`` for
   * In Progress, ``review`` for In Review, default otherwise) so the
   * agent receives the same prompt a state transition / tick produces.
   * - ``"run_ai"``: the "Run AI" button — no comment is posted first.
   * - ``"comment_and_run"``: the Comment & Run modal — caller has just
   *   posted a comment on the issue. */
  mode: "run_ai" | "comment_and_run";
};

export function useCreateAgentRun() {
  const workspaceStore = useWorkspace();
  const { t } = useTranslation();
  const [isSubmitting, setIsSubmitting] = useState(false);

  const triggerRun = useCallback(
    async ({ workspaceSlug, issueId, mode }: CreateRunArgs) => {
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
        const run =
          mode === "run_ai"
            ? await agentRunService.runAi({
                workspace: workspace.id,
                work_item: issueId,
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
