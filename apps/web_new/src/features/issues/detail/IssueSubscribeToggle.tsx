// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Subscribe toggle for the detail header (parity/specs/issues.md §13,
// ISS-161). Reads and writes the work-item subscription through the same
// status query the contextual palette uses, so both stay in sync. A
// loader shows until the state is known; archived items hide the toggle
// entirely; guests and in-flight writes disable it.
import { Button, Spinner } from "@pidash/kit";
import { subscribeIssue, unsubscribeIssue } from "@pidash/api-client";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Bell, BellOff } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { contextualKeys, paletteSubscriptionQueryOptions } from "../../../shared/shell/contextual/index.js";

export interface IssueSubscribeToggleProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Archived items carry no subscription control. */
  archived: boolean;
  /** Project member standing; guests read the toggle disabled. */
  canWrite: boolean;
  /** After a landed change (the detail re-reads the issue). */
  onChanged?: (() => void) | undefined;
}

export const SUBSCRIBED_TITLE = "Subscribed to work item notifications.";
export const UNSUBSCRIBED_TITLE = "Unsubscribed from work item notifications.";
export const SUBSCRIPTION_FAILED_TITLE = "Couldn't update the subscription.";

export function IssueSubscribeToggle({
  workspaceSlug,
  projectId,
  issueId,
  archived,
  canWrite,
  onChanged,
}: IssueSubscribeToggleProps): React.ReactElement | null {
  const client = getAppClient();
  const queryClient = useQueryClient();
  const status = useQuery({
    ...paletteSubscriptionQueryOptions(client, workspaceSlug, projectId, issueId),
    enabled: !archived,
  });
  const toggle = useMutation({
    mutationFn: async (subscribed: boolean) =>
      subscribed
        ? unsubscribeIssue(client, workspaceSlug, projectId, issueId)
        : subscribeIssue(client, workspaceSlug, projectId, issueId),
    onSuccess: (_row, wasSubscribed) => {
      notifySuccess(wasSubscribed ? UNSUBSCRIBED_TITLE : SUBSCRIBED_TITLE);
      void queryClient
        .invalidateQueries({ queryKey: contextualKeys.subscription(workspaceSlug, projectId, issueId) })
        .then(() => onChanged?.());
    },
    onError: (error: Error) => notifyError(SUBSCRIPTION_FAILED_TITLE, error.message),
  });

  const subscribed = status.data?.subscribed ?? false;
  if (archived) return null;
  if (status.isPending) return <Spinner size="small" label="Checking subscription" />;
  // A refused read leaves the state unknown; no toggle is better than a
  // wrong one, and the next mount retries the read.
  if (status.isError) return null;

  return (
    <Button
      variant="secondary"
      size="small"
      disabled={!canWrite || toggle.isPending}
      onClick={() => toggle.mutate(subscribed)}
    >
      <span className="flex items-center gap-(--space-1)">
        {subscribed ? (
          <BellOff size={14} strokeWidth={1.5} aria-hidden="true" />
        ) : (
          <Bell size={14} strokeWidth={1.5} aria-hidden="true" />
        )}
        <span>{subscribed ? "Unsubscribe" : "Subscribe"}</span>
      </span>
    </Button>
  );
}
