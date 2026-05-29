/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useMemo, useState } from "react";
import { observer } from "mobx-react";
import type { SubmitHandler } from "react-hook-form";
import { Controller, useForm } from "react-hook-form";
import useSWR from "swr";
// pi dash imports
import { API_BASE_URL } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { PodService } from "@pi-dash/services";
import type { IPod, TPartialProject } from "@pi-dash/types";
import { CustomSelect, EModalPosition, EModalWidth, Input, ModalCore } from "@pi-dash/ui";
// app
import { RunnerCliCommand } from "@/components/runners/runner-cli-command";
import { ProjectService } from "@/services/project";

type Props = {
  isOpen: boolean;
  onClose: () => void;
  workspaceId: string;
  workspaceSlug: string;
};

// Mirrors the runner CLI's ``--agent`` value-enum (kebab-case). Keep in
// sync with runner/src/config/schema.rs:AgentKind.
const AGENT_OPTIONS = ["claude-code", "codex"] as const;
type TAgent = (typeof AGENT_OPTIONS)[number];
const DEFAULT_AGENT: TAgent = "claude-code";

interface FormValues {
  projectIdentifier: string;
  podName: string;
  name: string;
  workingDir: string;
  agent: TAgent;
}

type RunnerCommandValues = Pick<FormValues, "projectIdentifier" | "podName" | "name" | "workingDir" | "agent">;

const DEFAULT_VALUES: FormValues = {
  projectIdentifier: "",
  podName: "",
  name: "",
  workingDir: "",
  agent: DEFAULT_AGENT,
};

const podService = new PodService();
const projectService = new ProjectService();

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

  // Reset everything on close→open. State persists between consecutive
  // opens otherwise (RHF + local command state would carry the stale
  // last-submission and confuse the next user).
  useEffect(() => {
    if (!isOpen) return;
    setRunnerCommand(null);
    reset(DEFAULT_VALUES);
  }, [isOpen, reset]);

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

  const agentOptionLabel = (value: TAgent): string =>
    value === "claude-code"
      ? t("runners.add_modal.agent_options.claude_code")
      : t("runners.add_modal.agent_options.codex");

  const onSubmit: SubmitHandler<FormValues> = (values) => {
    setRunnerCommand({
      projectIdentifier: values.projectIdentifier,
      podName: values.podName,
      name: values.name,
      workingDir: values.workingDir,
      agent: values.agent,
    });
  };

  // Two layouts: form (before submit) and command panel (after).
  // Splitting them keeps state clean — the command panel doesn't
  // need RHF/Controller wiring.
  return (
    <ModalCore isOpen={isOpen} handleClose={onClose} position={EModalPosition.CENTER} width={EModalWidth.XXL}>
      {runnerCommand === null ? (
        <form onSubmit={handleSubmit(onSubmit)} className="flex flex-col gap-5 p-5">
          <div>
            <div className="text-18 font-medium text-primary">{t("runners.add_modal.title")}</div>
            <p className="mt-1 text-13 text-secondary">{t("runners.add_modal.subtitle")}</p>
          </div>

          <div className="flex flex-col gap-1">
            <label htmlFor="add-runner-project" className="text-13 font-medium text-primary">
              {t("runners.add_modal.project_label")}
            </label>
            <Controller
              control={control}
              name="projectIdentifier"
              rules={{ required: t("runners.add_modal.errors.project_required") }}
              render={({ field }) => (
                <CustomSelect
                  value={field.value}
                  label={
                    projects?.find((p) => p.identifier === field.value)?.name ?? t("runners.list.project_placeholder")
                  }
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
            <p className="text-12 text-secondary">{t("runners.add_modal.project_help")}</p>
            {errors.projectIdentifier && (
              <span className="text-red-500 text-12">{errors.projectIdentifier.message}</span>
            )}
            {projectsError && (
              <span className="text-red-500 text-12">{t("runners.add_modal.errors.load_projects_failed")}</span>
            )}
          </div>

          <div className="flex flex-col gap-1">
            <label htmlFor="add-runner-pod" className="text-13 font-medium text-primary">
              {t("runners.add_modal.pod_label")}
            </label>
            <Controller
              control={control}
              name="podName"
              render={({ field }) => (
                <CustomSelect
                  value={field.value}
                  label={field.value || t("runners.add_modal.pod_default_option")}
                  onChange={field.onChange}
                  buttonClassName="border border-subtle"
                  input
                  maxHeight="lg"
                  placement="bottom-start"
                >
                  <>
                    <CustomSelect.Option value="">{t("runners.add_modal.pod_default_option")}</CustomSelect.Option>
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
            <p className="text-12 text-secondary">{t("runners.add_modal.pod_help")}</p>
            {podsError && (
              <span className="text-red-500 text-12">{t("runners.add_modal.errors.load_pods_failed")}</span>
            )}
          </div>

          <div className="flex flex-col gap-1">
            <label htmlFor="add-runner-name" className="text-13 font-medium text-primary">
              {t("runners.add_modal.name_label")}
            </label>
            <Controller
              control={control}
              name="name"
              render={({ field }) => (
                <Input {...field} id="add-runner-name" placeholder={t("runners.add_modal.name_placeholder")} />
              )}
            />
            <p className="text-12 text-secondary">{t("runners.add_modal.name_help")}</p>
          </div>

          <div className="flex flex-col gap-1">
            <label htmlFor="add-runner-working-dir" className="text-13 font-medium text-primary">
              {t("runners.add_modal.working_dir_label")}
            </label>
            <Controller
              control={control}
              name="workingDir"
              render={({ field }) => (
                <Input
                  {...field}
                  id="add-runner-working-dir"
                  placeholder={t("runners.add_modal.working_dir_placeholder")}
                />
              )}
            />
            <p className="text-12 text-secondary">{t("runners.add_modal.working_dir_help")}</p>
          </div>

          <div className="flex flex-col gap-1">
            <label htmlFor="add-runner-agent" className="text-13 font-medium text-primary">
              {t("runners.add_modal.agent_label")}
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
            <p className="text-12 text-secondary">{t("runners.add_modal.agent_help")}</p>
          </div>

          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={onClose}>
              {t("runners.add_modal.cancel")}
            </Button>
            <Button type="submit">{t("runners.add_modal.submit")}</Button>
          </div>
        </form>
      ) : (
        <div className="flex flex-col gap-4 p-5">
          <div>
            <div className="text-18 font-medium text-primary">{t("runners.add_modal.title")}</div>
            <p className="mt-1 text-13 text-secondary">
              {t("runners.add_modal.project_label")}: <code className="text-12">{runnerCommand.projectIdentifier}</code>
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
            isUsingBrowserOrigin={isUsingBrowserOrigin}
          />

          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={() => setRunnerCommand(null)}>
              {t("runners.add_modal.back")}
            </Button>
            <Button onClick={onClose}>{t("runners.add_modal.done")}</Button>
          </div>
        </div>
      )}
    </ModalCore>
  );
});
