/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { observer } from "mobx-react";
import type { SubmitHandler } from "react-hook-form";
import { Controller, useForm } from "react-hook-form";
import useSWR from "swr";
// pi dash imports
import { API_BASE_URL } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { PodService, RunnerService } from "@pi-dash/services";
import type { IDevMachine, IPod, TPartialProject } from "@pi-dash/types";
import { CustomSelect, EModalPosition, EModalWidth, Input, ModalCore } from "@pi-dash/ui";
// app
import { RunnerCliCommand } from "@/components/runners/runner-cli-command";
import {
  DEFAULT_MODEL_BY_AGENT,
  RUNNER_MODEL_OPTIONS,
  resolveRunnerModel,
  runnerModelLabel,
} from "@/components/runners/runner-models";
import { ProjectService } from "@/services/project";

type Props = {
  isOpen: boolean;
  onClose: () => void;
  workspaceId: string;
  workspaceSlug: string;
};

// Mirrors the runner CLI's ``--agent`` value-enum (kebab-case). Keep in
// sync with runner/src/config/schema.rs:AgentKind.
const AGENT_OPTIONS = ["claude-code", "codex", "cursor-agent", "open-claw", "grok"] as const;
type TAgent = (typeof AGENT_OPTIONS)[number];
const DEFAULT_AGENT: TAgent = "claude-code";
const RUNNER_NAME_WHITESPACE_RE = /\s/;
const RUNNER_NAME_RE = /^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$/;

/** Sentinel devMachineId meaning "show the command, don't auto-create". */
const MANUAL_MACHINE = "";
/** How long the modal waits for the daemon to report back. */
const REMOTE_CREATE_TIMEOUT_MS = 90_000;
const REMOTE_CREATE_POLL_MS = 2_000;

interface FormValues {
  /** Target connected dev machine; `MANUAL_MACHINE` = command panel. */
  devMachineId: string;
  projectIdentifier: string;
  podName: string;
  name: string;
  workingDir: string;
  agent: TAgent;
  /** Selected model-catalog option id (``"default"`` = agent's own default). */
  model: string;
}

type RunnerCommandValues = Pick<FormValues, "projectIdentifier" | "podName" | "name" | "workingDir" | "agent"> & {
  /** Resolved ``--model`` value; undefined for the default sentinel. */
  model?: string;
  /** Resolved ``--reasoning-effort`` value; codex only. */
  reasoningEffort?: string;
};

/** Progress of a cloud-driven creation on a connected dev machine. */
type RemoteCreateState = {
  phase: "creating" | "ok" | "error" | "timeout";
  machineLabel: string;
  runnerName?: string;
  error?: string;
};

const DEFAULT_VALUES: FormValues = {
  devMachineId: MANUAL_MACHINE,
  projectIdentifier: "",
  podName: "",
  name: "",
  workingDir: "",
  agent: DEFAULT_AGENT,
  model: DEFAULT_MODEL_BY_AGENT[DEFAULT_AGENT],
};

const podService = new PodService();
const projectService = new ProjectService();
const runnerService = new RunnerService();

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

const machineDisplayLabel = (machine: IDevMachine): string => machine.label || machine.host_label || machine.id;

export const AddRunnerModal = observer(function AddRunnerModal(props: Props) {
  const { isOpen, onClose, workspaceId, workspaceSlug } = props;
  const { t } = useTranslation();
  const [origin, setOrigin] = useState(() => (typeof window !== "undefined" ? window.location.origin : ""));

  useEffect(() => {
    if (typeof window !== "undefined") setOrigin(window.location.origin);
  }, []);

  const cloudUrl = API_BASE_URL || origin;
  const isUsingBrowserOrigin = !API_BASE_URL;

  const {
    control,
    handleSubmit,
    reset,
    watch,
    setValue,
    formState: { errors },
  } = useForm<FormValues>({ defaultValues: DEFAULT_VALUES });

  const [runnerCommand, setRunnerCommand] = useState<RunnerCommandValues | null>(null);
  const [remoteCreate, setRemoteCreate] = useState<RemoteCreateState | null>(null);
  // Stashed on submit so a failed remote create can fall back to the
  // manual command panel with the same values.
  const [lastSubmitted, setLastSubmitted] = useState<RunnerCommandValues | null>(null);
  // Flipped on close so an in-flight status poll stops writing state
  // into a modal the user already dismissed.
  const pollCancelled = useRef(false);
  // One-shot per open: auto-select the first connected machine once the
  // machine list arrives, unless the user already touched the picker.
  const autoSelectedMachine = useRef(false);

  // Reset everything on close→open. State persists between consecutive
  // opens otherwise (RHF + local command state would carry the stale
  // last-submission and confuse the next user).
  useEffect(() => {
    if (!isOpen) {
      pollCancelled.current = true;
      return;
    }
    pollCancelled.current = false;
    autoSelectedMachine.current = false;
    setRunnerCommand(null);
    setRemoteCreate(null);
    setLastSubmitted(null);
    reset(DEFAULT_VALUES);
  }, [isOpen, reset]);

  // Connected dev machines for the picker. Kept fresh while the modal
  // is open so a machine coming online mid-flow becomes selectable.
  const { data: devMachines, error: devMachinesError } = useSWR<IDevMachine[]>(
    isOpen && workspaceId ? ["dev-machines", workspaceId] : null,
    () => runnerService.listDevMachines(workspaceId),
    { refreshInterval: 15_000 }
  );
  const connectedMachines = useMemo(
    () => (devMachines ?? []).filter((m) => m.control_online && !m.revoked_at),
    [devMachines]
  );
  const selectedDevMachineId = watch("devMachineId");
  const selectedMachine = connectedMachines.find((m) => m.id === selectedDevMachineId);

  // Default the picker to the first connected machine — cloud-driven
  // creation is the preferred path; manual command stays one click away.
  useEffect(() => {
    if (autoSelectedMachine.current || !isOpen) return;
    if (!connectedMachines.length) return;
    autoSelectedMachine.current = true;
    if (!selectedDevMachineId) {
      setValue("devMachineId", connectedMachines[0].id);
    }
  }, [connectedMachines, isOpen, selectedDevMachineId, setValue]);

  // A machine can drop offline between selection and submit; snap the
  // picker back to manual so the button's promise stays honest.
  useEffect(() => {
    if (!selectedDevMachineId || selectedMachine) return;
    setValue("devMachineId", MANUAL_MACHINE);
  }, [selectedDevMachineId, selectedMachine, setValue]);

  // Project picker + pod picker load whenever the modal is open. Pod
  // list re-fetches when the chosen project changes; the picker shows
  // a "(default pod)" sentinel that resolves cloud-side.
  const { data: projects, error: projectsError } = useSWR<TPartialProject[]>(
    isOpen && workspaceSlug ? ["projects-lite", workspaceSlug] : null,
    () => projectService.getProjectsLite(workspaceSlug)
  );
  const selectedProject = watch("projectIdentifier");
  const selectedPodName = watch("podName");
  const { data: pods, error: podsError } = useSWR<IPod[]>(isOpen && workspaceId ? ["pods", workspaceId] : null, () =>
    podService.list(workspaceId)
  );

  // Cascade rules for the pod picker:
  // - If no project is selected yet, show every pod the user can see
  //   (so a pod-first selection is possible).
  // - If a project IS selected, narrow to pods inside that project so
  //   the user can't pick a pod the cloud will reject.
  // Older serializers without ``project_identifier`` fall through to
  // showing every pod — it just means the cascade degrades to "no
  // filter" rather than crashing.
  const podsForPicker = useMemo(() => {
    if (!pods) return [];
    if (!selectedProject) return pods;
    return pods.filter((pod) => !("project_identifier" in pod) || pod.project_identifier === selectedProject);
  }, [pods, selectedProject]);

  // Backfill the project field when the user picks a pod first. Skip
  // when a project is already chosen (don't overwrite their selection),
  // and skip if the pod's project doesn't appear in the projects list
  // (defensive — could happen if the projects fetch is still loading).
  useEffect(() => {
    if (!selectedPodName || selectedProject) return;
    const pod = pods?.find((p) => p.name === selectedPodName);
    if (!pod || !pod.project_identifier) return;
    if (!projects?.some((p) => p.identifier === pod.project_identifier)) return;
    setValue("projectIdentifier", pod.project_identifier, { shouldValidate: true });
  }, [selectedPodName, selectedProject, pods, projects, setValue]);

  // If the user changes project AFTER picking a pod and the pod no
  // longer fits the new project, clear the pod field so the picker
  // doesn't show a value the cloud would reject. The "default pod"
  // sentinel ("") always fits, so leave that alone.
  useEffect(() => {
    if (!selectedProject || !selectedPodName) return;
    const stillFits = podsForPicker.some((p) => p.name === selectedPodName);
    if (!stillFits) setValue("podName", "");
  }, [selectedProject, selectedPodName, podsForPicker, setValue]);

  const agentOptionLabel = (value: TAgent): string => {
    if (value === "claude-code") return t("Claude Code");
    if (value === "cursor-agent") return t("Cursor");
    if (value === "open-claw") return t("OpenClaw");
    if (value === "grok") return t("Grok");
    return t("Codex");
  };

  // Model options are agent-specific, so a stale selection from a previous
  // agent would generate an inapplicable ``--model``. Reset to the default
  // sentinel whenever the agent changes.
  const selectedAgent = watch("agent");
  useEffect(() => {
    setValue("model", DEFAULT_MODEL_BY_AGENT[selectedAgent]);
  }, [selectedAgent, setValue]);

  // Drive the cloud → daemon creation and poll the daemon's result.
  // Transient status-poll failures are retried until the deadline; a
  // deadline without a verdict shows the timeout panel (the runner may
  // still appear — the daemon just didn't report back in time).
  const runRemoteCreate = async (machine: IDevMachine, cmd: RunnerCommandValues) => {
    const machineLabel = machineDisplayLabel(machine);
    setRemoteCreate({ phase: "creating", machineLabel });
    let requestId: string;
    try {
      const resp = await runnerService.createRunnerOnMachine(machine.id, workspaceId, {
        project: cmd.projectIdentifier,
        pod: cmd.podName || undefined,
        name: cmd.name || undefined,
        working_dir: cmd.workingDir || undefined,
        agent: cmd.agent,
        model: cmd.model,
        reasoning_effort: cmd.reasoningEffort,
      });
      requestId = resp.request_id;
    } catch (e: unknown) {
      const code = (e as { error?: string } | undefined)?.error;
      setRemoteCreate({
        phase: "error",
        machineLabel,
        error:
          code === "machine_offline"
            ? t("The dev machine went offline before the command could be delivered.")
            : (code ?? t("Could not reach the cloud to start the runner creation.")),
      });
      return;
    }
    const deadline = Date.now() + REMOTE_CREATE_TIMEOUT_MS;
    while (Date.now() < deadline) {
      if (pollCancelled.current) return;
      // eslint-disable-next-line no-await-in-loop -- deliberate sequential status polling
      await sleep(REMOTE_CREATE_POLL_MS);
      let status;
      try {
        // eslint-disable-next-line no-await-in-loop -- deliberate sequential status polling
        status = await runnerService.getCreateRunnerOnMachineStatus(machine.id, requestId, workspaceId);
      } catch {
        continue; // transient — keep polling until the deadline
      }
      if (status.status === "ok") {
        setRemoteCreate({ phase: "ok", machineLabel, runnerName: status.runner_name });
        return;
      }
      if (status.status === "error") {
        setRemoteCreate({ phase: "error", machineLabel, error: status.error });
        return;
      }
    }
    if (!pollCancelled.current) setRemoteCreate({ phase: "timeout", machineLabel });
  };

  const onSubmit: SubmitHandler<FormValues> = (values) => {
    const { model, reasoningEffort } = resolveRunnerModel(values.agent, values.model);
    const cmd: RunnerCommandValues = {
      projectIdentifier: values.projectIdentifier,
      podName: values.podName,
      name: values.name.trim(),
      workingDir: values.workingDir,
      agent: values.agent,
      model,
      reasoningEffort,
    };
    setLastSubmitted(cmd);
    const machine = connectedMachines.find((m) => m.id === values.devMachineId);
    if (machine) {
      void runRemoteCreate(machine, cmd);
      return;
    }
    setRunnerCommand(cmd);
  };

  // Three layouts: form (before submit), remote-create progress panel
  // (machine-targeted submit), and command panel (manual submit).
  // Splitting them keeps state clean — the panels don't need
  // RHF/Controller wiring.
  return (
    <ModalCore isOpen={isOpen} handleClose={onClose} position={EModalPosition.CENTER} width={EModalWidth.XXL}>
      {remoteCreate !== null ? (
        <div className="flex flex-col gap-4 p-5">
          <div>
            <div className="text-18 font-medium text-primary">
              {remoteCreate.phase === "ok" ? t("Runner created") : t("Add runner")}
            </div>
            {remoteCreate.phase === "creating" && (
              <p className="mt-1 text-13 text-secondary">
                {t("Creating runner on")} <span className="font-medium">{remoteCreate.machineLabel}</span>…{" "}
                {t("Waiting for the dev machine to report back. This usually takes a few seconds.")}
              </p>
            )}
            {remoteCreate.phase === "ok" && (
              <p className="mt-1 text-13 text-secondary">
                {remoteCreate.runnerName ? (
                  <>
                    <code className="text-12">{remoteCreate.runnerName}</code>{" "}
                  </>
                ) : null}
                {t("is now running on")} <span className="font-medium">{remoteCreate.machineLabel}</span>.
              </p>
            )}
            {remoteCreate.phase === "error" && (
              <p className="mt-1 text-13 text-danger-primary">
                {t("Runner creation failed")}
                {remoteCreate.error ? `: ${remoteCreate.error}` : "."}
              </p>
            )}
            {remoteCreate.phase === "timeout" && (
              <p className="mt-1 text-13 text-secondary">
                {t(
                  "The dev machine did not report back in time. The runner may still appear shortly — check the runners list, or run the command manually."
                )}
              </p>
            )}
          </div>

          <div className="flex justify-end gap-2">
            {remoteCreate.phase === "creating" ? (
              <Button variant="secondary" onClick={onClose}>
                {t("Close")}
              </Button>
            ) : remoteCreate.phase === "ok" ? (
              <Button onClick={onClose}>{t("Done")}</Button>
            ) : (
              <>
                <Button variant="secondary" onClick={() => setRemoteCreate(null)}>
                  {t("Back")}
                </Button>
                {lastSubmitted && (
                  <Button
                    onClick={() => {
                      setRunnerCommand(lastSubmitted);
                      setRemoteCreate(null);
                    }}
                  >
                    {t("Show manual command")}
                  </Button>
                )}
              </>
            )}
          </div>
        </div>
      ) : runnerCommand === null ? (
        <form onSubmit={handleSubmit(onSubmit)} className="flex flex-col gap-5 p-5">
          <div>
            <div className="text-18 font-medium text-primary">{t("Add runner")}</div>
            <p className="mt-1 text-13 text-secondary">
              {t(
                "Create a runner on a connected dev machine, or generate a `pidash runner add` command to run manually."
              )}
            </p>
          </div>

          <div className="flex flex-col gap-1">
            <label htmlFor="add-runner-dev-machine" className="text-13 font-medium text-primary">
              {t("Dev machine")}
            </label>
            <Controller
              control={control}
              name="devMachineId"
              render={({ field }) => (
                <CustomSelect
                  value={field.value}
                  label={selectedMachine ? machineDisplayLabel(selectedMachine) : t("Run `pidash runner add` manually")}
                  onChange={field.onChange}
                  buttonClassName="border border-subtle"
                  input
                  maxHeight="lg"
                  placement="bottom-start"
                >
                  <>
                    {connectedMachines.map((machine) => (
                      <CustomSelect.Option key={machine.id} value={machine.id}>
                        {machineDisplayLabel(machine)}
                      </CustomSelect.Option>
                    ))}
                    <CustomSelect.Option value={MANUAL_MACHINE}>
                      {t("Run `pidash runner add` manually")}
                    </CustomSelect.Option>
                  </>
                </CustomSelect>
              )}
            />
            <p className="text-12 text-secondary">
              {connectedMachines.length
                ? t("Connected dev machines can create the runner directly — no copy-paste needed.")
                : t(
                    "No connected dev machines. Install and start the pidash daemon on your machine, or generate the command to run manually."
                  )}
            </p>
            {devMachinesError && (
              <span className="text-12 text-danger-primary">{t("Could not load dev machines.")}</span>
            )}
          </div>

          <div className="flex flex-col gap-1">
            <label htmlFor="add-runner-project" className="text-13 font-medium text-primary">
              {t("Project")}
            </label>
            <Controller
              control={control}
              name="projectIdentifier"
              rules={{ required: t("Pick a project.") }}
              render={({ field }) => (
                <CustomSelect
                  value={field.value}
                  label={projects?.find((p) => p.identifier === field.value)?.name ?? t("Select a project")}
                  onChange={field.onChange}
                  buttonClassName="border border-subtle"
                  input
                  maxHeight="lg"
                  placement="bottom-start"
                  disabled={!projects || projects.length === 0}
                >
                  <>
                    {(projects ?? []).map((p) => (
                      <CustomSelect.Option key={p.id} value={p.identifier}>
                        {p.name}
                      </CustomSelect.Option>
                    ))}
                  </>
                </CustomSelect>
              )}
            />
            <p className="text-12 text-secondary">{t("The project this runner will work on.")}</p>
            {errors.projectIdentifier && (
              <span className="text-12 text-danger-primary">{errors.projectIdentifier.message}</span>
            )}
            {projectsError && <span className="text-12 text-danger-primary">{t("Could not load projects.")}</span>}
          </div>

          <div className="flex flex-col gap-1">
            <label htmlFor="add-runner-pod" className="text-13 font-medium text-primary">
              {t("Pod (optional)")}
            </label>
            <Controller
              control={control}
              name="podName"
              render={({ field }) => (
                <CustomSelect
                  value={field.value}
                  label={field.value || t("(default pod)")}
                  onChange={field.onChange}
                  buttonClassName="border border-subtle"
                  input
                  maxHeight="lg"
                  placement="bottom-start"
                >
                  <>
                    <CustomSelect.Option value="">{t("(default pod)")}</CustomSelect.Option>
                    {podsForPicker.map((pod) => (
                      <CustomSelect.Option key={pod.id} value={pod.name}>
                        {pod.name}
                        {pod.project_identifier ? ` (${pod.project_identifier})` : ""}
                      </CustomSelect.Option>
                    ))}
                  </>
                </CustomSelect>
              )}
            />
            <p className="text-12 text-secondary">{t("Defaults to the project's default pod.")}</p>
            {podsError && <span className="text-12 text-danger-primary">{t("Could not load pods.")}</span>}
          </div>

          <div className="flex flex-col gap-1">
            <label htmlFor="add-runner-name" className="text-13 font-medium text-primary">
              {t("Name (optional)")}
            </label>
            <Controller
              control={control}
              name="name"
              rules={{
                validate: (value) => {
                  const trimmed = value.trim();
                  if (!trimmed) return true;
                  if (RUNNER_NAME_WHITESPACE_RE.test(trimmed))
                    return t(
                      "Runner name cannot contain spaces. It must start with a letter, digit, or underscore and contain only letters, digits, underscore, dot, or dash."
                    );
                  return (
                    RUNNER_NAME_RE.test(trimmed) ||
                    t(
                      "Runner name cannot contain spaces. It must start with a letter, digit, or underscore and contain only letters, digits, underscore, dot, or dash."
                    )
                  );
                },
              }}
              render={({ field }) => <Input {...field} id="add-runner-name" placeholder={t("my-laptop-runner")} />}
            />
            <p className="text-12 text-secondary">
              {t(
                "Auto-assigned if blank. No spaces. If provided, use letters, digits, underscore, dot, or dash; start with a letter, digit, or underscore."
              )}
            </p>
            {errors.name && <span className="text-12 text-danger-primary">{errors.name.message}</span>}
          </div>

          <div className="flex flex-col gap-1">
            <label htmlFor="add-runner-working-dir" className="text-13 font-medium text-primary">
              {t("Working directory (optional)")}
            </label>
            <Controller
              control={control}
              name="workingDir"
              render={({ field }) => (
                <Input
                  {...field}
                  id="add-runner-working-dir"
                  placeholder={t("local dev machine project working dir")}
                />
              )}
            />
            <p className="text-12 text-secondary">
              {t(
                "Local path the daemon runs the agent CLI in — usually the project repo on disk. Defaults to a sandbox under the runner's data dir, which is rarely what you want."
              )}
            </p>
          </div>

          <div className="flex flex-col gap-1">
            <label htmlFor="add-runner-agent" className="text-13 font-medium text-primary">
              {t("Agent")}
            </label>
            <Controller
              control={control}
              name="agent"
              render={({ field }) => (
                <CustomSelect
                  value={field.value}
                  label={agentOptionLabel(field.value)}
                  onChange={(v: TAgent) => field.onChange(v)}
                  buttonClassName="border border-subtle"
                  input
                  maxHeight="lg"
                  placement="bottom-start"
                >
                  <>
                    {AGENT_OPTIONS.map((opt) => (
                      <CustomSelect.Option key={opt} value={opt}>
                        {agentOptionLabel(opt)}
                      </CustomSelect.Option>
                    ))}
                  </>
                </CustomSelect>
              )}
            />
            <p className="text-12 text-secondary">
              {t("Which AI agent CLI this runner will drive. Baked into the displayed ``pidash runner add`` command.")}
            </p>
          </div>

          <div className="flex flex-col gap-1">
            <label htmlFor="add-runner-model" className="text-13 font-medium text-primary">
              {t("Model (optional)")}
            </label>
            <Controller
              control={control}
              name="model"
              render={({ field }) => (
                <CustomSelect
                  value={field.value}
                  label={runnerModelLabel(selectedAgent, field.value)}
                  onChange={field.onChange}
                  buttonClassName="border border-subtle"
                  input
                  maxHeight="lg"
                  placement="bottom-start"
                >
                  <>
                    {RUNNER_MODEL_OPTIONS[selectedAgent].map((opt) => (
                      <CustomSelect.Option key={opt.id} value={opt.id}>
                        {opt.label}
                      </CustomSelect.Option>
                    ))}
                  </>
                </CustomSelect>
              )}
            />
            <p className="text-12 text-secondary">
              {t(
                "Default model this runner's agent uses. ``Default`` lets the agent pick its own; the choice is baked into the displayed ``pidash runner add`` command."
              )}
            </p>
          </div>

          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={onClose}>
              {t("Cancel")}
            </Button>
            <Button type="submit">{t("Generate Runner")}</Button>
          </div>
        </form>
      ) : (
        <div className="flex flex-col gap-4 p-5">
          <div>
            <div className="text-18 font-medium text-primary">{t("Add runner")}</div>
            <p className="mt-1 text-13 text-secondary">
              {t("Project")}: <code className="text-12">{runnerCommand.projectIdentifier}</code>
            </p>
          </div>

          <RunnerCliCommand
            cloudUrl={cloudUrl}
            workspaceSlug={workspaceSlug}
            projectIdentifier={runnerCommand.projectIdentifier}
            podName={runnerCommand.podName}
            name={runnerCommand.name}
            workingDir={runnerCommand.workingDir}
            agent={runnerCommand.agent}
            model={runnerCommand.model}
            reasoningEffort={runnerCommand.reasoningEffort}
            isUsingBrowserOrigin={isUsingBrowserOrigin}
          />

          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={() => setRunnerCommand(null)}>
              {t("Back")}
            </Button>
            <Button onClick={onClose}>{t("Done")}</Button>
          </div>
        </div>
      )}
    </ModalCore>
  );
});
