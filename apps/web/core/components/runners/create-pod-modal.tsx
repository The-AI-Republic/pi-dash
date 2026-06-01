/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect } from "react";
import { observer } from "mobx-react";
import type { SubmitHandler } from "react-hook-form";
import { Controller, useForm } from "react-hook-form";
import useSWR from "swr";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { PodService } from "@pi-dash/services";
import type { IPod, TPartialProject } from "@pi-dash/types";
import { CustomSelect, EModalPosition, EModalWidth, Input, ModalCore } from "@pi-dash/ui";
import { ProjectService } from "@/services/project";

type Props = {
  isOpen: boolean;
  onClose: () => void;
  workspaceSlug: string;
  onCreated: (pod: IPod) => void;
};

interface FormValues {
  projectId: string;
  name: string;
  description: string;
}

const DEFAULT_VALUES: FormValues = {
  projectId: "",
  name: "",
  description: "",
};

const podService = new PodService();
const projectService = new ProjectService();

export const CreatePodModal = observer(function CreatePodModal(props: Props) {
  const { isOpen, onClose, workspaceSlug, onCreated } = props;
  const { t } = useTranslation();

  const {
    control,
    handleSubmit,
    reset,
    formState: { errors, isSubmitting },
  } = useForm<FormValues>({ defaultValues: DEFAULT_VALUES });

  useEffect(() => {
    if (!isOpen) return;
    reset(DEFAULT_VALUES);
  }, [isOpen, reset]);

  const { data: projects, error: projectsError } = useSWR<TPartialProject[]>(
    isOpen && workspaceSlug ? ["projects-lite", workspaceSlug] : null,
    () => projectService.getProjectsLite(workspaceSlug)
  );

  const onSubmit: SubmitHandler<FormValues> = async (values) => {
    try {
      const pod = await podService.create({
        project: values.projectId,
        name: values.name.trim(),
        description: values.description.trim() || undefined,
      });
      onCreated(pod);
      onClose();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Error!"),
        message: err?.error ?? t("Could not create the pod."),
      });
    }
  };

  return (
    <ModalCore isOpen={isOpen} handleClose={onClose} position={EModalPosition.CENTER} width={EModalWidth.XXL}>
      <form onSubmit={handleSubmit(onSubmit)} className="flex flex-col gap-5 p-5">
        <div>
          <div className="text-18 font-medium text-primary">{t("Create new pod")}</div>
          <p className="mt-1 text-13 text-secondary">{t("Pods group runners under a project. Pick a project, then give the pod a name.")}</p>
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="create-pod-project" className="text-13 font-medium text-primary">
            {t("Project")}
          </label>
          <Controller
            control={control}
            name="projectId"
            rules={{ required: t("Pick a project.") }}
            render={({ field }) => (
              <CustomSelect
                value={field.value}
                label={
                  projects?.find((p) => p.id === field.value)?.name ?? t("Select a project")
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
                    <CustomSelect.Option key={p.id} value={p.id}>
                      {p.name}
                    </CustomSelect.Option>
                  ))}
                </>
              </CustomSelect>
            )}
          />
          <p className="text-12 text-secondary">{t("The project this pod belongs to. The name will be prefixed with the project identifier.")}</p>
          {errors.projectId && <span className="text-red-500 text-12">{errors.projectId.message}</span>}
          {projectsError && (
            <span className="text-red-500 text-12">{t("Could not load projects.")}</span>
          )}
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="create-pod-name" className="text-13 font-medium text-primary">
            {t("Name")}
          </label>
          <Controller
            control={control}
            name="name"
            rules={{
              validate: (v) => v.trim().length > 0 || t("Name is required."),
            }}
            render={({ field }) => (
              <Input {...field} id="create-pod-name" placeholder={t("beefy")} />
            )}
          />
          <p className="text-12 text-secondary">{t("Letters, digits, dashes, and underscores. The project prefix is added automatically.")}</p>
          {errors.name && <span className="text-red-500 text-12">{errors.name.message}</span>}
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="create-pod-description" className="text-13 font-medium text-primary">
            {t("Description (optional)")}
          </label>
          <Controller
            control={control}
            name="description"
            render={({ field }) => (
              <Input
                {...field}
                id="create-pod-description"
                placeholder={t("Where this pod runs, what it's for, etc.")}
              />
            )}
          />
        </div>

        <div className="flex justify-end gap-2">
          <Button variant="secondary" onClick={onClose} disabled={isSubmitting}>
            {t("Cancel")}
          </Button>
          <Button type="submit" loading={isSubmitting} disabled={isSubmitting}>
            {isSubmitting ? t("Creating…") : t("Create pod")}
          </Button>
        </div>
      </form>
    </ModalCore>
  );
});
