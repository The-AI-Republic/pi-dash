/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useCallback, useState } from "react";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
// hooks
import { useWorkspace } from "@/hooks/store/use-workspace";
// services
import { AgentRunService } from "@/services/runner";

const agentRunService = new AgentRunService();

type CreateRunArgs = {
  workspaceSlug: string;
  issueId: string;
  prompt: string;
};

export function useCreateAgentRun() {
  const workspaceStore = useWorkspace();
  const [isSubmitting, setIsSubmitting] = useState(false);

  const triggerRun = useCallback(
    async ({ workspaceSlug, issueId, prompt }: CreateRunArgs) => {
      const workspace = workspaceStore.getWorkspaceBySlug(workspaceSlug);
      if (!workspace?.id) {
        setToast({
          type: TOAST_TYPE.ERROR,
          title: "Could not start agent run",
          message: "Workspace not found.",
        });
        return null;
      }

      setIsSubmitting(true);
      try {
        const run = await agentRunService.createAgentRun({
          workspace: workspace.id,
          work_item: issueId,
          prompt,
        });
        setToast({
          type: TOAST_TYPE.SUCCESS,
          title: "Agent run started",
          message: "The AI agent will pick up this work item shortly.",
        });
        return run;
      } catch (error: unknown) {
        const message = (error as { error?: string })?.error ?? "Could not start the agent run. Please try again.";
        setToast({
          type: TOAST_TYPE.ERROR,
          title: "Failed to start agent run",
          message,
        });
        return null;
      } finally {
        setIsSubmitting(false);
      }
    },
    [workspaceStore]
  );

  return { triggerRun, isSubmitting };
}
