// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-settings AI Workers tab (hybrid assignment, design §6.6).
// One form owns the lost-runner rule and the run policy (NEWPIDASH-261):
// what the project does with a dedicated issue when its runner is
// revoked or deleted, and who may move an issue's state or run it.
// Project admins edit; members read the stored rules on a dimmed,
// inert form. The run policy is not the states-tab workflow switch
// (`members_can_edit_states`): that controls editing the workflow
// definition, this controls moving and running a particular issue.
import { Button, Combobox, Skeleton } from "@pidash/kit";
import type { ProjectSettingsDetail } from "@pidash/api-client";
import { ChevronDown } from "lucide-react";
import * as React from "react";

import { useProjectSettings } from "../components/ProjectSettingsLayout.js";
import { useGeneralDetail } from "../general/api/queries.js";
import { isAdminStanding } from "../tabs.js";
import { useSaveLostRunnerRule } from "./api/mutations.js";
import {
  buildAiWorkersPayload,
  ISSUE_RUN_POLICY_OPTIONS,
  ISSUE_RUN_POLICY_ORDER,
  issueRunPolicyDescription,
  issueRunPolicyFrom,
  issueRunPolicyLabel,
  LOST_RUNNER_RULE_OPTIONS,
  LOST_RUNNER_RULE_ORDER,
  lostRunnerRuleDescription,
  lostRunnerRuleFrom,
  lostRunnerRuleLabel,
  type IssueRunPolicyValue,
  type LostRunnerRule,
} from "./rules.js";

export interface AiWorkersTabProps {
  workspaceSlug: string;
  projectId: string;
}

/** Save control copy at rest and while the write is in flight. */
export const SAVE_LABEL = "Save changes";
export const SAVING_LABEL = "Saving";

/**
 * The rule form, rendered once the stored row resolves. A stored row
 * arriving after the first render wins over the draft; a save in
 * flight never resets the form. The save's own reseed is skipped too:
 * the PATCH response does not carry the field yet (PDASHOSS01-297/304),
 * so the seeded row would read back the default over the just-saved
 * pick (PSET-064).
 */
function AiWorkersTabBody({
  workspaceSlug,
  projectId,
  stored,
  canEdit,
}: AiWorkersTabProps & { stored: ProjectSettingsDetail; canEdit: boolean }): React.ReactElement {
  const [value, setValue] = React.useState<LostRunnerRule>(() =>
    lostRunnerRuleFrom(stored.dedicated_runner_lost_action)
  );
  const [policyValue, setPolicyValue] = React.useState<IssueRunPolicyValue>(() =>
    issueRunPolicyFrom(stored.settings?.issue_run_policy)
  );
  const save = useSaveLostRunnerRule(workspaceSlug, projectId);
  const lastStored = React.useRef(stored);
  const justSaved = React.useRef(false);
  React.useEffect(() => {
    if (lastStored.current === stored || save.isPending) return;
    lastStored.current = stored;
    if (justSaved.current) {
      justSaved.current = false;
      return;
    }
    setValue(lostRunnerRuleFrom(stored.dedicated_runner_lost_action));
    setPolicyValue(issueRunPolicyFrom(stored.settings?.issue_run_policy));
  }, [stored, save.isPending]);

  function submit(event: React.FormEvent<HTMLFormElement>): void {
    event.preventDefault();
    if (!canEdit || save.isPending) return;
    save.mutate(
      buildAiWorkersPayload(stored.dedicated_runner_lost_action, value, stored.settings?.issue_run_policy, policyValue),
      {
        onSuccess: () => {
          justSaved.current = true;
        },
      }
    );
  }

  const formClass = canEdit ? "flex flex-col gap-(--space-6)" : "flex flex-col gap-(--space-6) opacity-60";
  return (
    <form onSubmit={submit} className={formClass} aria-label="AI Workers settings">
      <section aria-label="Dedicated runners" className="flex flex-col gap-(--space-2)">
        <span className="text-body font-medium text-(--text)">When a dedicated runner is revoked or deleted</span>
        <Combobox
          trigger={
            <Button variant="secondary" disabled={!canEdit} className="w-full justify-between">
              <span>{lostRunnerRuleLabel(value)}</span>
              <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
            </Button>
          }
          options={LOST_RUNNER_RULE_ORDER.map((rule) => ({
            id: rule,
            label: LOST_RUNNER_RULE_OPTIONS[rule].label,
          }))}
          value={value}
          onSelect={(id) => setValue(lostRunnerRuleFrom(id))}
          listLabel="Lost-runner rule"
          searchEnabled={false}
          emptyText="No rules"
          disabled={!canEdit}
        />
        <p className="text-caption text-(--text-muted)">{lostRunnerRuleDescription(value)}</p>
      </section>
      <section aria-label="Who can run issues" className="flex flex-col gap-(--space-2)">
        <span className="text-body font-medium text-(--text)">Who can run issues</span>
        <Combobox
          trigger={
            <Button variant="secondary" disabled={!canEdit} className="w-full justify-between">
              <span>{issueRunPolicyLabel(policyValue)}</span>
              <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
            </Button>
          }
          options={ISSUE_RUN_POLICY_ORDER.map((policy) => ({
            id: policy,
            label: ISSUE_RUN_POLICY_OPTIONS[policy].label,
          }))}
          value={policyValue}
          onSelect={(id) => setPolicyValue(issueRunPolicyFrom(id))}
          listLabel="Who can run issues"
          searchEnabled={false}
          emptyText="No options"
          disabled={!canEdit}
        />
        <p className="text-caption text-(--text-muted)">{issueRunPolicyDescription(policyValue)}</p>
      </section>
      {canEdit ? (
        <div className="flex justify-end">
          <Button type="submit" loading={save.isPending}>
            {save.isPending ? SAVING_LABEL : SAVE_LABEL}
          </Button>
        </div>
      ) : null}
    </form>
  );
}

/** Skeleton shaped like the two fields and save control. */
export function AiWorkersSkeleton(): React.ReactElement {
  return (
    <div data-testid="project-settings-ai-workers-skeleton" className="flex flex-col gap-(--space-6)">
      <div className="flex flex-col gap-(--space-2)">
        <Skeleton className="h-4 w-64" />
        <Skeleton className="h-(--control-height-large) w-full" />
      </div>
      <div className="flex flex-col gap-(--space-2)">
        <Skeleton className="h-4 w-64" />
        <Skeleton className="h-(--control-height-large) w-full" />
      </div>
      <Skeleton className="h-(--control-height) w-32 self-end" />
    </div>
  );
}

/**
 * The tab. Reads the shared settings row behind a skeleton; members
 * read the stored rule, admins edit it.
 */
export function ProjectSettingsAiWorkersTab({ workspaceSlug, projectId }: AiWorkersTabProps): React.ReactElement {
  const { role } = useProjectSettings();
  const detail = useGeneralDetail(workspaceSlug, projectId);

  if (detail.data === undefined) return <AiWorkersSkeleton />;
  return (
    <AiWorkersTabBody
      workspaceSlug={workspaceSlug}
      projectId={projectId}
      stored={detail.data}
      canEdit={isAdminStanding(role)}
    />
  );
}
