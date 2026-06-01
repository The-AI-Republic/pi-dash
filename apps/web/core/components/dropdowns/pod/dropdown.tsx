/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useRef } from "react";
import useSWR from "swr";
// pi dash imports
import { PodService } from "@pi-dash/services";
import type { IPod } from "@pi-dash/types";
// local imports
import type { TPodDropdownBaseProps } from "./base";
import { PodDropdownBase } from "./base";

const podService = new PodService();

type TPodDropdownProps = Omit<TPodDropdownBaseProps, "pods" | "isInitializing"> & {
  projectId: string | undefined;
  /** When true (work-item creation), pre-select the project's default pod so
   * every new issue is pinned to a concrete pod rather than left to the
   * server-side fallback. */
  isForWorkItemCreation?: boolean;
};

export function PodDropdown(props: TPodDropdownProps) {
  const { projectId, isForWorkItemCreation, value, onChange } = props;
  // Pods are project-scoped; key the cache by project so switching the issue's
  // project refetches the right list.
  const { data: pods, isLoading } = useSWR<IPod[]>(
    projectId ? ["issue-pods", projectId] : null,
    projectId ? () => podService.list(undefined, projectId) : null
  );
  // Pre-select the default pod exactly once on create, only while the field is
  // still empty (never clobber a user's explicit pick or an existing issue).
  const didPreselect = useRef(false);
  useEffect(() => {
    if (!isForWorkItemCreation || didPreselect.current || value || !pods?.length) return;
    const defaultPod = pods.find((pod) => pod.is_default) ?? pods[0];
    if (defaultPod) {
      didPreselect.current = true;
      onChange(defaultPod.id);
    }
  }, [isForWorkItemCreation, value, pods, onChange]);

  return <PodDropdownBase {...props} pods={pods ?? []} isInitializing={isLoading} />;
}
