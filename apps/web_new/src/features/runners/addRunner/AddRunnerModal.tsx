// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Add-runner dialog (RUN-006–009, RUN-049). Two creation paths share one
// form: drive a connected dev machine (create + poll) or generate a manual
// enrolment command. The form is deliberately local state: every reopen
// starts clean, and a failed remote create can carry the same values into
// the command panel.

import { Button, Combobox, Dialog, Input, type ComboboxOption } from "@pidash/kit";
import type { CreateRunnerStatus, DevMachine } from "@pidash/api-client";
import * as React from "react";

import { useRunnersArea } from "../area.js";
import {
  AGENT_LABELS,
  MODEL_CHOICES,
  modelContribution,
  RUNNER_AGENTS,
  selectedModelLabel,
  type RunnerAgent,
} from "./catalog.js";
import { AddRunnerCommandPanel } from "./CommandPanel.js";
import { resolveCommandOrigin, type CommandFields } from "./command.js";
import {
  agentGating,
  autoSelectMachine,
  connectedMachines,
  DEFAULT_POD,
  DEFAULT_POD_LABEL,
  defaultModelFor,
  emptyValues,
  machineLabel,
  MANUAL_MACHINE,
  MANUAL_MACHINE_LABEL,
  podOptionLabel,
  podsForProject,
  projectForPod,
  settleAgent,
  settleMachineSelection,
  settlePodSelection,
  validateValues,
  type AddRunnerErrors,
  type AddRunnerValues,
  type ConnectedMachine,
  type RemoteState,
} from "./model.js";
import { createRunnerTransport, useAddRunnerMachines, useAddRunnerPods, useAddRunnerProjects } from "./queries.js";
import { runRemoteCreate } from "./remoteCreate.js";

/** Build-time API base, baked by the harness as `VITE_API_BASE_URL`. */
declare const __PIDASH_API_BASE_URL__: string | undefined;

/** The configured API base, or null when the build carries none. */
export function configuredApiBase(): string | null {
  if (typeof __PIDASH_API_BASE_URL__ !== "string") return null;
  const trimmed = __PIDASH_API_BASE_URL__.trim();
  return trimmed === "" ? null : trimmed;
}

function browserOrigin(): string {
  return globalThis.window.location.origin;
}

export interface AddRunnerModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Prefill and lock the project picker (project-scoped entry). */
  projectId?: string | undefined;
}

type Screen = { kind: "form" } | { kind: "remote"; state: RemoteState } | { kind: "command"; fields: CommandFields };

const DIALOG_BODY = "flex flex-col gap-(--space-4)";

export function AddRunnerModal({ open, onOpenChange, projectId }: AddRunnerModalProps): React.ReactElement {
  const area = useRunnersArea();
  const workspaceSlug = area.scope.workspaceSlug;
  const scopeProjectId = area.scope.projectId ?? null;
  const projectLocked = projectId !== undefined;
  const origin = browserOrigin();
  const resolvedOrigin = React.useMemo(() => resolveCommandOrigin(configuredApiBase(), origin), [origin]);

  const [values, setValues] = React.useState<AddRunnerValues>(emptyValues);
  const [errors, setErrors] = React.useState<AddRunnerErrors>({});
  const [screen, setScreen] = React.useState<Screen>({ kind: "form" });
  const [lastFields, setLastFields] = React.useState<CommandFields | null>(null);
  const [machineTouched, setMachineTouched] = React.useState(false);
  const cancelled = React.useRef(false);
  const prefilled = React.useRef(false);

  const machines = useAddRunnerMachines(workspaceSlug, area.workspaceId, open);
  const projects = useAddRunnerProjects(workspaceSlug, open);
  const pods = useAddRunnerPods(workspaceSlug, area.workspaceId, scopeProjectId, open);

  const connected = React.useMemo(() => connectedMachines(machines.data), [machines.data]);
  const connectedForPicker = React.useMemo<ConnectedMachine[]>(
    () =>
      connected.map((machine) => ({
        id: machine.id,
        label: machineLabel(machine),
        supportedAgents: supportedOf(machine),
      })),
    [connected]
  );
  const selectedMachine = connectedForPicker.find((machine) => machine.id === values.devMachineId);
  const gating = agentGating(selectedMachine);

  // Fresh form per open: reset every field, the screen, and both one-shot refs.
  React.useEffect(() => {
    if (!open) {
      cancelled.current = true;
      return;
    }
    cancelled.current = false;
    prefilled.current = false;
    setValues(emptyValues());
    setErrors({});
    setScreen({ kind: "form" });
    setLastFields(null);
    setMachineTouched(false);
  }, [open]);

  // One-shot machine auto-selection once the list arrives.
  React.useEffect(() => {
    if (!open) return;
    const picked = autoSelectMachine(values.devMachineId, connectedForPicker, machineTouched);
    if (picked !== null) setValues((current) => ({ ...current, devMachineId: picked }));
  }, [connectedForPicker, machineTouched, open, values.devMachineId]);

  // A chosen machine that drops offline falls back to manual.
  React.useEffect(() => {
    if (!open) return;
    const settled = settleMachineSelection(values.devMachineId, connectedForPicker);
    if (settled !== values.devMachineId) setValues((current) => ({ ...current, devMachineId: settled }));
  }, [connectedForPicker, open, values.devMachineId]);

  // Prefill the locked project from the route once the list arrives.
  React.useEffect(() => {
    if (!open || prefilled.current || projectId === undefined) return;
    const match = projects.data?.find((project) => project.id === projectId);
    if (match === undefined) return;
    prefilled.current = true;
    setValues((current) => ({ ...current, projectIdentifier: match.identifier }));
  }, [open, projectId, projects.data]);

  // Pod-first selection backfills the project; a project change clears a pod
  // that no longer fits.
  React.useEffect(() => {
    if (!open) return;
    if (values.projectIdentifier === "" && values.podName !== DEFAULT_POD) {
      const project = projectForPod(values.podName, pods.data, projects.data);
      if (project !== null) setValues((current) => ({ ...current, projectIdentifier: project }));
    }
  }, [open, pods.data, projects.data, values.podName, values.projectIdentifier]);

  React.useEffect(() => {
    if (!open) return;
    const settledPod = settlePodSelection(values.podName, values.projectIdentifier, pods.data);
    if (settledPod !== values.podName) setValues((current) => ({ ...current, podName: settledPod }));
  }, [open, pods.data, values.podName, values.projectIdentifier]);

  // Agent change resets the model so a stale choice never leaks across agents.
  const [lastAgent, setLastAgent] = React.useState<RunnerAgent>(values.agent);
  React.useEffect(() => {
    if (values.agent === lastAgent) return;
    setLastAgent(values.agent);
    setValues((current) => ({ ...current, model: defaultModelFor(values.agent) }));
  }, [lastAgent, values.agent]);

  function setField<K extends keyof AddRunnerValues>(key: K, value: AddRunnerValues[K]): void {
    setValues((current) => ({ ...current, [key]: value }));
    if (key === "projectIdentifier" || key === "name") {
      setErrors((current) => ({ ...current, [key]: undefined }));
    }
  }

  function pickMachine(machineId: string): void {
    setMachineTouched(true);
    const machine = connectedForPicker.find((candidate) => candidate.id === machineId);
    setValues((current) => ({
      ...current,
      devMachineId: machineId,
      agent: settleAgent(current.agent, machine),
    }));
  }

  function fieldsFor(V: AddRunnerValues): CommandFields {
    const contribution = modelContribution(V.agent, V.model);
    return {
      cloudUrl: resolvedOrigin.url,
      workspaceSlug,
      projectIdentifier: V.projectIdentifier,
      podName: V.podName,
      runnerName: V.name.trim(),
      workingDir: V.workingDir,
      agent: V.agent,
      model: contribution.model,
      reasoningEffort: contribution.reasoningEffort,
    };
  }

  function submit(): void {
    const found = validateValues(values);
    if (found.projectIdentifier !== undefined || found.name !== undefined) {
      setErrors(found);
      return;
    }
    const fields = fieldsFor(values);
    setLastFields(fields);
    const machine = connectedForPicker.find((candidate) => candidate.id === values.devMachineId);
    if (machine !== undefined) {
      void runRemote(machine);
      return;
    }
    setScreen({ kind: "command", fields });
  }

  async function runRemote(machine: ConnectedMachine): Promise<void> {
    setScreen({ kind: "remote", state: { phase: "creating", machineLabel: machine.label } });
    const transport = createRunnerRequest();
    await runRemoteCreate(
      { machineId: machine.id, machineLabel: machine.label },
      {
        createRunner: () => transport.create(machine.id),
        readStatus: () => transport.status(machine.id),
        sleep: (ms) => new Promise<void>((resolve) => setTimeout(resolve, ms)),
        now: () => Date.now(),
        isCancelled: () => cancelled.current,
        update: (state) => setScreen({ kind: "remote", state }),
      }
    );
  }

  /** POST then GET helpers bound to the current form values and workspace. */
  function createRunnerRequest(): {
    create: (machineId: string) => Promise<{ request_id: string }>;
    status: (machineId: string) => Promise<CreateRunnerStatus>;
  } {
    const fields = fieldsFor(values);
    const transport = createRunnerTransport();
    let requestId = "";
    const input = {
      workspace: area.workspaceId,
      project: fields.projectIdentifier,
      ...(fields.podName ? { pod: fields.podName } : {}),
      ...(fields.runnerName ? { name: fields.runnerName } : {}),
      ...(fields.workingDir ? { working_dir: fields.workingDir } : {}),
      agent: fields.agent,
      ...(fields.model === undefined ? {} : { model: fields.model }),
      ...(fields.reasoningEffort === undefined ? {} : { reasoning_effort: fields.reasoningEffort }),
    };
    return {
      create: async (machineId) => {
        const accepted = await transport.createRunner(machineId, input);
        requestId = accepted.request_id;
        return accepted;
      },
      status: (machineId) => transport.readStatus(machineId, requestId, area.workspaceId),
    };
  }

  function close(): void {
    cancelled.current = true;
    onOpenChange(false);
  }

  function remoteBack(): void {
    setScreen({ kind: "form" });
  }

  function showManual(): void {
    setScreen({ kind: "command", fields: lastFields as CommandFields });
  }

  function commandBack(): void {
    setScreen({ kind: "form" });
  }

  return (
    <Dialog
      title={screen.kind === "remote" && screen.state.phase === "ok" ? "Runner created" : "Add runner"}
      open={open}
      onOpenChange={(next) => {
        if (!next) cancelled.current = true;
        onOpenChange(next);
      }}
      bodyClassName={DIALOG_BODY}
    >
      {screen.kind === "remote" ? (
        <RemotePanel state={screen.state} onClose={close} onDone={close} onBack={remoteBack} onManual={showManual} />
      ) : screen.kind === "command" ? (
        <div className="flex flex-col gap-(--space-4)">
          <p className="text-body text-(--text-muted)">
            Project: <code className="text-meta">{screen.fields.projectIdentifier}</code>
          </p>
          <AddRunnerCommandPanel fields={screen.fields} originFallback={resolvedOrigin.fellBack} />
          <div className="flex justify-end gap-(--space-2)">
            <Button variant="secondary" onClick={commandBack}>
              Back
            </Button>
            <Button onClick={close}>Done</Button>
          </div>
        </div>
      ) : (
        <FormPanel
          values={values}
          errors={errors}
          projectLocked={projectLocked}
          projectOptions={(projects.data ?? []).map((project) => ({ id: project.identifier, label: project.name }))}
          projectsError={projects.isError}
          podOptions={[
            { id: DEFAULT_POD, label: DEFAULT_POD_LABEL },
            ...podsForProject(pods.data, values.projectIdentifier).map((pod) => ({
              id: pod.name,
              label: podOptionLabel(pod),
            })),
          ]}
          podsError={pods.isError}
          machineOptions={[
            ...connectedForPicker.map((machine) => ({ id: machine.id, label: machine.label })),
            { id: MANUAL_MACHINE, label: MANUAL_MACHINE_LABEL },
          ]}
          machinesError={machines.isError}
          onField={setField}
          onPickMachine={pickMachine}
          gating={gating}
          onCancel={close}
          onSubmit={submit}
        />
      )}
    </Dialog>
  );
}

function supportedOf(machine: DevMachine): readonly string[] | undefined {
  return machine.supported_agents;
}

interface FormPanelProps {
  values: AddRunnerValues;
  errors: AddRunnerErrors;
  projectLocked: boolean;
  projectOptions: ComboboxOption[];
  projectsError: boolean;
  podOptions: ComboboxOption[];
  podsError: boolean;
  machineOptions: ComboboxOption[];
  machinesError: boolean;
  gating: ReturnType<typeof agentGating>;
  onField: <K extends keyof AddRunnerValues>(key: K, value: AddRunnerValues[K]) => void;
  onPickMachine: (id: string) => void;
  onCancel: () => void;
  onSubmit: () => void;
}

function FormPanel({
  values,
  errors,
  projectLocked,
  projectOptions,
  projectsError,
  podOptions,
  podsError,
  machineOptions,
  machinesError,
  gating,
  onField,
  onPickMachine,
  onCancel,
  onSubmit,
}: FormPanelProps): React.ReactElement {
  const machineLabelText = machineOptions.find((option) => option.id === values.devMachineId)?.label;
  const projectLabel =
    projectOptions.find((option) => option.id === values.projectIdentifier)?.label ?? "Select a project";
  const podLabel = podOptions.find((option) => option.id === values.podName)?.label ?? DEFAULT_POD_LABEL;
  const agentOptions: ComboboxOption[] = RUNNER_AGENTS.map((agent) => ({
    id: agent,
    label: AGENT_LABELS[agent],
    disabled: gating.disabled.includes(agent),
  }));
  const modelOptions: ComboboxOption[] = MODEL_CHOICES[values.agent].map((choice) => ({
    id: choice.id,
    label: choice.label,
  }));

  return (
    <div className="flex flex-col gap-(--space-4)">
      <p className="text-body text-(--text-muted)">
        Create a runner on a connected dev machine, or generate a command to run manually.
      </p>

      <Field label="Dev machine">
        <Picker
          label="Dev machine"
          value={values.devMachineId}
          options={machineOptions}
          onChange={onPickMachine}
          triggerLabel={machineLabelText === undefined ? MANUAL_MACHINE_LABEL : machineLabelText}
        />
        <p className="text-meta text-(--text-muted)">
          {machineOptions.length > 1
            ? "Connected dev machines can create the runner directly — no copy-paste needed."
            : "No connected dev machines. Install and start the pidash daemon on your machine, or generate the command to run manually."}
        </p>
        {machinesError ? <p className="text-meta text-(--danger)">Could not load dev machines.</p> : null}
      </Field>

      <Field label="Project">
        <Picker
          label="Project"
          value={values.projectIdentifier}
          options={projectOptions}
          onChange={(id) => onField("projectIdentifier", id)}
          triggerLabel={projectLabel}
          disabled={projectLocked || projectOptions.length === 0}
          emptyText="No projects available"
        />
        <p className="text-meta text-(--text-muted)">The project this runner will work on.</p>
        {errors.projectIdentifier === undefined ? null : (
          <p role="alert" className="text-meta text-(--danger)">
            {errors.projectIdentifier}
          </p>
        )}
        {projectsError ? <p className="text-meta text-(--danger)">Could not load projects.</p> : null}
      </Field>

      <Field label="Pod (optional)">
        <Picker
          label="Pod (optional)"
          value={values.podName}
          options={podOptions}
          onChange={(id) => onField("podName", id)}
          triggerLabel={podLabel}
          emptyText="No pods available"
        />
        <p className="text-meta text-(--text-muted)">Defaults to the project&apos;s default pod.</p>
        {podsError ? <p className="text-meta text-(--danger)">Could not load pods.</p> : null}
      </Field>

      <Input
        id="add-runner-name"
        label="Name (optional)"
        value={values.name}
        placeholder="my-laptop-runner"
        onChange={(event) => onField("name", event.target.value)}
        {...(errors.name === undefined ? {} : { error: errors.name })}
        hint="Auto-assigned if blank. No spaces. If provided, use letters, digits, underscore, dot, or dash; start with a letter, digit, or underscore."
      />

      <Input
        id="add-runner-working-dir"
        label="Working directory (optional)"
        value={values.workingDir}
        placeholder="local dev machine project working dir"
        onChange={(event) => onField("workingDir", event.target.value)}
        hint="Local path the daemon runs the agent CLI in — usually the project repo on disk. Defaults to a sandbox under the runner's data dir, which is rarely what you want."
      />

      <Field label="Agent">
        <Picker
          label="Agent"
          value={values.agent}
          options={agentOptions}
          onChange={(id) => onField("agent", id as RunnerAgent)}
          triggerLabel={AGENT_LABELS[values.agent]}
        />
        <p className="text-meta text-(--text-muted)">
          Which AI agent CLI this runner will drive. Baked into the generated command.
        </p>
        {gating.hint === null ? null : <p className="text-meta text-(--text-muted)">{gating.hint}</p>}
      </Field>

      <Field label="Model (optional)">
        <Picker
          label="Model (optional)"
          value={values.model}
          options={modelOptions}
          onChange={(id) => onField("model", id)}
          triggerLabel={selectedModelLabel(values.agent, values.model)}
        />
        <p className="text-meta text-(--text-muted)">
          Default model this runner&apos;s agent uses. The sentinel lets the agent pick its own.
        </p>
      </Field>

      <div className="flex justify-end gap-(--space-2)">
        <Button variant="secondary" onClick={onCancel}>
          Cancel
        </Button>
        <Button variant="primary" onClick={onSubmit}>
          Generate Runner
        </Button>
      </div>
    </div>
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

interface PickerProps {
  label: string;
  value: string;
  options: ComboboxOption[];
  onChange: (id: string) => void;
  triggerLabel: string;
  disabled?: boolean;
  emptyText?: string;
}

function Picker({
  label,
  value,
  options,
  onChange,
  triggerLabel,
  disabled = false,
  emptyText = "No options",
}: PickerProps): React.ReactElement {
  return (
    <Combobox
      trigger={
        <Button
          variant="secondary"
          disabled={disabled}
          aria-label={label}
          className="w-full justify-between"
          data-picker={label}
        >
          {triggerLabel}
        </Button>
      }
      options={options}
      value={value}
      onSelect={onChange}
      listLabel={label}
      searchEnabled={options.length > 6}
      emptyText={emptyText}
      disabled={disabled}
      className="w-full"
    />
  );
}

interface RemotePanelProps {
  state: RemoteState;
  onClose: () => void;
  onDone: () => void;
  onBack: () => void;
  onManual: () => void;
}

function RemotePanel({ state, onClose, onDone, onBack, onManual }: RemotePanelProps): React.ReactElement {
  return (
    <div className="flex flex-col gap-(--space-4)">
      <div>
        {state.phase === "creating" ? (
          <p className="text-body text-(--text-muted)">
            Creating runner on <span className="font-medium text-(--text)">{state.machineLabel}</span>. Waiting for the
            dev machine to report back. This usually takes a few seconds.
          </p>
        ) : null}
        {state.phase === "ok" ? (
          <p className="text-body text-(--text-muted)">
            {state.runnerName === undefined ? null : <code className="text-meta">{state.runnerName} </code>}
            is now running on <span className="font-medium text-(--text)">{state.machineLabel}</span>.
          </p>
        ) : null}
        {state.phase === "error" ? (
          <p className="text-body text-(--danger)">
            Runner creation failed{state.error === undefined || state.error === "" ? "." : `: ${state.error}`}
          </p>
        ) : null}
        {state.phase === "timeout" ? (
          <p className="text-body text-(--text-muted)">
            The dev machine did not report back in time. The runner may still appear shortly — check the runners list,
            or run the command manually.
          </p>
        ) : null}
      </div>
      <div className="flex justify-end gap-(--space-2)">
        {state.phase === "creating" ? (
          <Button variant="secondary" onClick={onClose}>
            Close
          </Button>
        ) : state.phase === "ok" ? (
          <Button variant="primary" onClick={onDone}>
            Done
          </Button>
        ) : (
          <>
            <Button variant="secondary" onClick={onBack}>
              Back
            </Button>
            <Button onClick={onManual}>Show manual command</Button>
          </>
        )}
      </div>
    </div>
  );
}
