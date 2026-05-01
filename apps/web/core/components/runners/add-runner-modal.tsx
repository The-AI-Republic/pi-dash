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
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { PodService, RunnerService } from "@pi-dash/services";
import type { IPod, IRunnerInvite, TPartialProject } from "@pi-dash/types";
import { CustomSelect, EModalPosition, EModalWidth, Input, ModalCore } from "@pi-dash/ui";
// app
import { ProjectService } from "@/services/project";

type Props = {
  isOpen: boolean;
  onClose: () => void;
  workspaceId: string;
  workspaceSlug: string;
  /** Refetch the runners list so the new pending row shows up. */
  onCreated: () => void;
};

interface FormValues {
  projectIdentifier: string;
  podName: string;
  name: string;
  hostLabel: string;
  workingDir: string;
}

const DEFAULT_VALUES: FormValues = {
  projectIdentifier: "",
  podName: "",
  name: "",
  hostLabel: "",
  workingDir: "",
};

const runnerService = new RunnerService();
const podService = new PodService();
const projectService = new ProjectService();

export const AddRunnerModal = observer(function AddRunnerModal(props: Props) {
  const { isOpen, onClose, workspaceId, workspaceSlug, onCreated } = props;
  const { t } = useTranslation();

  const {
    control,
    handleSubmit,
    reset,
    watch,
    setValue,
    formState: { errors, isSubmitting },
  } = useForm<FormValues>({ defaultValues: DEFAULT_VALUES });

  const [invite, setInvite] = useState<IRunnerInvite | null>(null);
  const [origin, setOrigin] = useState("");
  const [justCopied, setJustCopied] = useState(false);

  // Reset everything on close→open. State persists between consecutive
  // opens otherwise (RHF + local invite state would carry the stale
  // last-submission and confuse the next user).
  useEffect(() => {
    if (!isOpen) return;
    setInvite(null);
    setJustCopied(false);
    reset(DEFAULT_VALUES);
    if (typeof window !== "undefined") setOrigin(window.location.origin);
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

  const apiOrigin = API_BASE_URL || origin;

  const enrollmentCommand = useMemo(() => {
    if (!invite) return "";
    const hostLabelArg = watch("hostLabel").trim();
    const workingDirArg = watch("workingDir").trim();
    const lines = [`pidash connect \\`, `  --url ${apiOrigin} \\`, `  --token ${invite.enrollment_token}`];
    // host-label / working-dir are CLI-only flags; they don't go to the
    // invite endpoint. Append each on its own continuation line so the
    // generated command stays readable and copy-pasteable.
    const optionalArgs: string[] = [];
    if (hostLabelArg) optionalArgs.push(`  --host-label ${hostLabelArg}`);
    if (workingDirArg) optionalArgs.push(`  --working-dir ${workingDirArg}`);
    if (optionalArgs.length > 0) {
      lines[lines.length - 1] += " \\";
      for (let i = 0; i < optionalArgs.length; i += 1) {
        const isLast = i === optionalArgs.length - 1;
        lines.push(isLast ? optionalArgs[i] : `${optionalArgs[i]} \\`);
      }
    }
    return lines.join("\n");
  }, [invite, apiOrigin, watch]);

  const onSubmit: SubmitHandler<FormValues> = async (values) => {
    try {
      const result = await runnerService.createRunnerInvite({
        workspaceId,
        projectIdentifier: values.projectIdentifier,
        podName: values.podName || undefined,
        name: values.name.trim() || undefined,
      });
      setInvite(result);
      onCreated();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("runners.toast.error_title"),
        message: err?.error ?? t("runners.add_modal.errors.create_failed"),
      });
    }
  };

  const copyCommand = async () => {
    if (!enrollmentCommand) return;
    try {
      await navigator.clipboard.writeText(enrollmentCommand);
      setJustCopied(true);
      window.setTimeout(() => setJustCopied(false), 2000);
    } catch {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("runners.toast.error_title"),
        message: t("runners.list.copy_failed"),
      });
    }
  };

  // Two layouts: form (before submit) and command panel (after).
  // Splitting them keeps state clean — the command panel doesn't
  // need RHF/Controller wiring.
  return (
    <ModalCore isOpen={isOpen} handleClose={onClose} position={EModalPosition.CENTER} width={EModalWidth.XXL}>
      {invite === null ? (
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
            <label htmlFor="add-runner-host-label" className="text-13 font-medium text-primary">
              {t("runners.add_modal.host_label_label")}
            </label>
            <Controller
              control={control}
              name="hostLabel"
              render={({ field }) => (
                <Input
                  {...field}
                  id="add-runner-host-label"
                  placeholder={t("runners.add_modal.host_label_placeholder")}
                />
              )}
            />
            <p className="text-12 text-secondary">{t("runners.add_modal.host_label_help")}</p>
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

          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={onClose} disabled={isSubmitting}>
              {t("runners.add_modal.cancel")}
            </Button>
            <Button type="submit" loading={isSubmitting} disabled={isSubmitting}>
              {isSubmitting ? t("runners.add_modal.submitting") : t("runners.add_modal.submit")}
            </Button>
          </div>
        </form>
      ) : (
        <div className="flex flex-col gap-4 p-5">
          <div>
            <div className="text-18 font-medium text-primary">{t("runners.add_modal.title")}</div>
            <p className="mt-1 text-13 text-secondary">
              {t("runners.add_modal.runner_id_label")}: <code className="text-12">{invite.runner_id}</code>
              <br />
              {invite.name}
            </p>
          </div>

          <div className="border-amber-500/40 bg-amber-500/10 rounded border p-3 text-13 text-primary">
            <div className="font-medium">{t("runners.add_modal.token_warning")}</div>
            <p className="mt-2 text-secondary">{t("runners.add_modal.token_instructions")}</p>
            <pre className="font-mono mt-1 rounded border border-subtle bg-layer-1 p-2 text-11 whitespace-pre-wrap text-primary select-all">
              {enrollmentCommand}
            </pre>
            <div className="mt-2">
              <Button size="sm" onClick={copyCommand}>
                {justCopied ? t("runners.add_modal.copied") : t("runners.add_modal.copy_command")}
              </Button>
            </div>
          </div>

          <div className="flex justify-end">
            <Button onClick={onClose}>{t("runners.add_modal.done")}</Button>
          </div>
        </div>
      )}
    </ModalCore>
  );
});
