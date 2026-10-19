// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project automations page (parity/specs/agents.md §8, AGT-034–037).
// A project admin sees the two built-in idle rows and, below them, the
// edition extension slot — empty in the community build. Everyone else
// gets the not-authorized view. Every row change funnels through one
// handler; a refused write toasts the same generic notice and the rows
// keep showing the server's stored values, because they render from the
// project cache and only a successful write folds the answered row back.

import { Skeleton } from "@pidash/kit";
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { ROLE_ADMIN } from "../../../core/session/permissions.js";
import { useProjectRoles, useWorkspaceMembership } from "../../auth/index.js";
import { isProjectAdmin, projectDetailsQueryOptions } from "../../projects/index.js";
import { notifyError } from "../../../shared/shell/index.js";
import {
  AUTOMATION_UPDATE_FAILED_MESSAGE,
  AUTOMATION_UPDATE_FAILED_TITLE,
  AUTOMATIONS_NOT_AUTHORIZED_TITLE,
  automationStatesQueryOptions,
  useUpdateAutomation,
  type AutomationPatch,
  type ProjectDetails,
  type StateList,
} from "../api/automationQueries.js";
import { AutomationRows } from "./AutomationRows.js";

export const AUTOMATIONS_HEADING = "Automations";
export const AUTOMATIONS_INTRO =
  "Hand routine housekeeping to the project: finished and dormant items move on without manual upkeep.";
export const AUTOMATIONS_LOADING_LABEL = "Loading automations";
export const AUTOMATIONS_UNAVAILABLE_MESSAGE = "We could not load these settings. Check your connection and try again.";
export const AUTOMATIONS_EXTENSION_SLOT = "automations-extension";

export interface AutomationsSettingsProps {
  workspaceSlug: string;
  projectId: string;
  /**
   * Edition contributions rendered below the built-in rows. The
   * community build passes nothing, so the slot stays an empty mount
   * point (AGT-037).
   */
  extensions?: React.ReactNode;
}

export function AutomationsSettings({
  workspaceSlug,
  projectId,
  extensions,
}: AutomationsSettingsProps): React.ReactElement {
  const roles = useProjectRoles(workspaceSlug);
  const membership = useWorkspaceMembership(workspaceSlug);
  const projects = useQuery(projectDetailsQueryOptions(getAppClient(), workspaceSlug));
  const project = projects.data?.find((row) => row.id === projectId);
  const states = useQuery(automationStatesQueryOptions(getAppClient(), workspaceSlug, projectId));
  const update = useUpdateAutomation(workspaceSlug);

  const standingPending = roles.isPending || membership.isPending;
  // Admin standing needs evidence from both reads: the project role must
  // succeed, and the membership read must confirm the caller is in the
  // workspace. A failed read therefore refuses the page rather than
  // handing out enabled controls; a known member or guest who also failed
  // the membership read stays refused.
  // A workspace admin holds project-admin standing where the project row
  // says member or guest, exactly like the old page's permission store.
  const admin =
    roles.isSuccess &&
    membership.isSuccess &&
    (isProjectAdmin(roles.data?.[projectId] ?? null) || (membership.data?.role ?? 0) >= ROLE_ADMIN);
  // Rows write whole-field automation values: the close toggle needs the
  // cancelled-state list to name its target, and switching on needs the
  // stored row. Until both the project collection and the state list have
  // answered, hold the rows in their loading shape rather than letting an
  // early click write a null target or a value over an unknown stored one.
  const rowsPending = projects.isPending || states.isPending;
  const rowsFailed = projects.isError || states.isError;

  const change = React.useCallback(
    (patch: AutomationPatch) => {
      update.mutate(
        { projectId, patch },
        {
          onError: () => notifyError(AUTOMATION_UPDATE_FAILED_TITLE, AUTOMATION_UPDATE_FAILED_MESSAGE),
        }
      );
    },
    [projectId, update]
  );

  if (standingPending) {
    return <AutomationsSkeleton />;
  }

  if (!admin) {
    return <NotAuthorizedView />;
  }

  if (rowsFailed) {
    return <AutomationsUnavailable />;
  }

  return (
    <AutomationsScreen
      project={project}
      projectPending={rowsPending}
      states={states.data ?? []}
      onChange={change}
      extensions={extensions}
    />
  );
}

export interface AutomationsScreenProps {
  project: ProjectDetails | undefined;
  projectPending: boolean;
  states: StateList;
  onChange: (patch: AutomationPatch) => void;
  extensions: React.ReactNode;
}

export function AutomationsScreen({
  project,
  projectPending,
  states,
  onChange,
  extensions,
}: AutomationsScreenProps): React.ReactElement {
  return (
    <section aria-label={AUTOMATIONS_HEADING} data-testid="automations-settings" className="flex flex-col">
      <h2 className="text-h3 text-(--text)">{AUTOMATIONS_HEADING}</h2>
      <p className="text-body mt-(--space-1) text-(--text-muted)">{AUTOMATIONS_INTRO}</p>
      <div className="mt-(--space-6)">
        {projectPending ? (
          <div role="status" aria-label={AUTOMATIONS_LOADING_LABEL}>
            <RowSkeleton />
            <RowSkeleton />
          </div>
        ) : (
          <AutomationRows project={project} states={states} onChange={onChange} />
        )}
      </div>
      <div data-testid={AUTOMATIONS_EXTENSION_SLOT}>{extensions}</div>
    </section>
  );
}

function RowSkeleton(): React.ReactElement {
  return (
    <div className="flex flex-col gap-(--space-2) border-b border-(--border) py-(--space-4)">
      <Skeleton width="40%" height="16px" />
      <Skeleton width="70%" height="12px" />
    </div>
  );
}

function AutomationsSkeleton(): React.ReactElement {
  return (
    <section aria-label={AUTOMATIONS_HEADING} role="status" aria-busy="true" className="flex flex-col">
      <RowSkeleton />
      <RowSkeleton />
    </section>
  );
}

function AutomationsUnavailable(): React.ReactElement {
  return (
    <section
      aria-label={AUTOMATIONS_HEADING}
      data-testid="automations-unavailable"
      className="flex flex-col gap-(--space-4)"
    >
      <h2 className="text-h3 text-(--text)">{AUTOMATIONS_HEADING}</h2>
      <p role="alert" className="text-body text-(--text-muted)">
        {AUTOMATIONS_UNAVAILABLE_MESSAGE}
      </p>
    </section>
  );
}

export function NotAuthorizedView(): React.ReactElement {
  return (
    <section
      aria-label="Automations"
      data-testid="automations-not-authorized"
      className="flex flex-col gap-(--space-4)"
    >
      <h1 className="text-h2 text-(--text)">{AUTOMATIONS_NOT_AUTHORIZED_TITLE}</h1>
      <p className="text-body text-(--text-muted)">Only project admins can change these settings.</p>
    </section>
  );
}
