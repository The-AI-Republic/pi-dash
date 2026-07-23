/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useCallback, useState } from "react";
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
// services
import { AgentRunService } from "@/services/runner";

const agentRunService = new AgentRunService();

/**
 * Drives the "Abort run" affordance on the issue AgentRun card: signals the
 * associated runner to stop the active run and marks the AgentRun terminal
 * (``cancelled``) server-side. The runner stops as soon as it receives the
 * signal; the server rejects an already-terminal run. Returns ``true`` on a
 * successful abort so the caller can refresh the card.
 */
export function useAbortRun() {
  const { t } = useTranslation();
  const [isSubmitting, setIsSubmitting] = useState(false);

  const abortRun = useCallback(
    async (runId: string): Promise<boolean> => {
      setIsSubmitting(true);
      try {
        await agentRunService.abortRun(runId, "user");
        setToast({
          type: TOAST_TYPE.SUCCESS,
          title: t("Run aborted"),
          message: t("The runner will stop this run as soon as it gets the signal."),
        });
        return true;
      } catch (error: unknown) {
        const message = (error as { error?: string })?.error ?? t("Could not abort this run. Please try again.");
        setToast({
          type: TOAST_TYPE.ERROR,
          title: t("Failed to abort run"),
          message,
        });
        return false;
      } finally {
        setIsSubmitting(false);
      }
    },
    [t]
  );

  return { abortRun, isSubmitting };
}
