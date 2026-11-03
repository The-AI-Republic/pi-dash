// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Execution pod for one install (parity/specs/agents.md §4, AGT-019).
// The empty value is the project default, which resolves at fire time.
// The list is the project's pods, default first; a saved pod that no
// longer exists reads as the default and is reported back as such, so a
// save never hands the server a pod id the project can no longer resolve.

import { listPods, type ApiClient, type Pod } from "@pidash/api-client";
import { queryOptions, useQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../core/api/client.js";
import { LIST_STALE_TIME_MS, workspaceScope } from "../../core/query/client.js";

export const DEFAULT_POD_VALUE = "";

export function podKeys(workspaceSlug: string, projectId: string) {
  return [...workspaceScope(workspaceSlug), "scheduler-pods", projectId] as const;
}

export function projectPodsQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: podKeys(workspaceSlug, projectId),
    queryFn: () => listPods(client, { projectId }),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** Labels read "Name (default)" for the project's default pod. */
export function podLabel(pod: Pod): string {
  return pod.is_default ? `${pod.name} (default)` : pod.name;
}

/** The stored value, or the default when the saved pod is gone. */
export function selectedPodValue(pod: string | null, pods: readonly Pod[]): string {
  return pod !== null && pods.some((candidate) => candidate.id === pod) ? pod : DEFAULT_POD_VALUE;
}

export interface PodFieldProps {
  workspaceSlug: string;
  projectId: string;
  value: string | null;
  disabled?: boolean;
  onChange: (pod: string | null) => void;
}

export function PodField({
  workspaceSlug,
  projectId,
  value,
  disabled = false,
  onChange,
}: PodFieldProps): React.ReactElement {
  const pods = useQuery(projectPodsQueryOptions(getAppClient(), workspaceSlug, projectId));
  const rows = pods.data ?? [];
  const selected = selectedPodValue(value, rows);
  const busy = disabled || pods.isPending;
  const report = React.useRef(onChange);
  report.current = onChange;
  const stale = pods.isSuccess && value !== null && selected === DEFAULT_POD_VALUE;
  React.useEffect(() => {
    // Once the project's pods are known, a stored id that is not among
    // them can never fire again; tell the form it is the default so the
    // next save does not send a dead id the server rejects.
    if (stale) report.current(null);
  }, [stale]);
  return (
    <div className="flex flex-col gap-(--space-2)">
      <label htmlFor={`scheduler-pod-${projectId}`} className="text-body font-medium text-(--text)">
        Pod
      </label>
      <select
        id={`scheduler-pod-${projectId}`}
        aria-label="Pod"
        value={selected}
        disabled={busy}
        className="text-body h-(--control-height) w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-(--text) disabled:cursor-not-allowed disabled:opacity-50"
        onChange={(event) => onChange(event.target.value === DEFAULT_POD_VALUE ? null : event.target.value)}
      >
        <option value={DEFAULT_POD_VALUE}>Project default pod</option>
        {rows.map((pod) => (
          <option key={pod.id} value={pod.id}>
            {podLabel(pod)}
          </option>
        ))}
      </select>
      <p className="text-meta text-(--text-muted)">
        {pods.isError
          ? "The project's pods could not be loaded. Runs fall back to the project default."
          : "The runner pool this install's runs land on."}
      </p>
    </div>
  );
}
