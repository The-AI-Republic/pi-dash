/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect } from "react";
import { observer } from "mobx-react";
import type { SubmitHandler } from "react-hook-form";
import { Controller, useForm } from "react-hook-form";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import type { IScheduler } from "@pi-dash/services";
import { EModalPosition, EModalWidth, ModalCore, ToggleSwitch } from "@pi-dash/ui";
import { SCHEDULER_COLOR_PALETTE as COLOR_PALETTE } from "@/components/project/scheduler-bindings/constants";
import { SchedulerDefinitionFields } from "./scheduler-definition-fields";

interface SchedulerFormValues {
  slug: string;
  name: string;
  description: string;
  prompt: string;
  color: string;
  is_enabled: boolean;
}

type Props = {
  isOpen: boolean;
  onClose: () => void;
  onSubmit: (values: SchedulerFormValues) => Promise<void>;
  /** When set, the form is in edit mode — slug is locked. */
  scheduler?: IScheduler | null;
};

const emptyValues: SchedulerFormValues = {
  slug: "",
  name: "",
  description: "",
  prompt: "",
  color: COLOR_PALETTE[0],
  is_enabled: true,
};

export const SchedulerFormModal = observer(function SchedulerFormModal(props: Props) {
  const { isOpen, onClose, onSubmit, scheduler } = props;
  const isEdit = !!scheduler;
  const { t } = useTranslation();

  const {
    control,
    handleSubmit,
    reset,
    formState: { errors, isSubmitting },
  } = useForm<SchedulerFormValues>({ defaultValues: emptyValues });

  useEffect(() => {
    if (!isOpen) return;
    if (scheduler) {
      reset({
        slug: scheduler.slug,
        name: scheduler.name,
        description: scheduler.description ?? "",
        prompt: scheduler.prompt,
        color: scheduler.color || COLOR_PALETTE[0],
        is_enabled: scheduler.is_enabled,
      });
    } else {
      reset(emptyValues);
    }
  }, [isOpen, scheduler, reset]);

  const handleFormSubmit: SubmitHandler<SchedulerFormValues> = async (values) => {
    await onSubmit(values);
  };

  return (
    <ModalCore isOpen={isOpen} handleClose={onClose} position={EModalPosition.CENTER} width={EModalWidth.XXL}>
      <form onSubmit={handleSubmit(handleFormSubmit)} className="flex flex-col gap-5 p-5">
        <div className="text-18 font-medium text-primary">{isEdit ? t("Edit scheduler") : t("New scheduler")}</div>

        <SchedulerDefinitionFields
          control={control}
          errors={errors}
          nameName="name"
          slugName="slug"
          descriptionName="description"
          promptName="prompt"
          colorName="color"
          slugDisabled={isEdit}
        />

        <div className="flex items-start justify-between gap-4">
          <div className="flex flex-col">
            <span className="text-13 font-medium text-primary">{t("Enabled")}</span>
            <span className="text-12 text-secondary">
              {t("Disabled schedulers cannot be installed on new projects, and existing bindings will not fire.")}
            </span>
          </div>
          <Controller
            control={control}
            name="is_enabled"
            render={({ field: { value, onChange } }) => <ToggleSwitch value={value} onChange={onChange} size="sm" />}
          />
        </div>

        <div className="flex items-center justify-end gap-3 border-t border-subtle pt-4">
          <Button variant="secondary" onClick={onClose} disabled={isSubmitting} type="button">
            {t("Cancel")}
          </Button>
          <Button variant="primary" type="submit" loading={isSubmitting} disabled={isSubmitting}>
            {isEdit ? (isSubmitting ? t("Saving…") : t("Save")) : isSubmitting ? t("Creating…") : t("Create scheduler")}
          </Button>
        </div>
      </form>
    </ModalCore>
  );
});
