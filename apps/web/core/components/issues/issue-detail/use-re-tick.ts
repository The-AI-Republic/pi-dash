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
import type { TReTickResponse } from "@/services/runner";

const agentRunService = new AgentRunService();

/**
 * Drives the "re-tick" affordance on the issue AgentRun card: adds the
 * project's Re-tick grant to an issue whose run pool is spent and starts a
 * run now (or queues it if one is active). The server enforces the
 * guardrails (ticking state + spent pool); a ``granted: false`` response is
 * a normal outcome, not an error, so we surface it as an informational
 * toast rather than a failure.
 */
export function useReTick() {
  const { t } = useTranslation();
  const [isSubmitting, setIsSubmitting] = useState(false);

  const reTick = useCallback(
    async (issueId: string): Promise<TReTickResponse | null> => {
      setIsSubmitting(true);
      try {
        const result = await agentRunService.reTick({ work_item: issueId });
        if (result?.granted) {
          setToast({
            type: TOAST_TYPE.SUCCESS,
            title: t("Ticking restarted"),
            message: result.run_id
              ? t("Added more runs to this issue's budget. The AI agent is starting now.")
              : t("Added more runs to this issue's budget. The next run starts as soon as the active run ends."),
          });
        } else {
          setToast({
            type: TOAST_TYPE.INFO,
            title: t("Nothing to re-tick"),
            message: t("Re-ticking only applies while the issue is ticking and its run budget is used up."),
          });
        }
        return result;
      } catch (error: unknown) {
        const message = (error as { error?: string })?.error ?? t("Could not re-tick this issue. Please try again.");
        setToast({
          type: TOAST_TYPE.ERROR,
          title: t("Failed to re-tick"),
          message,
        });
        return null;
      } finally {
        setIsSubmitting(false);
      }
    },
    [t]
  );

  return { reTick, isSubmitting };
}
