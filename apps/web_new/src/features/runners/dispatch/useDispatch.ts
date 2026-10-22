// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dispatch hooks for issue surfaces (NEWFRONT-547, RUN-046/RUN-048).
// The mounts live in the issues area; these hooks own the submitting
// flag, the started/queued/refused toasts, and the best-effort issue
// refresh the caller hands in.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import type { AgentRunCreateResult, ReTickResponse } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { platform } from "../../../core/platform/index.js";
import { notifyError, notifyInfo, notifySuccess } from "../../../shared/shell/index.js";
import { runnersKeys } from "../api/keys.js";
import type { HeartbeatPoll } from "../runtime/service.js";
import { abortAgentRun, isQueuedDispatch, requestReTick, triggerAgentRun, type DispatchMode } from "./dispatch.js";

/** A fresh run left the queue and started. */
export const RUN_STARTED_TITLE = "Run started";
const RUN_STARTED_BODY = "The agent is picking up this work item.";

/** A run is already active, so this dispatch waits its turn. */
export const RUN_QUEUED_TITLE = "Run queued";
const RUN_QUEUED_BODY = "Another run is still active — this one starts when it ends.";

/** Dispatch never reached the server or the server refused it. */
export const RUN_START_FAILED_TITLE = "Couldn't start the run";
const RUN_START_FAILED_BODY = "Try again in a moment.";

/** Abort never reached the server or the server refused it. */
export const RUN_ABORT_FAILED_TITLE = "Couldn't stop the run";
const RUN_ABORT_FAILED_BODY = "The run is still going — try again.";

/** The server granted fresh budget. */
export const RE_TICK_GRANTED_TITLE = "Budget refilled";
const RE_TICK_GRANTED_NOW_BODY = "More runs are available and the agent is starting now.";
const RE_TICK_GRANTED_QUEUED_BODY = "More runs are available; the next one starts when the active run ends.";

/** The server refused: nothing is owed right now. */
export const RE_TICK_REFUSED_TITLE = "No refill needed";
const RE_TICK_REFUSED_BODY = "A refill applies only while the issue is ticking with a spent budget.";

/** The re-tick never reached the server. */
export const RE_TICK_FAILED_TITLE = "Couldn't re-tick";
const RE_TICK_FAILED_BODY = "Try again in a moment.";

/** Best-effort refresh after a settled dispatch, owned by the caller. */
export interface DispatchRefreshOptions {
  onSettledRefresh?: (() => Promise<unknown>) | undefined;
}

/** Server or transport message when one reads, else the fallback. */
function failureMessage(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message !== "") return error.message;
  return fallback;
}

export interface TriggerRunArgs {
  workspaceId: string;
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  mode: DispatchMode;
  heartbeatPoll?: HeartbeatPoll | undefined;
}

/**
 * Run AI and Comment & Run from an issue control. Returns the run (or
 * the queued notice) on success, null on failure; the executor the
 * server resolves is never written back.
 */
export function useTriggerAgentRun(options: DispatchRefreshOptions = {}) {
  const [isSubmitting, setIsSubmitting] = React.useState(false);
  const refresh = options.onSettledRefresh;

  const triggerRun = React.useCallback(
    async (args: TriggerRunArgs): Promise<AgentRunCreateResult | null> => {
      setIsSubmitting(true);
      try {
        const run = await triggerAgentRun({ client: getAppClient(), runtime: platform.agentRuntime, ...args });
        if (isQueuedDispatch(run)) {
          notifyInfo(RUN_QUEUED_TITLE, RUN_QUEUED_BODY);
        } else {
          notifySuccess(RUN_STARTED_TITLE, RUN_STARTED_BODY);
        }
        try {
          await refresh?.();
        } catch {
          /* the card catches up on its next scheduled refresh */
        }
        return run;
      } catch (error: unknown) {
        notifyError(RUN_START_FAILED_TITLE, failureMessage(error, RUN_START_FAILED_BODY));
        return null;
      } finally {
        setIsSubmitting(false);
      }
    },
    [refresh]
  );

  return { triggerRun, isSubmitting };
}

export interface AbortRunArgs {
  runId: string;
  reason?: string | undefined;
}

/**
 * Abort an in-flight run from an issue card. Refreshes the run detail
 * and every runs list page after the server answers; failures toast
 * and the run keeps going.
 */
export function useAbortAgentRun(workspaceSlug: string, options: DispatchRefreshOptions = {}) {
  const queryClient = useQueryClient();
  const refresh = options.onSettledRefresh;
  return useMutation({
    mutationFn: (args: AbortRunArgs) => abortAgentRun(getAppClient(), args.runId, args.reason),
    onError: () => {
      notifyError(RUN_ABORT_FAILED_TITLE, RUN_ABORT_FAILED_BODY);
    },
    onSuccess: (run) => {
      queryClient.setQueryData(runnersKeys.run(workspaceSlug, run.id), run);
    },
    onSettled: async () => {
      await queryClient.invalidateQueries({ queryKey: runnersKeys.all(workspaceSlug) });
      try {
        await refresh?.();
      } catch {
        /* the card catches up on its next scheduled refresh */
      }
    },
  });
}

/**
 * Re-tick an issue's run budget. A grant toasts success, a refusal
 * toasts informationally — both refresh the issue — and only a
 * transport failure toasts an error. Human-only: agent runs never
 * call this (enforced server-side).
 */
export function useReTick(options: DispatchRefreshOptions = {}) {
  const [isSubmitting, setIsSubmitting] = React.useState(false);
  const refresh = options.onSettledRefresh;

  const reTick = React.useCallback(
    async (issueId: string): Promise<ReTickResponse | null> => {
      setIsSubmitting(true);
      try {
        const result = await requestReTick(getAppClient(), issueId);
        if (result.granted) {
          notifySuccess(
            RE_TICK_GRANTED_TITLE,
            result.run_id === null ? RE_TICK_GRANTED_QUEUED_BODY : RE_TICK_GRANTED_NOW_BODY
          );
        } else {
          notifyInfo(RE_TICK_REFUSED_TITLE, RE_TICK_REFUSED_BODY);
        }
        try {
          await refresh?.();
        } catch {
          /* the card catches up on its next scheduled refresh */
        }
        return result;
      } catch (error: unknown) {
        notifyError(RE_TICK_FAILED_TITLE, failureMessage(error, RE_TICK_FAILED_BODY));
        return null;
      } finally {
        setIsSubmitting(false);
      }
    },
    [refresh]
  );

  return { reTick, isSubmitting };
}
