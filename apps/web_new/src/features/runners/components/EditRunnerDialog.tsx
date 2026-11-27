// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner edit dialog. The name saves on the server (PATCH); the working
// directory, agent and model save on the runner's machine through a
// reconfigure the daemon applies and confirms. The project is shown, never
// edited. Only the changed fields are sent; an untouched form just closes.

import { Button, Combobox, Dialog, Input, type ComboboxOption } from "@pidash/kit";
import { getRunnerReconfigureStatus, reconfigureRunner, type Runner } from "@pidash/api-client";
import { useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { notifySuccess } from "../../../shared/shell/index.js";
import { useUpdateRunner } from "../api/mutations.js";
import { runnersKeys } from "../api/keys.js";
import {
  AGENT_LABELS,
  MODEL_CHOICES,
  RUNNER_AGENTS,
  choicesForAgent,
  selectedModelLabelFrom,
  type ModelChoice,
  type RunnerAgent,
} from "../addRunner/catalog.js";
import { useRunnerModelCatalog } from "../addRunner/queries.js";
import {
  EDIT_RUNNER_AGENT_HINT,
  EDIT_RUNNER_DONE_TITLE,
  EDIT_RUNNER_HINT,
  EDIT_RUNNER_MODEL_UNREPORTED,
  EDIT_RUNNER_SAVING,
  EDIT_RUNNER_SUBMIT,
  EDIT_RUNNER_TIMEOUT,
  EDIT_RUNNER_TITLE,
  choicesWithCurrent,
  machineFieldsBlocker,
  reconfigureErrorText,
  runnerEditPlan,
  runnerReportsModel,
  seedRunnerEdit,
  validateRunnerName,
  type RunnerEditValues,
} from "../edit/model.js";
import { refusalReason, runReconfigure } from "../edit/reconfigure.js";

/** The daemon re-reports its settings once the runner reconnects; read again then. */
const SETTLE_REFRESH_MS = 3_000;

export interface EditRunnerDialogProps {
  workspaceSlug: string;
  /** The runner to edit; null keeps the dialog closed. */
  runner: Runner | null;
  onClose: () => void;
}

function useAgentChoices(): (agent: RunnerAgent) => ModelChoice[] {
  const catalog = useRunnerModelCatalog(true);
  return React.useCallback(
    (agent: RunnerAgent) => {
      const rows = catalog.data?.agents[agent];
      return rows !== undefined ? choicesForAgent(rows) : MODEL_CHOICES[agent];
    },
    [catalog.data]
  );
}

function EditRunnerForm({
  workspaceSlug,
  runner,
  onClose,
  onBusyChange,
}: {
  workspaceSlug: string;
  runner: Runner;
  onClose: () => void;
  onBusyChange: (busy: boolean) => void;
}): React.ReactElement {
  const queryClient = useQueryClient();
  const update = useUpdateRunner(workspaceSlug);
  const baseChoices = useAgentChoices();
  const reported = runner.dev_metadata;
  const choicesFor = React.useCallback(
    (agent: RunnerAgent, seedAgent: RunnerAgent) =>
      agent === seedAgent
        ? choicesWithCurrent(baseChoices(agent), reported.model, reported.reasoning_effort)
        : baseChoices(agent),
    [baseChoices, reported.model, reported.reasoning_effort]
  );
  const [seed] = React.useState<RunnerEditValues>(() => {
    const initial = seedRunnerEdit(runner, []);
    return seedRunnerEdit(runner, choicesFor(initial.agent, initial.agent));
  });
  const [draft, setDraft] = React.useState<RunnerEditValues>(seed);
  const [nameError, setNameError] = React.useState<string | undefined>(undefined);
  const [failure, setFailure] = React.useState<string | null>(null);
  const [saving, setSaving] = React.useState(false);
  const cancelled = React.useRef(false);
  React.useEffect(
    () => () => {
      cancelled.current = true;
    },
    []
  );

  const blocker = machineFieldsBlocker(runner);
  const choices = choicesFor(draft.agent, seed.agent);

  function setField<K extends keyof RunnerEditValues>(key: K, value: RunnerEditValues[K]): void {
    setDraft((current) => ({ ...current, [key]: value }));
    setFailure(null);
  }

  function pickAgent(agent: RunnerAgent): void {
    // A new agent starts on its own default model; the old one would not apply.
    const modelId = agent === seed.agent ? seed.modelId : (choicesFor(agent, seed.agent)[0]?.id ?? "");
    setDraft((current) => ({ ...current, agent, modelId }));
    setFailure(null);
  }

  function refresh(): void {
    void queryClient.invalidateQueries({ queryKey: runnersKeys.all(workspaceSlug) });
  }

  async function save(event: React.FormEvent): Promise<void> {
    event.preventDefault();
    const name = validateRunnerName(draft.name);
    if ("error" in name) {
      setNameError(name.error);
      return;
    }
    const plan = runnerEditPlan(seed, { ...draft, name: name.value }, choices);
    if (plan.name === null && plan.reconfigure === null) {
      onClose();
      return;
    }
    setSaving(true);
    onBusyChange(true);
    try {
      if (plan.name !== null) {
        try {
          await update.mutateAsync({ runnerId: runner.id, patch: { name: plan.name } });
        } catch (error) {
          setFailure(reconfigureErrorText(refusalReason(error)));
          return;
        }
        // A rename alone still reaches the machine: an empty reconfigure
        // pushes the cloud name to the daemon's config. Best effort; the
        // next reconfigure carries the name again.
        if (plan.reconfigure === null && blocker === null)
          void reconfigureRunner(getAppClient(), runner.id, {}).catch(() => undefined);
      }
      if (plan.reconfigure !== null) {
        const client = getAppClient();
        const outcome = await runReconfigure(runner.id, plan.reconfigure, {
          send: (runnerId, changes) => reconfigureRunner(client, runnerId, changes),
          readStatus: (runnerId, requestId) => getRunnerReconfigureStatus(client, runnerId, requestId),
          sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
          now: () => Date.now(),
          isCancelled: () => cancelled.current,
        });
        if (outcome.kind === "cancelled") return;
        if (outcome.kind === "timeout") {
          setFailure(EDIT_RUNNER_TIMEOUT);
          return;
        }
        if (outcome.kind === "error") {
          setFailure(reconfigureErrorText(outcome.reason));
          return;
        }
        refresh();
        setTimeout(refresh, SETTLE_REFRESH_MS);
      }
      notifySuccess(EDIT_RUNNER_DONE_TITLE);
      onClose();
    } finally {
      if (!cancelled.current) setSaving(false);
      onBusyChange(false);
    }
  }

  const machineLocked = blocker !== null || saving;
  const agentOptions: ComboboxOption[] = RUNNER_AGENTS.map((agent) => ({ id: agent, label: AGENT_LABELS[agent] }));
  const modelOptions: ComboboxOption[] = choices.map((choice) => ({ id: choice.id, label: choice.label }));

  return (
    <form className="flex flex-col gap-(--space-5)" noValidate onSubmit={(event) => void save(event)}>
      <p className="text-meta text-(--text-muted)">
        Project: <span className="text-(--text)">{runner.pod_detail.project_identifier}</span>
      </p>
      <Input
        id="edit-runner-name"
        label="Name"
        value={draft.name}
        disabled={saving}
        {...(nameError === undefined ? {} : { error: nameError })}
        onChange={(event) => {
          setField("name", event.target.value);
          setNameError(undefined);
        }}
      />
      {/* While saving, the runner restarting reads as offline; that is expected. */}
      {blocker === null || saving ? null : (
        <p data-testid="edit-runner-blocker" className="text-meta text-(--text-muted)">
          {blocker}
        </p>
      )}
      <Input
        id="edit-runner-working-dir"
        label="Working directory"
        value={draft.workingDir}
        disabled={machineLocked}
        placeholder="e.g. /home/me/src/my-project"
        hint="An absolute path on the runner's machine. It is created if missing and must not overlap another runner's."
        onChange={(event) => setField("workingDir", event.target.value)}
      />
      <Field label="Agent">
        <Picker
          label="Agent"
          value={draft.agent}
          options={agentOptions}
          disabled={machineLocked}
          onChange={(id) => pickAgent(id as RunnerAgent)}
          triggerLabel={AGENT_LABELS[draft.agent]}
        />
        {draft.agent === seed.agent ? null : <p className="text-meta text-(--text-muted)">{EDIT_RUNNER_AGENT_HINT}</p>}
      </Field>
      <Field label="Model">
        <Picker
          label="Model"
          value={draft.modelId}
          options={modelOptions}
          disabled={machineLocked}
          onChange={(id) => setField("modelId", id)}
          triggerLabel={selectedModelLabelFrom(choices, draft.modelId)}
        />
        {runnerReportsModel(runner) ? null : (
          <p className="text-meta text-(--text-muted)">{EDIT_RUNNER_MODEL_UNREPORTED}</p>
        )}
      </Field>
      {failure === null ? null : (
        <p role="alert" data-testid="edit-runner-error" className="text-meta text-(--danger)">
          {failure}
        </p>
      )}
      <div className="flex justify-end gap-(--space-2)">
        <Button variant="ghost" disabled={saving} onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" loading={saving}>
          {saving ? EDIT_RUNNER_SAVING : EDIT_RUNNER_SUBMIT}
        </Button>
      </div>
    </form>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }): React.ReactElement {
  return (
    <div className="flex flex-col gap-(--space-1)">
      <span className="text-body font-medium text-(--text)">{label}</span>
      {children}
    </div>
  );
}

function Picker(props: {
  label: string;
  value: string;
  options: ComboboxOption[];
  onChange: (id: string) => void;
  triggerLabel: string;
  disabled: boolean;
}): React.ReactElement {
  return (
    <Combobox
      trigger={
        <Button
          variant="secondary"
          disabled={props.disabled}
          aria-label={props.label}
          className="w-full justify-between"
        >
          {props.triggerLabel}
        </Button>
      }
      options={props.options}
      value={props.value}
      onSelect={props.onChange}
      listLabel={props.label}
      searchEnabled={props.options.length > 6}
      emptyText="No options"
      disabled={props.disabled}
      className="w-full"
    />
  );
}

/**
 * The form freezes its seed on mount; wait for the catalog so the seeded
 * model id matches the choices the pickers will offer. Mounted only while
 * the dialog is open, so a closed dialog never fetches.
 */
function EditRunnerBody(props: {
  workspaceSlug: string;
  runner: Runner;
  onClose: () => void;
  onBusyChange: (busy: boolean) => void;
}): React.ReactElement | null {
  const catalog = useRunnerModelCatalog(true);
  if (catalog.isPending === true) return null;
  return <EditRunnerForm {...props} />;
}

export function EditRunnerDialog({ workspaceSlug, runner, onClose }: EditRunnerDialogProps): React.ReactElement {
  const [busy, setBusy] = React.useState(false);
  return (
    <Dialog
      open={runner !== null}
      onOpenChange={(next) => {
        if (!next && !busy) onClose();
      }}
      title={EDIT_RUNNER_TITLE}
      description={EDIT_RUNNER_HINT}
      nonDismissable={busy}
      bodyClassName="mt-(--space-4)"
    >
      {runner === null ? null : (
        <EditRunnerBody
          key={runner.id}
          workspaceSlug={workspaceSlug}
          runner={runner}
          onClose={onClose}
          onBusyChange={setBusy}
        />
      )}
    </Dialog>
  );
}
