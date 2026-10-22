// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project cycles list screen (parity/specs/cycles.md §1 and §5,
// CYC-010–012). Reads the project row for the feature gate, the live
// cycle collection for the list, and the role map for the shortcut
// gating; sets the tab title; and composes exactly one body state. No
// list fetch runs while the gate shows, and a list refetch keeps its
// rows instead of flashing the skeleton or the zero-state.

import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useProjectRoles, useProjectRow } from "../../auth/index.js";
import { isProjectAdmin, isProjectAuthorized } from "../../projects/index.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import { cyclesListQueryOptions } from "../api/queries.js";
import { CYCLES_CRUMB, cyclesFeatureHref, cyclesTitle, resolveCyclesBody } from "../lib/cycleScreen.js";
import { CyclesFeatureGate } from "./CyclesFeatureGate.js";
import { CyclesListSkeleton } from "./CyclesListSkeleton.js";
import { CyclesLoadError } from "./CyclesLoadError.js";
import { CyclesZeroState } from "./CyclesZeroState.js";

export interface CyclesListScreenProps {
  workspaceSlug: string;
  projectId: string;
  /** Opens the create dialog. Slot owner: NEWFRONT-710. */
  onCreateCycle?: (() => void) | undefined;
  /** The hero plus groups body. Slot owner: NEWFRONT-708. */
  children?: React.ReactNode;
}

export function CyclesListScreen({
  workspaceSlug,
  projectId,
  onCreateCycle,
  children,
}: CyclesListScreenProps): React.ReactElement {
  const navigate = useNavigate();
  const project = useProjectRow(workspaceSlug, projectId);
  const gateOff = project.data?.cycle_view === false;
  const cycles = useQuery({
    ...cyclesListQueryOptions(getAppClient(), workspaceSlug, projectId),
    enabled: project.isSuccess && !gateOff,
  });
  const roles = useProjectRoles(workspaceSlug);
  const role = roles.data?.[projectId] ?? null;

  useDocumentHead({
    title: project.data === undefined ? CYCLES_CRUMB : cyclesTitle(project.data.name),
    robots: "noindex, nofollow",
  });

  const state = resolveCyclesBody({
    projectPending: project.isPending,
    projectError: project.isError,
    gateOff,
    cyclesPending: cycles.isPending,
    cyclesError: cycles.isError,
    cycleCount: cycles.data?.length ?? 0,
  });

  return (
    <section
      aria-label={CYCLES_CRUMB}
      data-testid="cycles-list-screen"
      className="flex min-h-0 flex-1 flex-col gap-(--space-4) p-(--space-4)"
    >
      {state === "skeleton" ? (
        <CyclesListSkeleton />
      ) : state === "error" ? (
        <CyclesLoadError
          onRetry={() => {
            void project.refetch();
            void cycles.refetch();
          }}
        />
      ) : state === "gate" ? (
        <CyclesFeatureGate
          isAdmin={isProjectAdmin(role)}
          onManageFeatures={() => {
            void navigate({ to: cyclesFeatureHref(workspaceSlug, projectId) });
          }}
        />
      ) : state === "zero" ? (
        <CyclesZeroState canCreate={isProjectAuthorized(role)} onCreateCycle={onCreateCycle} />
      ) : (
        children
      )}
    </section>
  );
}
