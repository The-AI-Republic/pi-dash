/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import { FormProvider, useForm } from "react-hook-form";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
// components
import ProjectCommonAttributes from "@/components/project/create/common-attributes";
import ProjectCreateHeader from "@/components/project/create/header";
import ProjectCreateButtons from "@/components/project/create/project-create-buttons";
// hooks
import { useProject } from "@/hooks/store/use-project";
import { usePlatformOS } from "@/hooks/use-platform-os";
// pi dash web types
import type { TProject } from "@/pi-dash-web/types/projects";
import { ProjectAttributes } from "./attributes";
import { getProjectFormValues } from "./utils";

export type TCreateProjectFormProps = {
  setToFavorite?: boolean;
  workspaceSlug: string;
  onClose: () => void;
  handleNextStep: (projectId: string) => void;
  data?: Partial<TProject>;
  templateId?: string;
};

export const CreateProjectForm = observer(function CreateProjectForm(props: TCreateProjectFormProps) {
  const { setToFavorite, workspaceSlug, data, onClose, handleNextStep } = props;
  // store
  const { t } = useTranslation();
  const { addProjectToFavorites, createProject } = useProject();
  // states
  const [shouldAutoSyncIdentifier, setShouldAutoSyncIdentifier] = useState(true);
  // form info
  const methods = useForm<TProject>({
    defaultValues: { ...getProjectFormValues(), ...data },
    reValidateMode: "onChange",
  });
  const { handleSubmit, reset, setValue } = methods;
  const { isMobile } = usePlatformOS();
  const handleAddToFavorites = (projectId: string) => {
    if (!workspaceSlug) return;

    addProjectToFavorites(workspaceSlug.toString(), projectId).catch(() => {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Error!"),
        message: t("Couldn't remove the project from favorites. Please try again."),
      });
    });
  };

  const onSubmit = async (formData: Partial<TProject>) => {
    // Upper case identifier
    formData.identifier = formData.identifier?.toUpperCase();

    try {
      const res = await createProject(workspaceSlug.toString(), formData);
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: t("Success"),
        message: t("Project created successfully"),
      });
      if (setToFavorite) {
        handleAddToFavorites(res.id);
      }
      handleNextStep(res.id);
    } catch (err) {
      try {
        // Handle the new error format where codes are nested in arrays under field names
        const errorData = (err as { data?: Record<string, string[] | undefined> })?.data ?? {};

        const nameError = errorData.name?.includes("PROJECT_NAME_ALREADY_EXIST");
        const identifierError = errorData?.identifier?.includes("PROJECT_IDENTIFIER_ALREADY_EXIST");

        if (nameError || identifierError) {
          if (nameError) {
            setToast({
              type: TOAST_TYPE.ERROR,
              title: t("Error!"),
              message: t("The project name is already taken."),
            });
          }

          if (identifierError) {
            setToast({
              type: TOAST_TYPE.ERROR,
              title: t("Error!"),
              message: t("The project identifier is already taken."),
            });
          }
        } else {
          setToast({
            type: TOAST_TYPE.ERROR,
            title: t("Error!"),
            message: t("Something went wrong"),
          });
        }
      } catch (error) {
        // Fallback error handling if the error processing fails
        console.error("Error processing API error:", error);
        setToast({
          type: TOAST_TYPE.ERROR,
          title: t("Error!"),
          message: t("Something went wrong"),
        });
      }
    }
  };

  const handleClose = () => {
    onClose();
    setShouldAutoSyncIdentifier(true);
    setTimeout(() => {
      reset();
    }, 300);
  };

  return (
    <FormProvider {...methods}>
      <ProjectCreateHeader handleClose={handleClose} isMobile={isMobile} />

      <form onSubmit={handleSubmit(onSubmit)} className="px-3">
        <div className="mt-4 space-y-6 pb-5">
          <ProjectCommonAttributes
            setValue={setValue}
            isMobile={isMobile}
            shouldAutoSyncIdentifier={shouldAutoSyncIdentifier}
            setShouldAutoSyncIdentifier={setShouldAutoSyncIdentifier}
          />
          <ProjectAttributes isMobile={isMobile} />
        </div>
        <ProjectCreateButtons handleClose={handleClose} />
      </form>
    </FormProvider>
  );
});
