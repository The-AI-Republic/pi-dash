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
 * Drives the "re-tick" affordance on the issue AgentRun card: re-grants a
 * fresh ticking budget to an exhausted issue ticker so the periodic agent
 * runs resume. The server enforces the guardrails (ticking state +
 * exhausted budget); a ``granted: false`` response is a normal outcome, not
 * an error, so we surface it as an informational toast rather than a
 * failure.
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
            message: t("Granted a fresh ticking budget. The AI agent will resume on its schedule."),
          });
        } else {
          setToast({
            type: TOAST_TYPE.INFO,
            title: t("Nothing to re-tick"),
            message: t("Re-ticking only applies while the issue is ticking and its budget is used up."),
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
